//! OSIRIS veri modelleri — `schemas/*.schema.json` ile birebir (docs/SOZLESME.md §4–§5).
//!
//! `tests/schemas.rs` schemars ile üretilen şemayı elle yazılmış JSON Schema'larla
//! karşılaştırır ve örnek değerleri doğrular.

pub mod pyval;

use indexmap::IndexMap;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Plugin yapılandırması (API/DB'den gelen JSON nesnesi).
pub type Config = Map<String, Value>;

/// Plugin'in ürettiği tek veri birimi (`collected_item.schema.json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CollectedItem {
    #[schemars(required)]
    pub url: Option<String>,
    #[schemars(required)]
    pub title: Option<String>,
    pub raw_content: String,
    /// Kaynağın verdiği ham tarih dizgesi (ayrıştırılmaz).
    #[schemars(required)]
    pub published_at: Option<String>,
    pub metadata: Map<String, Value>,
    pub tags: Vec<String>,
}

impl CollectedItem {
    /// Yalnızca içerikle (diğer alanlar boş).
    pub fn new(raw_content: impl Into<String>) -> Self {
        Self {
            url: None,
            title: None,
            raw_content: raw_content.into(),
            published_at: None,
            metadata: Map::new(),
            tags: Vec::new(),
        }
    }
}

/// Bir toplama görevinin sonucu (`collection_result.schema.json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CollectionResult {
    pub success: bool,
    /// `success = false` iken dolu; metni sözleşme dışıdır.
    #[schemars(required)]
    pub error: Option<String>,
    pub items: Vec<CollectedItem>,
    pub metadata: Map<String, Value>,
}

impl CollectionResult {
    pub fn ok(items: Vec<CollectedItem>, metadata: Map<String, Value>) -> Self {
        Self { success: true, error: None, items, metadata }
    }

    pub fn fail(error: impl Into<String>) -> Self {
        Self { success: false, error: Some(error.into()), items: Vec::new(), metadata: Map::new() }
    }
}

/// `osiris:stream:raw_items` girdisinin `data` alanı (`queue_message.schema.json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QueueMessage {
    pub plugin_id: String,
    #[serde(default)]
    pub source_id: Option<String>,
    pub item: CollectedItem,
    /// Unix epoch saniye (kesirli).
    pub collected_at: f64,
}

/// `sources.network_type` / manifest `network_type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum NetworkType {
    Www,
    Tor,
    I2p,
    P2p,
    Freenet,
    Zeronet,
    Irc,
    Matrix,
    Rss,
    Api,
    Blockchain,
    Sdr,
    Custom,
}

/// `config_schema` alan türü.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum FieldType {
    String,
    Integer,
    Number,
    Boolean,
    Array,
    Object,
}

/// `config_schema` alanı.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FieldSpec {
    #[serde(rename = "type")]
    pub kind: FieldType,
    /// true ise null/boş dize kabul edilmez.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// `plugins/<id>/manifest.json` (`manifest.schema.json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub network_type: NetworkType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    pub description: String,
    /// Manifest dosyasındaki sırayla (Python `dict`; hata iletisi ve arayüz alan sırası).
    pub config_schema: IndexMap<String, FieldSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requires_proxy: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule_default: Option<String>,
}

impl Manifest {
    /// `CollectorManager._missing_required`: `required` alan null/""/yok ise adı döner.
    pub fn missing_required(&self, cfg: &Config) -> Vec<String> {
        self.config_schema
            .iter()
            .filter(|(_, spec)| spec.required == Some(true))
            .filter(|(name, _)| match cfg.get(name.as_str()) {
                None | Some(Value::Null) => true,
                Some(Value::String(s)) => s.is_empty(),
                _ => false,
            })
            .map(|(name, _)| name.clone())
            .collect()
    }
}
