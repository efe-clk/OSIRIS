//! `osiris` — OSIRIS komut satırı arayüzü (R6; Python `osiris_cli` ile aynı komutlar ve
//! çıkış kodları: 0 başarı, 1 plugin yok / toplama başarısız, 2 kullanım ya da geçersiz
//! `--config`).
//!
//! `collect` öğeleri Python'daki gibi kuyruğa yazar (`OSIRIS_REDIS_URL`); `OSIRIS_DATABASE_URL`
//! tanımlıysa ve yapılandırmada `source_id` varsa kaynak sağlığı da yazılır.

use clap::{Parser, Subcommand};
use std::process::ExitCode;
use std::time::Duration;

#[derive(Parser)]
#[command(name = "osiris", version, about = "OSIRIS — Açık Kaynak İstihbarat Platformu.")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Sistem durumunu göster.
    Status,
    /// Yüklü plugin'leri listele.
    Plugins {
        /// Geriye uyumluluk: plugin'ler ikiliye gömülüdür, değer yok sayılır.
        #[arg(long, default_value = "plugins", hide = true)]
        plugins_dir: String,
    },
    /// Tek bir koleksiyon görevi çalıştır.
    Collect {
        plugin_id: String,
        /// Görev yapılandırması (JSON)
        #[arg(long, default_value = "{}")]
        config: String,
    },
}

fn manager() -> osiris_collector::Manager {
    let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    let redis_url = var("OSIRIS_REDIS_URL").unwrap_or_else(|| "redis://localhost:6379/0".into());
    let redis = osiris_collector::redis_manager(&redis_url).ok();
    let pool = var("OSIRIS_DATABASE_URL")
        .and_then(|u| osiris_db::connect_lazy(&u, 2, Duration::from_secs(3)).ok());
    osiris_collector::Manager::new(osiris_plugin::real::host(), redis, pool)
}

async fn run(cli: Cli) -> u8 {
    match cli.cmd {
        Cmd::Status => {
            let n = osiris_plugins_count();
            println!(
                "OSIRIS {} — durum: Rust çekirdeği (API, CLI, veri katmanı); {n} plugin",
                env!("CARGO_PKG_VERSION")
            );
            0
        }
        Cmd::Plugins { plugins_dir: _ } => {
            let mgr = manager();
            let mut ids: Vec<&String> = mgr.plugins.keys().collect();
            ids.sort();
            println!("{} plugin yüklendi:", ids.len());
            for id in ids {
                let m = mgr.plugins[id].manifest();
                let net = serde_json::to_value(m.network_type)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned));
                println!("  {id} — {} ({})", m.name, net.unwrap_or_default());
            }
            0
        }
        Cmd::Collect { plugin_id, config } => {
            let cfg = match serde_json::from_str::<serde_json::Value>(&config) {
                Ok(serde_json::Value::Object(m)) => m,
                Ok(_) => {
                    println!("config bir JSON objesi olmalı");
                    return 2;
                }
                Err(e) => {
                    println!("Geçersiz JSON config: {e}");
                    return 2;
                }
            };
            let mgr = manager();
            if !mgr.plugins.contains_key(&plugin_id) {
                println!("Plugin bulunamadı: {plugin_id}");
                return 1;
            }
            match mgr.run_collection(&plugin_id, cfg, true).await {
                Ok(r) if r.success => {
                    println!("{} öğe toplandı", r.items.len());
                    0
                }
                Ok(r) => {
                    println!("Hata: {}", r.error.unwrap_or_default());
                    1
                }
                Err(e) => {
                    println!("Hata: {e}");
                    1
                }
            }
        }
    }
}

fn osiris_plugins_count() -> usize {
    // Manager kurmadan (Redis/DB bağlantısı açmadan) gömülü plugin sayısı
    osiris_collector::Manager::new(osiris_plugin::real::host(), None, None).plugins.len()
}

fn main() -> ExitCode {
    let cli = Cli::parse(); // kullanım hatası → clap çıkış kodu 2
    let rt = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("Çalışma zamanı başlatılamadı: {e}");
            return ExitCode::from(1);
        }
    };
    ExitCode::from(rt.block_on(run(cli)))
}
