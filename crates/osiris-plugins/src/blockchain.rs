//! `plugins/blockchain` — EVM JSON-RPC (adres bakiyesi / blok numarası) ve Blockstream (BTC).

use crate::common::{UA, jget, json_str, meta, py_sub, to_value};
use async_trait::async_trait;
use osiris_hash::Json;
use osiris_plugin::{Collector, FetchOptions, Host, HttpError, PluginCrash, fetch};
use osiris_security::UrlPolicy;
use osiris_types::pyval::{py_str, trunc};
use osiris_types::{CollectedItem, CollectionResult, Config, Manifest};
use serde_json::{Value, json};
use std::sync::LazyLock;

const BLOCKSTREAM: &str = "https://blockstream.info/api";
/// `re.match(r"^[A-Za-z0-9]{20,100}$")` — Python `$` sondaki tek `\n`'yi de kabul eder.
static ADDRESS: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"\A[A-Za-z0-9]{20,100}\n?\z").expect("adres deseni"));

pub struct Blockchain(pub Manifest);

/// Python'da yakalanan hata türleri: ValueError ya da RequestException.
enum CallErr {
    Http(HttpError),
    Crash(PluginCrash),
}

impl From<HttpError> for CallErr {
    fn from(e: HttpError) -> Self {
        CallErr::Http(e)
    }
}

async fn rpc(host: &Host, url: &str, method: &str, params: Value) -> Result<Json, CallErr> {
    let mut opts = FetchOptions::new("POST", url)
        .timeout_secs(30)
        .max_bytes(1_000_000)
        .header("User-Agent", UA);
    opts.json = Some(json!({ "jsonrpc": "2.0", "method": method, "params": params, "id": 1 }));
    let resp = fetch(host, opts).await?;
    resp.raise_for_status()?;
    let data = resp.json()?;
    if !matches!(data, Json::Object(_)) {
        return Err(HttpError::Value("RPC: beklenmeyen yanıt biçimi".into()).into());
    }
    Ok(jget(&data, "result").cloned().unwrap_or(Json::Null))
}

fn item(raw: String, metadata: Value, tags: Vec<String>) -> CollectedItem {
    let mut it = CollectedItem::new(trunc(&raw, 1000));
    it.metadata = meta(metadata);
    it.tags = tags;
    it
}

async fn evm_address(
    host: &Host,
    url: &str,
    chain: &str,
    address: &str,
) -> Result<Vec<CollectedItem>, CallErr> {
    if !address.starts_with("0x") || address.chars().count() != 42 {
        return Ok(Vec::new());
    }
    let balance = rpc(host, url, "eth_getBalance", json!([address, "latest"])).await?;
    let txcount = rpc(host, url, "eth_getTransactionCount", json!([address, "latest"])).await?;
    Ok(vec![item(
        format!(
            "address={address} balance_wei={} txcount={}",
            json_str(&balance),
            json_str(&txcount)
        ),
        json!({ "chain": chain, "address": address }),
        vec!["crypto".into()],
    )])
}

async fn btc_address(host: &Host, address: &str) -> Result<Vec<CollectedItem>, CallErr> {
    let opts = FetchOptions::new("GET", &format!("{BLOCKSTREAM}/address/{address}"))
        .timeout_secs(30)
        .max_bytes(1_000_000)
        .header("User-Agent", UA);
    let resp = fetch(host, opts).await?;
    resp.raise_for_status()?;
    let info = resp.json()?;
    if !matches!(info, Json::Object(_)) {
        return Err(HttpError::Value("Blockstream: beklenmeyen yanıt".into()).into());
    }
    let stats = jget(&info, "chain_stats").cloned().unwrap_or(Json::Object(Vec::new()));
    if !matches!(stats, Json::Object(_)) {
        return Err(CallErr::Crash(PluginCrash("AttributeError: 'get'".into())));
    }
    let g = |k: &str| jget(&stats, k).cloned().unwrap_or(Json::Int(0));
    let balance = py_sub(&g("funded_txo_sum"), &g("spent_txo_sum")).map_err(CallErr::Crash)?;
    Ok(vec![item(
        format!("address={address} balance_sat={balance} txcount={}", json_str(&g("tx_count"))),
        json!({ "chain": "bitcoin", "address": address }),
        vec!["crypto".into()],
    )])
}

#[async_trait]
impl Collector for Blockchain {
    fn manifest(&self) -> &Manifest {
        &self.0
    }

    async fn collect(&self, host: &Host, cfg: &Config) -> Result<CollectionResult, PluginCrash> {
        let chain_raw = cfg.get("chain").map_or("ethereum".into(), py_str);
        let chain = trunc(&chain_raw, 50).to_lowercase();
        if let Some(addr) = cfg.get("address").filter(|v| !v.is_null()) {
            let Value::String(address) = addr else {
                return Ok(CollectionResult::fail("Geçersiz adres"));
            };
            if !ADDRESS.is_match(address) {
                return Ok(CollectionResult::fail("Geçersiz adres"));
            }
            let res = if chain == "bitcoin" {
                btc_address(host, address).await
            } else {
                let Some(Value::String(rpc_url)) = cfg.get("rpc_url") else {
                    return Ok(CollectionResult::fail("rpc_url gerekli"));
                };
                match host.assert_safe_url(rpc_url, UrlPolicy::default()).await {
                    Err(e) => Err(CallErr::Http(e)),
                    Ok(_) => evm_address(host, rpc_url, &chain, address).await,
                }
            };
            return match res {
                Ok(items) => Ok(CollectionResult::ok(
                    items,
                    meta(json!({ "chain": chain, "address": address })),
                )),
                Err(CallErr::Crash(c)) => Err(c),
                Err(CallErr::Http(e)) if e.is_request_exception() => {
                    Ok(CollectionResult::fail(format!("Adres sorgusu: {}", e.type_name())))
                }
                Err(CallErr::Http(e)) => {
                    Ok(CollectionResult::fail(trunc(&format!("Adres sorgusu: {e}"), 300)))
                }
            };
        }
        let Some(rpc_url) = crate::common::cfg_str(cfg, "rpc_url") else {
            return Ok(CollectionResult::fail("rpc_url gerekli"));
        };
        if let Err(e) = host.assert_safe_url(rpc_url, UrlPolicy::default()).await {
            return Ok(CollectionResult::fail(format!("Güvensiz rpc_url: {e}")));
        }
        let block = match rpc(host, rpc_url, "eth_blockNumber", json!([])).await {
            Ok(b) => b,
            Err(CallErr::Crash(c)) => return Err(c),
            Err(CallErr::Http(e)) if e.is_request_exception() => {
                return Ok(CollectionResult::fail(format!("RPC hatası: {}", e.type_name())));
            }
            Err(CallErr::Http(_)) => return Ok(CollectionResult::fail("RPC: geçersiz yanıt")),
        };
        let it = item(
            format!("block_number={}", json_str(&block)),
            json!({ "chain": trunc(&chain_raw, 100) }),
            Vec::new(),
        );
        Ok(CollectionResult::ok(vec![it], meta(json!({ "block_number": to_value(&block) }))))
    }

    async fn health_check(&self, host: &Host, cfg: &Config) -> bool {
        let Some(url) = crate::common::cfg_str(cfg, "rpc_url") else { return false };
        let mut opts = FetchOptions::new("POST", url)
            .timeout_secs(10)
            .max_bytes(65536)
            .header("User-Agent", UA);
        opts.json =
            Some(json!({ "jsonrpc": "2.0", "method": "eth_blockNumber", "params": [], "id": 1 }));
        fetch(host, opts).await.is_ok_and(|r| r.status < 500)
    }
}
