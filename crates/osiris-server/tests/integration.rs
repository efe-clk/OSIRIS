//! Gerçek PostgreSQL + Redis'e karşı entegrasyon testleri (R7'de Python `tests/integration`'dan
//! taşındı): denetim zinciri, saklama kuralları, depolama, Redis Streams kuyruğu, embedding.
//!
//! Ortam: `OSIRIS_TEST_DATABASE_URL` (veritabanı adı `_test` ile bitmeli — tablolar TRUNCATE
//! edilir), `OSIRIS_TEST_REDIS_URL` (test başına benzersiz anahtarlar; FLUSHDB yok). Embedding
//! testi modeller kuruluysa koşar (`osiris-server models`). Değişkenler yoksa testler atlanır.

use osiris_hash::Json;
use osiris_pipeline::Pipeline;
use osiris_pipeline::stream::{self, Consumer};
use sqlx::PgPool;
use std::sync::LazyLock;
use std::time::Duration;

/// Testler aynı veritabanını paylaşır: sırayla koşsunlar.
static LOCK: LazyLock<tokio::sync::Mutex<()>> = LazyLock::new(|| tokio::sync::Mutex::new(()));

const TABLES: &str = "audit_logs, item_entities, entity_relations, entities, items, source_metrics, \
                      sources, graph_edges, saved_queries, api_keys, users";

async fn db() -> Option<PgPool> {
    let url = std::env::var("OSIRIS_TEST_DATABASE_URL").ok().filter(|u| !u.is_empty())?;
    let pool = osiris_db::connect(&url).await.expect("test DB'ye bağlanılamadı");
    let name: String =
        sqlx::query_scalar("SELECT current_database()").fetch_one(&pool).await.unwrap();
    assert!(name.ends_with("_test"), "Güvenlik: test DB adı '_test' ile bitmeli (gelen: {name})");
    sqlx::query(sqlx::AssertSqlSafe(format!("TRUNCATE {TABLES} RESTART IDENTITY CASCADE")))
        .execute(&pool)
        .await
        .unwrap();
    Some(pool)
}

fn bad_pool() -> PgPool {
    osiris_db::connect_lazy("postgresql://x:y@127.0.0.1:1/yok_test", 1, Duration::from_millis(300))
        .unwrap()
}

macro_rules! need {
    ($e:expr) => {
        match $e {
            Some(v) => v,
            None => {
                eprintln!("atlandı: OSIRIS_TEST_DATABASE_URL / OSIRIS_TEST_REDIS_URL yok");
                return;
            }
        }
    };
}

fn json(text: &str) -> Json {
    Json::parse_py(text).unwrap()
}

// ------------------------------------------------------------------------ denetim zinciri

#[tokio::test]
async fn audit_v2_jsonb_gidis_donusu() {
    let _g = LOCK.lock().await;
    let pool = need!(db().await);
    let details = [
        r#"{"ratio": 1.0, "big": 1e16, "tiny": 1e-7, "neg0": -0.0}"#,
        r#"{"n": 1152921504606846976, "nested": {"b": [1, 2.5, null, true], "a": "ş"}}"#,
        "null",
        r#"{"q": "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", "emoji": "🔒"}"#,
    ];
    for (i, d) in details.iter().enumerate() {
        let (user, res) = (format!("u{i}"), format!("r{i}"));
        let out = osiris_db::append_audit(&pool, Some(&user), "test.action", Some(&res), &json(d))
            .await
            .unwrap();
        assert_eq!(out.chain_version, 2);
    }
    let r = osiris_db::verify_chain(&pool, 1000).await.unwrap();
    assert_eq!((r.ok, r.checked, r.truncated), (true, 4, false), "{r:?}");
    // kurcalama: ayrıntı değişirse zincir kırılır
    sqlx::query("UPDATE audit_logs SET detail = '{\"ratio\": 2}'::jsonb WHERE id = 1")
        .execute(&pool)
        .await
        .unwrap();
    let r = osiris_db::verify_chain(&pool, 1000).await.unwrap();
    assert!(!r.ok && r.checked == 0, "{r:?}");
    // pencere tablonun başını içermiyorsa `truncated`
    sqlx::query("TRUNCATE audit_logs RESTART IDENTITY").execute(&pool).await.unwrap();
    for _ in 0..3 {
        osiris_db::append_audit(&pool, Some("u"), "a", None, &json("{}")).await.unwrap();
    }
    let r = osiris_db::verify_chain(&pool, 2).await.unwrap();
    assert_eq!((r.ok, r.checked, r.truncated), (true, 2, true));
}

