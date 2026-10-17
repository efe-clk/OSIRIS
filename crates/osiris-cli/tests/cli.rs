//! `osiris` ikilisinin çıkış kodları ve çıktısı (Python `osiris-cli/tests` ile aynı durumlar).

use std::process::Command;

fn osiris(args: &[&str]) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_osiris"))
        .args(args)
        // ulaşılamayan Redis: kuyruk yolu hiçbir testte başarıya ulaşmaz
        .env("OSIRIS_REDIS_URL", "redis://127.0.0.1:1/0")
        .env_remove("OSIRIS_DATABASE_URL")
        .output()
        .expect("osiris çalıştı");
    let text =
        String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    (out.status.code().unwrap_or(-1), text)
}

#[test]
fn durum_ve_surum() {
    let (code, out) = osiris(&["status"]);
    assert_eq!(code, 0);
    assert!(out.contains("OSIRIS"), "{out}");
    let (code, out) = osiris(&["--version"]);
    assert_eq!(code, 0);
    assert!(out.starts_with("osiris "), "{out}");
}

#[test]
fn pluginler() {
    let (code, out) = osiris(&["plugins"]);
    assert_eq!(code, 0);
    assert!(out.contains("11 plugin yüklendi:"), "{out}");
    assert!(out.contains("  irc — "), "{out}");
    assert_eq!(osiris(&["plugins", "--plugins-dir", "x"]).0, 0);
}

#[test]
fn toplama_cikis_kodlari() {
    assert_eq!(osiris(&["collect", "rss", "--config", "{bozuk"]).0, 2);
    assert_eq!(osiris(&["collect", "rss", "--config", "[1]"]).0, 2);
    let (code, out) = osiris(&["collect", "yok-boyle-plugin"]);
    assert_eq!(code, 1);
    assert!(out.contains("bulunamadı"), "{out}");
    // rss {} → zorunlu alan eksik
    let (code, out) = osiris(&["collect", "rss", "--config", "{}"]);
    assert_eq!(code, 1);
    assert!(out.contains("Hata"), "{out}");
    // SSRF reddi
    let (code, out) =
        osiris(&["collect", "rss", "--config", r#"{"feed_url":"http://127.0.0.1/"}"#]);
    assert_eq!(code, 1, "{out}");
    assert_eq!(osiris(&["bilinmeyen"]).0, 2);
    assert_eq!(osiris(&[]).0, 2);
}
