//! `osiris-server` — OSIRIS Rust servis ikilisi.
//!
//! Alt komutlar: `api` (REST API), `pipeline` (Stream işçisi), `collector` (zamanlayıcı),
//! `collect` (tek seferlik toplama), `purge`, `migrate`, `models`, `audit-verify`, `health`.
//! `OSIRIS_LOG_FORMAT=json` → yapılandırılmış JSON log.
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
    /// REST API (Python osiris-api'nin yerine; uçlar ve sözleşme aynı)
    Api {
        #[arg(long, env = "OSIRIS_API_ADDR", default_value = "0.0.0.0:8000")]
        addr: String,
        /// Prometheus `/metrics` dinleyicisi (boş = kapalı; API portuna açılmaz)
        #[arg(long, env = "OSIRIS_METRICS_ADDR", default_value = "")]
        metrics_addr: String,
    },
    /// API sağlık denetimi (konteyner HEALTHCHECK): `GET /health` 200 → çıkış 0
    Health {
        #[arg(long, default_value = "127.0.0.1:8000")]
        addr: String,
    },
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
    /// ONNX modellerini indirir ve doğrular (sabit revizyon + özet; varsayılan $OSIRIS_MODEL_DIR)
    Models {
        #[arg(long, env = "OSIRIS_MODEL_DIR", default_value = "models")]
        dir: std::path::PathBuf,
        /// Yalnızca biri: embedding | ner
        #[arg(long, value_parser = ["embedding", "ner"])]
        only: Option<String>,
    },
    /// Veritabanı migration'larını uygular (gömülü db/migrations; tekrar çalıştırılabilir)
    Migrate,
    /// Saklama süresi dolan öğeleri ve yetim varlıkları siler
    Purge,
    /// Denetim zincirinin son N kaydını doğrular (bozuksa çıkış 1)
    AuditVerify {
        #[arg(long, default_value_t = 1000)]
        limit: i64,
    },
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

/// Ctrl-C ya da SIGTERM (`docker stop`) → bekleyen istekler tamamlanıp kapanır.
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        match signal(SignalKind::terminate()) {
            Ok(mut term) => {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {}
                    _ = term.recv() => {}
                }
            }
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
    tracing::info!("Kapanış sinyali alındı");
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let level = if cli.verbose { "debug" } else { "info" };
    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        // PostgreSQL NOTICE'leri (migration'larda "zaten var") varsayılan logu kirletmesin
        format!("{level},sqlx::postgres::notice=warn").into()
    });
    let fmt = tracing_subscriber::fmt().with_writer(std::io::stderr).with_env_filter(filter);
    if std::env::var("OSIRIS_LOG_FORMAT").is_ok_and(|v| v.eq_ignore_ascii_case("json")) {
        fmt.json().flatten_event(true).with_current_span(false).init();
    } else {
        fmt.init();
    }
    match cli.cmd {
        Cmd::Health { addr } => {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let probe = async {
                let mut s = tokio::net::TcpStream::connect(&addr).await?;
                s.write_all(b"GET /health HTTP/1.0\r\nHost: localhost\r\n\r\n").await?;
                let mut buf = Vec::new();
                s.read_to_end(&mut buf).await?;
                Ok::<_, std::io::Error>(
                    buf.starts_with(b"HTTP/1.0 200") || buf.starts_with(b"HTTP/1.1 200"),
                )
            };
            let ok =
                matches!(tokio::time::timeout(Duration::from_secs(5), probe).await, Ok(Ok(true)));
            std::process::exit(if ok { 0 } else { 1 });
        }
        Cmd::Api { addr, metrics_addr } => {
            use osiris_server::api;
            let cfg = api::Config::from_env();
            cfg.check_startup()?;
            let url = cli.database_url.clone().filter(|u| !u.is_empty());
            let url = url.unwrap_or_else(|| api::DEFAULT_DATABASE_URL.to_owned());
            let pool = osiris_db::connect_lazy(&url, 16, Duration::from_secs(5))
                .context("OSIRIS_DATABASE_URL geçersiz")?;
            let redis = collector::redis_manager(&cli.redis_url)?;
            let mgr = collector::Manager::new(
                osiris_plugin::real::host(),
                Some(redis),
                Some(pool.clone()),
            );
            let state = Arc::new(api::AppState::new(cfg, pool, mgr));
            if !metrics_addr.is_empty() {
                let listener = tokio::net::TcpListener::bind(&metrics_addr)
                    .await
                    .with_context(|| format!("ölçüm adresi dinlenemedi: {metrics_addr}"))?;
                let app = api::metrics_router(state.clone());
                tracing::info!("Ölçümler: http://{metrics_addr}/metrics");
                tokio::spawn(async move {
                    if let Err(e) = axum::serve(listener, app).await {
                        tracing::error!("Ölçüm dinleyicisi durdu: {e}");
                    }
                });
            }
            let listener = tokio::net::TcpListener::bind(&addr)
                .await
                .with_context(|| format!("API adresi dinlenemedi: {addr}"))?;
            tracing::info!("OSIRIS API dinliyor: {addr}");
            axum::serve(listener, api::router(state))
                .with_graceful_shutdown(shutdown_signal())
                .await?;
        }
        Cmd::Models { dir, only } => {
            for spec in osiris_pipeline::models::MODELS.iter() {
                if only.as_deref().is_none_or(|o| o == spec.name) {
                    osiris_pipeline::models::fetch(spec, &dir).await?;
                }
            }
        }
        Cmd::Migrate => {
            let pool = pool(&cli.database_url)
                .await?
                .context("--database-url (OSIRIS_DATABASE_URL) gerekli")?;
            let done = osiris_db::migrate(&pool).await?;
            println!("{} migration uygulandı: {}", done.len(), done.join(", "));
        }
        Cmd::AuditVerify { limit } => {
            let pool = pool(&cli.database_url)
                .await?
                .context("--database-url (OSIRIS_DATABASE_URL) gerekli")?;
            let r = osiris_db::verify_chain(&pool, limit).await?;
            println!(
                "{}",
                serde_json::json!({"ok": r.ok, "checked": r.checked, "truncated": r.truncated,
                                   "error": r.error})
            );
            if !r.ok {
                std::process::exit(1);
            }
        }
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
                let done = osiris_pipeline::reembed(&pool, e, 64).await?;
                println!("{done} öğe yeniden vektörleştirildi");
                return Ok(());
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
