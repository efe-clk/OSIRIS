//! `osiris-server` — OSIRIS Rust servis ikilisi (R5: veri katmanı; API R6'da eklenir).
//!
//! Alt komutlar: `pipeline` (Stream işçisi), `shadow` (Python çıktısıyla gölge
//! karşılaştırma), `collector` (zamanlayıcı), `collect` (tek seferlik toplama), `purge`.
//! Ortam değişkenleri Python servisleriyle aynıdır (`OSIRIS_DATABASE_URL`,
//! `OSIRIS_REDIS_URL`, `OSIRIS_EMBEDDING_MODEL`, `OSIRIS_MODEL_DIR`, `OSIRIS_NER`,
//! `OSIRIS_RETENTION_DAYS`, `OSIRIS_RETENTION_INTERVAL`, `OSIRIS_PIPELINE_CONSUMER`).

use anyhow::Context;
use clap::{Parser, Subcommand};
use osiris_collector as collector;
use osiris_pipeline::{Pipeline, stream};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Parser)]
#[command(name = "osiris-server", version, about = "OSIRIS Rust servisleri")]
struct Cli {
    #[arg(long, env = "OSIRIS_DATABASE_URL", global = true, hide_env_values = true)]
    database_url: Option<String>,
    #[arg(
        long,
        env = "OSIRIS_REDIS_URL",
        default_value = "redis://localhost:6379/0",
        global = true,
        hide_env_values = true
    )]
    redis_url: String,
    #[arg(short, long, global = true)]
    verbose: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Kuyruk işçisi (Python `osiris-pipeline` ile aynı bayraklar)
    Pipeline {
        #[arg(long, default_value_t = 50, value_parser = clap::value_parser!(u64).range(1..=1000))]
        batch: u64,
        #[arg(long)]
        once: bool,
        #[arg(long = "loop")]
        run_loop: bool,
        #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(u64).range(1..=3600))]
        interval: u64,
        /// --loop'ta saklama temizliği aralığı, sn (0 = kapalı)
        #[arg(long, env = "OSIRIS_RETENTION_INTERVAL", default_value_t = 3600, value_parser = clap::value_parser!(u64).range(0..=604_800))]
        purge_interval: u64,
        #[arg(long)]
        reembed: bool,
        /// Akış anahtarı
        #[arg(long, default_value = stream::STREAM)]
        stream: String,
        /// Boşaltılacak eski LIST kuyruğu (geçiş uyumluluğu)
        #[arg(long, default_value = stream::LEGACY_LIST)]
        queue: String,
    },
    /// Python işçisinin yazdığı öğeleri Rust hattıyla yeniden hesaplayıp karşılaştırır (yazmaz)
    Shadow {
        #[arg(long, default_value_t = 60)]
        minutes: i64,
        #[arg(long, default_value_t = 500)]
        limit: i64,
        #[arg(long = "loop")]
        run_loop: bool,
        #[arg(long, default_value_t = 300)]
        interval: u64,
    },
    /// Collector servisi; zamanlama yalnızca OSIRIS_SCHEDULER=on ile
    Collector {
        #[arg(long, env = "OSIRIS_SCHEDULER", default_value = "off")]
        scheduler: String,
        #[arg(long, default_value_t = 60)]
        refresh: u64,
    },
    /// Tek seferlik toplama; --enqueue olmadan kuru çalıştırma (sonuç JSON olarak basılır)
    Collect {
        #[arg(long)]
        plugin: String,
        #[arg(long, default_value = "{}")]
        config: String,
        #[arg(long)]
        enqueue: bool,
    },
    /// Saklama süresi dolan öğeleri ve yetim varlıkları siler
    Purge,
}

async fn redis_con(url: &str) -> anyhow::Result<redis::aio::MultiplexedConnection> {
    let client = redis::Client::open(url).context("Redis URL geçersiz")?;
    Ok(client.get_multiplexed_async_connection().await?)
}

async fn pool(url: &Option<String>) -> anyhow::Result<Option<sqlx::PgPool>> {
    match url {
        Some(u) if !u.is_empty() => {
            Ok(Some(osiris_db::connect(u).await.context("PostgreSQL bağlantısı")?))
        }
        _ => Ok(None),
    }
}

fn consumer_name() -> String {
    std::env::var("OSIRIS_PIPELINE_CONSUMER").ok().filter(|v| !v.is_empty()).unwrap_or_else(|| {
        let host = std::fs::read_to_string("/etc/hostname")
            .map(|h| h.trim().to_owned())
            .unwrap_or_else(|_| "rust".into());
        format!("{host}-{}", std::process::id())
    })
}

