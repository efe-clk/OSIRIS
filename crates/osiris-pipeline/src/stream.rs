//! Redis Streams tüketicisi — Python `ProcessingPipeline.run` ile aynı tur (SOZLESME §5):
//! (1) eski LIST'i boşalt, (2) `XAUTOCLAIM` (300 sn boşta), (3) `XREADGROUP >`.
//! Başarı → `XACK`+`XDEL`; bozuk kayıt ya da 5'ten fazla teslim → ölü-mektup akışı;
//! depolama hatası → bekleyen listesinde kalır (yeniden denenir).

use crate::{Invalid, Pipeline};
use redis::Value as RV;
use redis::aio::MultiplexedConnection;
use sqlx::PgPool;

pub const STREAM: &str = "osiris:stream:raw_items";
pub const DEAD_LETTER: &str = "osiris:stream:raw_items:dead";
pub const GROUP: &str = "osiris-pipeline";
pub const LEGACY_LIST: &str = "osiris:raw_items";
pub const FIELD: &str = "data";
pub const MAX_DELIVERIES: i64 = 5;
pub const CLAIM_IDLE_MS: u64 = 300_000;

/// `OSIRIS_STREAM_MAXLEN` (1000–10 000 000; varsayılan 200 000).
pub fn stream_maxlen() -> i64 {
    std::env::var("OSIRIS_STREAM_MAXLEN")
        .ok()
        .and_then(|v| v.trim().parse::<i64>().ok())
        .map_or(200_000, |v| v.clamp(1_000, 10_000_000))
}

