//! Kuyruk üreticisi (SOZLESME §5) — Python `osiris/queue.py` `enqueue` ile aynı komut.
//!
//! Tüketici (XREADGROUP/XACK/ölü-mektup) `osiris-pipeline::stream` modülündedir; bu crate
//! yalnızca yazma tarafını taşır ki collector, API ve CLI model çalışma zamanını
//! (ort/fastembed) bağlamadan kuyruğa yazabilsin.

use redis::aio::ConnectionLike;

pub const STREAM: &str = "osiris:stream:raw_items";
pub const FIELD: &str = "data";

/// `OSIRIS_STREAM_MAXLEN` (1000–10 000 000; varsayılan 200 000).
pub fn stream_maxlen() -> i64 {
    parse_maxlen(std::env::var("OSIRIS_STREAM_MAXLEN").ok().as_deref())
}

fn parse_maxlen(raw: Option<&str>) -> i64 {
    raw.and_then(|v| v.trim().parse::<i64>().ok()).map_or(200_000, |v| v.clamp(1_000, 10_000_000))
}

/// `XADD stream MAXLEN ~ n * data <payload>`
pub async fn enqueue<C: ConnectionLike + Send>(
    con: &mut C,
    stream: &str,
    payload: &str,
) -> redis::RedisResult<String> {
    redis::cmd("XADD")
        .arg(stream)
        .arg("MAXLEN")
        .arg("~")
        .arg(stream_maxlen())
        .arg("*")
        .arg(FIELD)
        .arg(payload)
        .query_async(con)
        .await
}

#[cfg(test)]
mod tests {
    #[test]
    fn maxlen_siniri() {
        assert_eq!(super::parse_maxlen(None), 200_000);
        assert_eq!(super::parse_maxlen(Some(" 5 ")), 1_000);
        assert_eq!(super::parse_maxlen(Some("99999999999")), 10_000_000);
        assert_eq!(super::parse_maxlen(Some("abc")), 200_000);
    }
}