async fn purge(pool: &sqlx::PgPool) -> anyhow::Result<(u64, u64)> {
    Ok(osiris_db::purge_expired(pool, osiris_db::default_retention_days(), 1000, 1000).await?)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let level = if cli.verbose { "debug" } else { "info" };
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| level.into()),
        )
        .init();
    match cli.cmd {
        Cmd::Purge => {
            let pool = pool(&cli.database_url)
                .await?
                .context("--database-url (OSIRIS_DATABASE_URL) gerekli")?;
            let (items, ents) = purge(&pool).await?;
            println!("{items} öğe, {ents} yetim varlık silindi");
        }
        Cmd::Pipeline {
            batch,
            once,
            run_loop,
            interval,
            purge_interval,
            reembed,
            stream: stream_key,
            queue,
        } => {
            let pool = pool(&cli.database_url).await?;
            let pipeline = tokio::task::spawn_blocking(Pipeline::from_env).await?;
            if reembed {
                let pool = pool.context("--reembed için OSIRIS_DATABASE_URL gerekli")?;
                let e = pipeline
                    .embedder
                    .as_ref()
                    .context("--reembed için OSIRIS_EMBEDDING_MODEL gerekli")?;
                let mut done = 0;
                loop {
                    let rows = osiris_db::items_without_embedding(&pool, 64).await?;
                    if rows.is_empty() {
                        break;
                    }
                    for (id, text) in rows {
                        osiris_db::set_embedding(&pool, &id, e.embed(&text).as_deref(), &e.name)
                            .await?;
                        done += 1;
                    }
                    tracing::info!("Yeniden vektörleştirme: {done} öğe");
                }
                println!("{done} öğe yeniden vektörleştirildi");
                return Ok(());
            }
            // Geçiş adımı 1 (OSIRIS_PIPELINE_SHADOW=1): kuyruğa dokunmadan, Python işçisinin
            // yazdıklarını periyodik olarak yeniden hesaplayıp karşılaştır.
            if matches!(
                std::env::var("OSIRIS_PIPELINE_SHADOW").unwrap_or_default().to_lowercase().as_str(),
                "1" | "on" | "true"
            ) {
                let pool = pool.context("gölge mod için OSIRIS_DATABASE_URL gerekli")?;
                tracing::info!("Gölge mod: kuyruk tüketilmiyor; Python çıktısı karşılaştırılıyor");
                loop {
                    match osiris_pipeline::shadow::compare_recent(&pipeline, &pool, 15, 1000).await
                    {
                        Ok(r) if r.clean() => {
                            tracing::info!(report = %r.to_json(), "gölge: uyuşmazlık yok")
                        }
                        Ok(r) => tracing::warn!(report = %r.to_json(), "gölge: UYUŞMAZLIK"),
                        Err(e) => tracing::error!("gölge karşılaştırma başarısız: {e}"),
                    }
                    tokio::time::sleep(Duration::from_secs(300)).await;
                }
            }
            let con = redis_con(&cli.redis_url).await?;
            let mut consumer = stream::Consumer::new(pipeline, pool.clone(), con, consumer_name());
            consumer.stream = stream_key;
            consumer.legacy = Some(queue).filter(|q| !q.is_empty());
            if once || !run_loop {
                let n = consumer.run(batch as usize).await;
                println!("{n} kayıt işlendi");
                return Ok(());
            }
            tracing::info!("Pipeline döngüsü başladı (her {interval}s)");
            let mut next_purge = Instant::now();
            loop {
                if let Some(p) = &pool
                    && purge_interval > 0
                    && Instant::now() >= next_purge
                {
                    next_purge = Instant::now() + Duration::from_secs(purge_interval);
                    if let Err(e) = purge(p).await {
                        tracing::error!("Saklama temizliği başarısız: {e:#}");
                    }
                }
                let n = consumer.run(batch as usize).await;
                if n > 0 {
                    tracing::info!("{n} kayıt işlendi");
                }
                tokio::time::sleep(Duration::from_secs(interval)).await;
            }
        }
        Cmd::Shadow { minutes, limit, run_loop, interval } => {
            let pool = pool(&cli.database_url)
                .await?
                .context("shadow için OSIRIS_DATABASE_URL gerekli")?;
            let pipeline = tokio::task::spawn_blocking(Pipeline::from_env).await?;
            loop {
                let report =
                    osiris_pipeline::shadow::compare_recent(&pipeline, &pool, minutes, limit)
                        .await?;
                println!("{}", report.to_json());
                if !run_loop {
                    std::process::exit(if report.clean() { 0 } else { 1 });
                }
                tokio::time::sleep(Duration::from_secs(interval)).await;
            }
        }
        Cmd::Collect { plugin, config, enqueue } => {
            let cfg: serde_json::Value =
                serde_json::from_str(&config).context("--config JSON nesnesi olmalı")?;
            let cfg = cfg.as_object().cloned().context("--config JSON nesnesi olmalı")?;
            let redis =
                if enqueue { Some(collector::redis_manager(&cli.redis_url)?) } else { None };
            let mgr = collector::Manager::new(
                osiris_plugin::real::host(),
                redis,
                pool(&cli.database_url).await?,
            );
            let res = mgr.run_collection(&plugin, cfg, enqueue).await?;
            println!("{}", serde_json::to_string_pretty(&res)?);
        }
        Cmd::Collector { scheduler, refresh } => {
            let on = matches!(scheduler.to_lowercase().as_str(), "on" | "1" | "true");
            let redis = Some(collector::redis_manager(&cli.redis_url)?);
            let mgr = Arc::new(collector::Manager::new(
                osiris_plugin::real::host(),
                redis,
                pool(&cli.database_url).await?,
            ));
            tracing::info!("Collector başladı: {} plugin", mgr.plugins.len());
            if !on {
                tracing::warn!(
                    "Zamanlama kapalı (OSIRIS_SCHEDULER=on ile açılır); hiçbir kaynak kendiliğinden toplanmaz"
                );
                tokio::signal::ctrl_c().await?;
                return Ok(());
            }
            mgr.schedule_loop(Duration::from_secs(refresh), Duration::from_secs(15)).await?;
        }
    }
    Ok(())
}