/// `XADD stream MAXLEN ~ n * data <payload>`
pub async fn enqueue(
    con: &mut MultiplexedConnection,
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

fn as_str(v: &RV) -> Option<String> {
    match v {
        RV::BulkString(b) => Some(String::from_utf8_lossy(b).into_owned()),
        RV::SimpleString(s) => Some(s.clone()),
        RV::VerbatimString { text, .. } => Some(text.clone()),
        _ => None,
    }
}

fn as_seq(v: &RV) -> Option<&Vec<RV>> {
    match v {
        RV::Array(a) | RV::Set(a) => Some(a),
        _ => None,
    }
}

/// `[id, [k, v, …]]` → (id, alanlar); alanlar yoksa (silinmiş girdi) None.
/// Akış girdisi: kimlik + alanlar (silinmişse None).
type Entry = (String, Option<Vec<(String, String)>>);

fn parse_entry(e: &RV) -> Option<Entry> {
    let parts = as_seq(e)?;
    let id = as_str(parts.first()?)?;
    let fields = parts.get(1).and_then(as_seq).map(|kv| {
        kv.chunks(2).filter_map(|c| Some((as_str(c.first()?)?, as_str(c.get(1)?)?))).collect()
    });
    Some((id, fields))
}

pub struct Consumer {
    pub pipeline: Pipeline,
    pub pool: Option<PgPool>,
    pub con: MultiplexedConnection,
    pub stream: String,
    pub group: String,
    pub consumer: String,
    pub legacy: Option<String>,
    embedding_v2: Option<bool>,
    group_ready: bool,
}

impl Consumer {
    pub fn new(
        pipeline: Pipeline,
        pool: Option<PgPool>,
        con: MultiplexedConnection,
        consumer: String,
    ) -> Self {
        Self {
            pipeline,
            pool,
            con,
            stream: STREAM.into(),
            group: GROUP.into(),
            consumer,
            legacy: Some(LEGACY_LIST.into()),
            embedding_v2: None,
            group_ready: false,
        }
    }

    async fn store(&mut self, raw: &str) -> Result<Option<String>, StoreErr> {
        let p = self.pipeline.process_one(raw).map_err(StoreErr::Invalid)?;
        let Some(pool) = self.pool.clone() else {
            tracing::debug!("database_url yok, depolama atlandı");
            return Ok(None);
        };
        if self.embedding_v2 != Some(true) {
            self.embedding_v2 = Some(
                osiris_db::has_column(&pool, "items", "embedding_v2")
                    .await
                    .map_err(StoreErr::Db)?,
            );
        }
        let item = self.pipeline.new_item(&p);
        let id = osiris_db::store(&pool, &item, self.embedding_v2 == Some(true))
            .await
            .map_err(StoreErr::Db)?;
        tracing::info!("Depolandı: {} ({})", &p.content_hash[..12], id.as_deref().unwrap_or("-"));
        Ok(id)
    }

    async fn ack(&mut self, id: &str) -> redis::RedisResult<()> {
        redis::cmd("XACK")
            .arg(&self.stream)
            .arg(&self.group)
            .arg(id)
            .query_async::<()>(&mut self.con)
            .await?;
        redis::cmd("XDEL").arg(&self.stream).arg(id).query_async::<()>(&mut self.con).await
    }

    async fn dead_letter(
        &mut self,
        id: &str,
        fields: &[(String, String)],
        reason: &str,
    ) -> redis::RedisResult<()> {
        let mut c = redis::cmd("XADD");
        c.arg(DEAD_LETTER).arg("MAXLEN").arg("~").arg(10_000).arg("*");
        for (k, v) in fields {
            c.arg(k).arg(v);
        }
        c.arg("source_id").arg(id).arg("reason").arg(reason);
        let res = c.query_async::<String>(&mut self.con).await;
        self.ack(id).await?;
        res.map(|_| ())
    }

    async fn deliveries(&mut self, id: &str) -> i64 {
        let r: redis::RedisResult<RV> = redis::cmd("XPENDING")
            .arg(&self.stream)
            .arg(&self.group)
            .arg(id)
            .arg(id)
            .arg(1)
            .query_async(&mut self.con)
            .await;
        r.ok()
            .as_ref()
            .and_then(as_seq)
            .and_then(|a| a.first())
            .and_then(as_seq)
            .and_then(|e| e.get(3))
            .and_then(|v| match v {
                RV::Int(i) => Some(*i),
                other => as_str(other)?.parse().ok(),
            })
            .unwrap_or(0)
    }

    /// Tek girdi: başarı → 1; kalıcı hata → 0 (ölü-mektup); geçici hata → 0 (bekler).
    async fn handle(
        &mut self,
        id: &str,
        fields: Vec<(String, String)>,
    ) -> redis::RedisResult<usize> {
        let raw =
            fields.iter().find(|(k, _)| k == FIELD).map(|(_, v)| v.clone()).unwrap_or_default();
        match self.store(&raw).await {
            Ok(_) => {
                self.ack(id).await?;
                Ok(1)
            }
            Err(StoreErr::Invalid(Invalid(e))) => {
                tracing::warn!("Bozuk kayıt atlandı ({id}): {e}");
                let reason: String = format!("invalid: {e}").chars().take(200).collect();
                self.dead_letter(id, &fields, &reason).await?;
                Ok(0)
            }
            Err(StoreErr::Db(e)) => {
                tracing::error!("Depolama hatası ({id}, yeniden denenecek): {e}");
                Ok(0)
            }
        }
    }

    async fn drain_legacy(&mut self, limit: usize) -> (usize, usize) {
        let Some(list) = self.legacy.clone() else { return (0, 0) };
        let (mut stored, mut consumed) = (0, 0);
        for _ in 0..limit {
            let raw: Option<String> =
                match redis::cmd("LPOP").arg(&list).query_async(&mut self.con).await {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::error!("Eski kuyruk okunamadı: {e}");
                        break;
                    }
                };
            let Some(raw) = raw else { break };
            consumed += 1;
            match self.store(&raw).await {
                Ok(_) => stored += 1,
                Err(StoreErr::Invalid(Invalid(e))) => tracing::warn!("Bozuk kayıt atlandı: {e}"),
                Err(StoreErr::Db(e)) => {
                    tracing::error!("Depolama hatası (eski kuyruk; kayıt akışa taşındı): {e}");
                    let stream = self.stream.clone();
                    if enqueue(&mut self.con, &stream, &raw).await.is_err() {
                        tracing::error!("Kayıt akışa taşınamadı, kayboldu");
                    }
                }
            }
        }
        (stored, consumed)
    }

    /// Bir tur; depolanan kayıt sayısı.
    pub async fn run(&mut self, batch: usize) -> usize {
        let batch = batch.clamp(1, 100);
        let (mut stored, consumed) = self.drain_legacy(batch).await;
        let mut remaining = batch.saturating_sub(consumed);
        if remaining == 0 {
            return stored;
        }
        if let Err(e) = self.read_stream(&mut stored, &mut remaining).await {
            tracing::error!("Akış okunamadı: {e}");
        }
        stored
    }

    async fn read_stream(
        &mut self,
        stored: &mut usize,
        remaining: &mut usize,
    ) -> redis::RedisResult<()> {
        if !self.group_ready {
            let r: redis::RedisResult<()> = redis::cmd("XGROUP")
                .arg("CREATE")
                .arg(&self.stream)
                .arg(&self.group)
                .arg("0")
                .arg("MKSTREAM")
                .query_async(&mut self.con)
                .await;
            if let Err(e) = r
                && !e.to_string().contains("BUSYGROUP")
            {
                return Err(e);
            }
            self.group_ready = true;
        }
        let claimed: RV = redis::cmd("XAUTOCLAIM")
            .arg(&self.stream)
            .arg(&self.group)
            .arg(&self.consumer)
            .arg(CLAIM_IDLE_MS)
            .arg("0-0")
            .arg("COUNT")
            .arg(*remaining)
            .query_async(&mut self.con)
            .await?;
        let entries: Vec<Entry> = as_seq(&claimed)
            .and_then(|a| a.get(1))
            .and_then(as_seq)
            .map(|es| es.iter().filter_map(parse_entry).collect())
            .unwrap_or_default();
        let n = entries.len();
        for (id, fields) in entries {
            let Some(fields) = fields else {
                redis::cmd("XACK")
                    .arg(&self.stream)
                    .arg(&self.group)
                    .arg(&id)
                    .query_async::<()>(&mut self.con)
                    .await?;
                continue;
            };
            if self.deliveries(&id).await > MAX_DELIVERIES {
                self.dead_letter(&id, &fields, "max_deliveries").await?;
                continue;
            }
            *stored += self.handle(&id, fields).await?;
        }
        *remaining = remaining.saturating_sub(n);
        if *remaining > 0 {
            let resp: RV = redis::cmd("XREADGROUP")
                .arg("GROUP")
                .arg(&self.group)
                .arg(&self.consumer)
                .arg("COUNT")
                .arg(*remaining)
                .arg("STREAMS")
                .arg(&self.stream)
                .arg(">")
                .query_async(&mut self.con)
                .await?;
            let mut entries = Vec::new();
            // RESP2: [[stream, [entries]]]; RESP3: {stream: [entries]}
            match &resp {
                RV::Array(streams) => {
                    for s in streams {
                        if let Some(es) = as_seq(s).and_then(|x| x.get(1)).and_then(as_seq) {
                            entries.extend(es.iter().filter_map(parse_entry));
                        }
                    }
                }
                RV::Map(m) => {
                    for (_, es) in m {
                        if let Some(es) = as_seq(es) {
                            entries.extend(es.iter().filter_map(parse_entry));
                        }
                    }
                }
                _ => {}
            }
            for (id, fields) in entries {
                *stored += self.handle(&id, fields.unwrap_or_default()).await?;
            }
        }
        Ok(())
    }
}

enum StoreErr {
    Invalid(Invalid),
    Db(sqlx::Error),
}
