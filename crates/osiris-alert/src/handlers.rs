//! Uyarı işleyicileri: webhook (JSON POST), Telegram Bot API, e-posta (SMTP).
//!
//! Yapılandırma ortamdan ([`from_env`]); hiçbiri tanımlı değilse liste boştur. Hata
//! iletilerine URL yazılmaz (Telegram jetonu ve webhook sırları URL'de taşınır).

use crate::{Handler, alert_payload, get, to_value};
use async_trait::async_trait;
use lettre::message::{Mailbox, header::ContentType};
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use osiris_hash::Json;
use osiris_hash::pystr::py_str;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(10);

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap_or_default()
}

fn field(alert: &Json, key: &str) -> String {
    get(alert, key).map(py_str).unwrap_or_default()
}

/// İnsan okunur tek satır özet (Telegram metni, e-posta konusu).
pub fn summary(alert: &Json) -> String {
    if field(alert, "kind") == "anomaly" {
        let z = match get(alert, "z_score") {
            Some(Json::Null) | None => "∞".to_owned(),
            Some(z) => py_str(z),
        };
        format!("OSIRIS anomali: {} = {} (z = {z})", field(alert, "metric"), field(alert, "value"))
    } else {
        format!(
            "OSIRIS uyarısı: {} — eşleşme: {}",
            field(alert, "query_name"),
            field(alert, "matched")
        )
    }
}

async fn post_json(
    client: &reqwest::Client,
    url: &str,
    body: &serde_json::Value,
) -> Result<(), String> {
    let resp = client.post(url).json(body).send().await.map_err(|e| e.without_url().to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status().as_u16()));
    }
    Ok(())
}

/// Uyarıyı JSON gövdesi olarak POST eder (`OSIRIS_ALERT_WEBHOOK_URL`, örn. n8n webhook'u).
pub struct WebhookHandler {
    url: String,
    client: reqwest::Client,
}

impl WebhookHandler {
    pub fn new(url: &str) -> Self {
        Self { url: url.to_owned(), client: client() }
    }
}

#[async_trait]
impl Handler for WebhookHandler {
    fn name(&self) -> &str {
        "webhook"
    }

    async fn handle(&self, alert: &Json) -> Result<(), String> {
        post_json(&self.client, &self.url, &to_value(alert)).await
    }
}

/// Telegram `sendMessage` (`OSIRIS_ALERT_TELEGRAM_BOT_TOKEN`, `OSIRIS_ALERT_TELEGRAM_CHAT_ID`).
pub struct TelegramHandler {
    token: String,
    chat_id: String,
    api_base: String,
    client: reqwest::Client,
}

impl TelegramHandler {
    pub fn new(token: &str, chat_id: &str) -> Self {
        Self::with_api_base(token, chat_id, "https://api.telegram.org")
    }

    pub fn with_api_base(token: &str, chat_id: &str, api_base: &str) -> Self {
        Self {
            token: token.to_owned(),
            chat_id: chat_id.to_owned(),
            api_base: api_base.trim_end_matches('/').to_owned(),
            client: client(),
        }
    }
}

#[async_trait]
impl Handler for TelegramHandler {
    fn name(&self) -> &str {
        "telegram"
    }

    async fn handle(&self, alert: &Json) -> Result<(), String> {
        let url = format!("{}/bot{}/sendMessage", self.api_base, self.token);
        let body = serde_json::json!({
            "chat_id": self.chat_id,
            "text": format!("{}\n\n{}", summary(alert), alert_payload(alert)),
            "disable_web_page_preview": true,
        });
        post_json(&self.client, &url, &body).await
    }
}

/// SMTP e-postası (`OSIRIS_ALERT_SMTP_URL` — örn. `smtps://kullanici:parola@smtp.example.com`
/// ya da `smtp://…?tls=required`; `OSIRIS_ALERT_EMAIL_FROM`, `OSIRIS_ALERT_EMAIL_TO` virgüllü).
pub struct EmailHandler {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    from: Mailbox,
    to: Vec<Mailbox>,
}

impl EmailHandler {
    pub fn new(smtp_url: &str, from: &str, to: &str) -> Result<Self, String> {
        let transport = AsyncSmtpTransport::<Tokio1Executor>::from_url(smtp_url)
            .map_err(|_| "OSIRIS_ALERT_SMTP_URL geçersiz".to_owned())?
            .timeout(Some(TIMEOUT))
            .build();
        let from = from.parse().map_err(|_| "OSIRIS_ALERT_EMAIL_FROM geçersiz".to_owned())?;
        let to = to
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.parse().map_err(|_| format!("Geçersiz alıcı: {s}")))
            .collect::<Result<Vec<Mailbox>, String>>()?;
        if to.is_empty() {
            return Err("OSIRIS_ALERT_EMAIL_TO boş".into());
        }
        Ok(Self { transport, from, to })
    }

    /// Gönderilecek ileti (test edilebilir).
    pub fn message(&self, alert: &Json) -> Result<Message, String> {
        let mut b = Message::builder().from(self.from.clone()).subject(summary(alert));
        for to in &self.to {
            b = b.to(to.clone());
        }
        let body = serde_json::to_string_pretty(&to_value(alert)).unwrap_or_default();
        b.header(ContentType::TEXT_PLAIN).body(body).map_err(|e| e.to_string())
    }
}

#[async_trait]
impl Handler for EmailHandler {
    fn name(&self) -> &str {
        "email"
    }

    async fn handle(&self, alert: &Json) -> Result<(), String> {
        let msg = self.message(alert)?;
        self.transport.send(msg).await.map(|_| ()).map_err(|e| e.to_string())
    }
}

/// Ortamdan işleyiciler. Eksik/yanlış yapılandırma loglanır, o işleyici atlanır.
pub fn from_env() -> Vec<Box<dyn Handler>> {
    let var =
        |k: &str| std::env::var(k).ok().map(|v| v.trim().to_owned()).filter(|v| !v.is_empty());
    let mut out: Vec<Box<dyn Handler>> = Vec::new();
    if let Some(url) = var("OSIRIS_ALERT_WEBHOOK_URL") {
        out.push(Box::new(WebhookHandler::new(&url)));
    }
    match (var("OSIRIS_ALERT_TELEGRAM_BOT_TOKEN"), var("OSIRIS_ALERT_TELEGRAM_CHAT_ID")) {
        (Some(t), Some(c)) => out.push(Box::new(TelegramHandler::new(&t, &c))),
        (None, None) => {}
        _ => tracing::warn!("Telegram işleyicisi için jeton ve sohbet kimliği birlikte gerekli"),
    }
    if let Some(url) = var("OSIRIS_ALERT_SMTP_URL") {
        let from = var("OSIRIS_ALERT_EMAIL_FROM").unwrap_or_default();
        let to = var("OSIRIS_ALERT_EMAIL_TO").unwrap_or_default();
        match EmailHandler::new(&url, &from, &to) {
            Ok(h) => out.push(Box::new(h)),
            Err(e) => tracing::warn!("E-posta işleyicisi atlandı: {e}"),
        }
    }
    out
}