#[tokio::test]
async fn audit_v1_kayitlari_artik_dogrulanmaz() {
    let _g = LOCK.lock().await;
    let pool = need!(db().await);
    // R7 kararı: v1 desteği tamamen kaldırıldı — eski v1 kayıt "bilinmeyen sürüm" raporlanır
    sqlx::query(
        "INSERT INTO audit_logs (user_id, action, resource, detail, prev_hash, hash, chain_version) \
         VALUES ('api', 'search', '', '{\"f\": 1.0}'::jsonb, NULL, repeat('0', 64), 1)",
    )
    .execute(&pool)
    .await
    .unwrap();
    let r = osiris_db::verify_chain(&pool, 1000).await.unwrap();
    assert!(!r.ok, "{r:?}");
    assert!(r.error.as_deref().unwrap_or("").contains("chain_version"), "{r:?}");
}

// --------------------------------------------------------------------- saklama / depolama

async fn source(pool: &PgPool, name: &str, retention: Option<i32>) -> Result<String, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO sources (name, network_type, plugin_id, retention_days) \
         VALUES ($1, 'rss', 'rss', $2) RETURNING id::text",
    )
    .bind(name)
    .bind(retention)
    .fetch_one(pool)
    .await
}

async fn item(pool: &PgPool, source_id: Option<&str>, age_days: i32, text: &str) -> String {
    sqlx::query_scalar(
        "INSERT INTO items (source_id, raw_content, cleaned_content, content_hash, collected_at) \
         VALUES ($1::uuid, $2, $2, md5(random()::text) || md5(random()::text), \
         NOW() - make_interval(days => $3)) RETURNING id::text",
    )
    .bind(source_id)
    .bind(text)
    .bind(age_days)
    .fetch_one(pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn saklama_kurallari() {
    let _g = LOCK.lock().await;
    let pool = need!(db().await);
    let forever = source(&pool, "sonsuz", Some(0)).await.unwrap();
    let short = source(&pool, "kisa", Some(10)).await.unwrap();
    let default = source(&pool, "varsayilan", None).await.unwrap();
    let forever_old = item(&pool, Some(&forever), 999, "a").await;
    let short_old = item(&pool, Some(&short), 11, "b").await;
    let short_new = item(&pool, Some(&short), 9, "c").await;
    item(&pool, Some(&default), 31, "d").await;
    let default_new = item(&pool, Some(&default), 29, "e").await;
    item(&pool, None, 31, "f").await;
    let ents: Vec<String> = sqlx::query_scalar(
        "INSERT INTO entities (type, value, normalized_value) VALUES \
         ('email', 'gider@x.com', 'gider@x.com'), ('email', 'kalir@x.com', 'kalir@x.com') \
         RETURNING id::text",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO item_entities (item_id, entity_id) VALUES ($1::uuid, $2::uuid), ($3::uuid, $4::uuid)",
    )
    .bind(&short_old)
    .bind(&ents[0])
    .bind(&short_new)
    .bind(&ents[1])
    .execute(&pool)
    .await
    .unwrap();
    // partileme de denenir (batch = 2)
    assert_eq!(osiris_db::purge_expired(&pool, 30, 2, 1000).await.unwrap(), (3, 1));
    let mut left: Vec<String> =
        sqlx::query_scalar("SELECT id::text FROM items").fetch_all(&pool).await.unwrap();
    left.sort();
    let mut want = vec![forever_old, short_new, default_new];
    want.sort();
    assert_eq!(left, want);
    let values: Vec<String> =
        sqlx::query_scalar("SELECT value FROM entities").fetch_all(&pool).await.unwrap();
    assert_eq!(values, vec!["kalir@x.com".to_owned()]);
    // varsayılan 0 → yalnızca kaynağa özel pozitif süreler
    assert_eq!(osiris_db::purge_expired(&pool, 0, 1000, 1000).await.unwrap().0, 0);
}

#[tokio::test]
async fn saklama_suresi_check_kisiti() {
    let _g = LOCK.lock().await;
    let pool = need!(db().await);
    for bad in [-1, 36_501] {
        let err = source(&pool, "x", Some(bad)).await.unwrap_err();
        assert!(err.to_string().contains("check"), "{err}");
    }
}

fn msg(text: &str, source_id: Option<&str>) -> String {
    serde_json::json!({"plugin_id": "rss", "source_id": source_id,
                       "item": {"raw_content": text}, "collected_at": 1.0})
    .to_string()
}

fn pipeline() -> Pipeline {
    Pipeline { ner: None, embedder: None }
}

async fn store(pool: &PgPool, p: &Pipeline, raw: &str) -> String {
    let item = p.new_item(&p.process_one(raw).unwrap());
    osiris_db::store(pool, &item).await.unwrap().unwrap()
}

#[tokio::test]
async fn depolama_kaynagi_baglar_silinmis_kaynagi_tolere_eder() {
    let _g = LOCK.lock().await;
    let pool = need!(db().await);
    let sid = source(&pool, "kaynak", None).await.unwrap();
    let p = pipeline();
    let first = store(&pool, &p, &msg("ilk öğe a@b.com", Some(&sid))).await;
    let gone = "3f2504e0-4f89-11d3-9a0c-0305e82c3301"; // hiç var olmamış kaynak
    let second = store(&pool, &p, &msg("ikinci öğe", Some(gone))).await;
    let rows: Vec<(String, Option<String>)> =
        sqlx::query_as("SELECT id::text, source_id::text FROM items")
            .fetch_all(&pool)
            .await
            .unwrap();
    let get = |id: &str| rows.iter().find(|(i, _)| i == id).map(|(_, s)| s.clone()).unwrap();
    assert_eq!(get(&first), Some(sid));
    assert_eq!(get(&second), None);
    let ents: Vec<(String, String)> =
        sqlx::query_as("SELECT type::text, value FROM entities").fetch_all(&pool).await.unwrap();
    assert_eq!(ents, vec![("email".to_owned(), "a@b.com".to_owned())]);
    // aynı içerik ikinci kez: aynı kimlik (content_hash tekilleştirmesi)
    assert_eq!(store(&pool, &p, &msg("ilk öğe a@b.com", None)).await, first);
}

// ---------------------------------------------------------------------- Redis Streams

struct Queue {
    con: redis::aio::MultiplexedConnection,
    stream: String,
    legacy: String,
    dead_before: i64,
}

async fn queue() -> Option<Queue> {
    let url = std::env::var("OSIRIS_TEST_REDIS_URL").ok().filter(|u| !u.is_empty())?;
    let client = redis::Client::open(url).unwrap();
    let mut con = client.get_multiplexed_async_connection().await.unwrap();
    let tag = format!("{:08x}", rand_u32());
    let dead_before: i64 =
        redis::cmd("XLEN").arg(stream::DEAD_LETTER).query_async(&mut con).await.unwrap_or(0);
    Some(Queue {
        con,
        stream: format!("test:{tag}:stream"),
        legacy: format!("test:{tag}:list"),
        dead_before,
    })
}

fn rand_u32() -> u32 {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap();
    h.write_u128(now.as_nanos());
    h.finish() as u32
}

impl Queue {
    fn consumer(&self, pool: Option<PgPool>, name: &str) -> Consumer {
        let mut c = Consumer::new(pipeline(), pool, self.con.clone(), name.to_owned());
        c.stream = self.stream.clone();
        c.legacy = Some(self.legacy.clone());
        c
    }

    async fn push(&mut self, payload: &str) {
        stream::enqueue(&mut self.con, &self.stream, payload).await.unwrap();
    }

    async fn xlen(&mut self, key: &str) -> i64 {
        redis::cmd("XLEN").arg(key).query_async(&mut self.con).await.unwrap()
    }

    async fn pending(&mut self) -> i64 {
        let v: redis::Value = redis::cmd("XPENDING")
            .arg(&self.stream)
            .arg(stream::GROUP)
            .query_async(&mut self.con)
            .await
            .unwrap();
        match v {
            redis::Value::Array(a) => match a.first() {
                Some(redis::Value::Int(n)) => *n,
                _ => 0,
            },
            _ => 0,
        }
    }

    /// Ölü-mektup akışındaki son kaydın alanları.
    async fn last_dead(&mut self) -> Vec<(String, String)> {
        let v: redis::Value = redis::cmd("XREVRANGE")
            .arg(stream::DEAD_LETTER)
            .arg("+")
            .arg("-")
            .arg("COUNT")
            .arg(1)
            .query_async(&mut self.con)
            .await
            .unwrap();
        let text = |v: &redis::Value| match v {
            redis::Value::BulkString(b) => String::from_utf8_lossy(b).into_owned(),
            _ => String::new(),
        };
        let redis::Value::Array(entries) = v else { return Vec::new() };
        let Some(redis::Value::Array(entry)) = entries.first() else { return Vec::new() };
        let Some(redis::Value::Array(kv)) = entry.get(1) else { return Vec::new() };
        kv.chunks(2).map(|c| (text(&c[0]), text(&c[1]))).collect()
    }

    async fn cleanup(&mut self) {
        let _: redis::RedisResult<()> =
            redis::cmd("DEL").arg(&self.stream).arg(&self.legacy).query_async(&mut self.con).await;
    }
}

async fn stored(pool: &PgPool) -> Vec<String> {
    let mut v: Vec<String> =
        sqlx::query_scalar("SELECT raw_content FROM items").fetch_all(pool).await.unwrap();
    v.sort();
    v
}

#[tokio::test]
async fn kuyruk_mutlu_yol_onaylar_ve_siler() {
    let _g = LOCK.lock().await;
    let pool = need!(db().await);
    let mut q = need!(queue().await);
    for t in ["bir", "iki", "üç"] {
        q.push(&msg(t, None)).await;
    }
    assert_eq!(q.consumer(Some(pool.clone()), "c1").run(10).await, 3);
    assert_eq!(stored(&pool).await, vec!["bir", "iki", "üç"]);
    let s = q.stream.clone();
    assert_eq!((q.xlen(&s).await, q.pending().await), (0, 0));
    q.cleanup().await;
}

#[tokio::test]
async fn kuyruk_depolama_hatasi_bekler_sonra_geri_alinir() {
    let _g = LOCK.lock().await;
    let pool = need!(db().await);
    let mut q = need!(queue().await);
    q.push(&msg("tekrar", None)).await;
    let mut c = q.consumer(Some(bad_pool()), "c1");
    assert_eq!(c.run(10).await, 0);
    let s = q.stream.clone();
    assert_eq!((q.pending().await, q.xlen(&s).await), (1, 1));
    c.pool = Some(pool.clone());
    c.claim_idle_ms = 0; // zaman aşımını beklemeden geri al
    assert_eq!(c.run(10).await, 1);
    assert_eq!(stored(&pool).await, vec!["tekrar"]);
    assert_eq!((q.xlen(&s).await, q.pending().await), (0, 0));
    q.cleanup().await;
}

#[tokio::test]
async fn kuyruk_bozuk_kayit_olu_mektuba() {
    let _g = LOCK.lock().await;
    let pool = need!(db().await);
    let mut q = need!(queue().await);
    q.push("{bozuk json").await;
    assert_eq!(q.consumer(Some(pool), "c1").run(10).await, 0);
    let s = q.stream.clone();
    assert_eq!((q.xlen(&s).await, q.pending().await), (0, 0));
    assert_eq!(q.xlen(stream::DEAD_LETTER).await, q.dead_before + 1);
    let last = q.last_dead().await;
    let field =
        |k: &str| last.iter().find(|(f, _)| f == k).map(|(_, v)| v.clone()).unwrap_or_default();
    assert_eq!(field("data"), "{bozuk json");
    assert!(field("reason").starts_with("invalid"), "{last:?}");
    q.cleanup().await;
}

#[tokio::test]
async fn kuyruk_en_cok_teslim_sonra_olu_mektup() {
    let _g = LOCK.lock().await;
    let _pool = need!(db().await);
    let mut q = need!(queue().await);
    q.push(&msg("hep başarısız", None)).await;
    let mut c = q.consumer(Some(bad_pool()), "c1");
    c.claim_idle_ms = 0;
    for _ in 0..stream::MAX_DELIVERIES + 2 {
        c.run(10).await;
    }
    let s = q.stream.clone();
    assert_eq!((q.xlen(&s).await, q.pending().await), (0, 0));
    let last = q.last_dead().await;
    assert!(last.iter().any(|(k, v)| k == "reason" && v == "max_deliveries"), "{last:?}");
    assert_eq!(q.xlen(stream::DEAD_LETTER).await, q.dead_before + 1);
    q.cleanup().await;
}

#[tokio::test]
async fn kuyruk_eski_liste_once_bosaltilir_kayip_yok() {
    let _g = LOCK.lock().await;
    let pool = need!(db().await);
    let mut q = need!(queue().await);
    let _: () = redis::cmd("RPUSH")
        .arg(&q.legacy)
        .arg(msg("eski-1", None))
        .arg(msg("eski-2", None))
        .query_async(&mut q.con)
        .await
        .unwrap();
    q.push(&msg("yeni", None)).await;
    // DB yokken: eski liste boşalır, depolanamayanlar akışa taşınır (kayıp yok)
    let mut c = q.consumer(Some(bad_pool()), "c1");
    assert_eq!(c.run(10).await, 0);
    let llen: i64 = redis::cmd("LLEN").arg(&q.legacy).query_async(&mut q.con).await.unwrap();
    assert_eq!(llen, 0);
    c.pool = Some(pool.clone());
    c.claim_idle_ms = 0;
    assert_eq!(c.run(10).await, 3);
    assert_eq!(stored(&pool).await, vec!["eski-1", "eski-2", "yeni"]);
    let s = q.stream.clone();
    assert_eq!((q.xlen(&s).await, q.pending().await), (0, 0));
    q.cleanup().await;
}

#[tokio::test]
async fn kuyruk_iki_tuketici_asla_cift_islemez() {
    let _g = LOCK.lock().await;
    let pool = need!(db().await);
    let mut q = need!(queue().await);
    for i in 0..20 {
        q.push(&msg(&format!("m{i:02}"), None)).await;
    }
    let mut a = q.consumer(Some(pool.clone()), "a");
    let mut b = q.consumer(Some(pool.clone()), "b");
    let (mut na, mut nb) = (0, 0);
    loop {
        let (x, y) = (a.run(3).await, b.run(3).await);
        na += x;
        nb += y;
        if x + y == 0 {
            break;
        }
    }
    assert_eq!(na + nb, 20);
    assert!(na > 0 && nb > 0, "{na} {nb}");
    let rows = stored(&pool).await;
    assert_eq!(rows, (0..20).map(|i| format!("m{i:02}")).collect::<Vec<_>>());
    q.cleanup().await;
}

#[tokio::test]
async fn kuyruk_collector_mesaji_gidis_donus() {
    let _g = LOCK.lock().await;
    let pool = need!(db().await);
    let mut q = need!(queue().await);
    let sid = source(&pool, "kaynak", None).await.unwrap();
    // collector'ın ürettiği mesaj biçimi (osiris_types::QueueMessage)
    let m = osiris_types::QueueMessage {
        plugin_id: "rss".into(),
        source_id: Some(sid.clone()),
        item: osiris_types::CollectedItem::new("<b>uçtan</b> uca"),
        collected_at: 1.0,
    };
    q.push(&serde_json::to_string(&m).unwrap()).await;
    assert_eq!(q.consumer(Some(pool.clone()), "c1").run(10).await, 1);
    let row: (String, Option<String>) =
        sqlx::query_as("SELECT cleaned_content, source_id::text FROM items")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(row, ("uçtan uca".to_owned(), Some(sid)));
    q.cleanup().await;
}

// ------------------------------------------------------------------------- embedding

fn embedder() -> Option<osiris_pipeline::embed::Embedder> {
    let repo = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let root =
        std::env::var("OSIRIS_MODEL_DIR").map_or_else(|_| repo.join("models"), |d| repo.join(d));
    let name = "sentence-transformers/paraphrase-multilingual-MiniLM-L12-v2";
    osiris_pipeline::embed::Embedder::load(&root, name).ok()
}

#[tokio::test]
async fn embedding_v2_yazilir_ve_semantik_arama_bulur() {
    let _g = LOCK.lock().await;
    let pool = need!(db().await);
    let Some(e) = embedder() else {
        assert!(
            std::env::var("OSIRIS_REQUIRE_MODELS").as_deref() != Ok("1"),
            "embedding modeli yok ama OSIRIS_REQUIRE_MODELS=1"
        );
        eprintln!("atlandı: embedding modeli yok (osiris-server models)");
        return;
    };
    let p = Pipeline { ner: None, embedder: Some(e) };
    let texts = [
        "Ransomware gang encrypts hospital servers and demands bitcoin",
        "Yerel futbol takımı şampiyonluğu kutladı",
        "Neues Kochrezept für Apfelkuchen",
    ];
    let mut ids = Vec::new();
    for t in texts {
        ids.push(store(&pool, &p, &msg(t, None)).await);
    }
    let e = p.embedder.as_ref().unwrap();
    let rows: Vec<(Option<String>, Option<i32>)> =
        sqlx::query_as("SELECT embedding_model, vector_dims(embedding_v2) FROM items")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert!(rows.iter().all(|r| *r == (Some(e.name.clone()), Some(384))), "{rows:?}");
    let q: Vec<f64> = e
        .embed("hastanelere fidye yazılımı saldırısı")
        .unwrap()
        .into_iter()
        .map(f64::from)
        .collect();
    let hits: Vec<serde_json::Value> =
        serde_json::from_str(&osiris_query::semantic_search(&pool, &q, 3).await.unwrap()).unwrap();
    assert_eq!(hits[0]["id"], ids[0]);
    assert!(hits[0]["similarity"].as_f64() > hits[1]["similarity"].as_f64());

    // yeniden vektörleştirme: eski (vektörsüz) öğeler doldurulur, boş metin #skip
    for (i, t) in ["eski öğe sızıntı verisi", "", "başka eski öğe"].iter().enumerate() {
        sqlx::query(
            "INSERT INTO items (raw_content, cleaned_content, content_hash) VALUES ($1, $1, $2)",
        )
        .bind(t)
        .bind(format!("{i:064}"))
        .execute(&pool)
        .await
        .unwrap();
    }
    assert_eq!(osiris_pipeline::reembed(&pool, e, 2).await.unwrap(), 3);
    let skip: String =
        sqlx::query_scalar("SELECT embedding_model FROM items WHERE cleaned_content = ''")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(skip.ends_with("#skip"));
    assert_eq!(osiris_pipeline::reembed(&pool, e, 2).await.unwrap(), 0);
}
