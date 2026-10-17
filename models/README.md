# models/

ONNX modelleri **repoya ve Docker imajına gömülmez**; sabit Hugging Face revizyonlarından
indirilip özetle (sha256 / git blob sha1) doğrulanır. Doğrulanmamış dosya hedefe yazılmaz.

```bash
# Docker ile (host kullanıcısıyla ./models'e yazar; OSIRIS_UID/OSIRIS_GID varsayılan 1000)
docker compose --profile models run --rm osiris-models

# Yerel ikiliyle
osiris-server models                 # hepsi (~420 MB), varsayılan $OSIRIS_MODEL_DIR veya ./models
osiris-server models --only ner      # yalnızca NER
```

| Ad | Kaynak | Dosya | Lisans | Kullanım |
|---|---|---|---|---|
| `ner` | `onnx-community/bert-base-multilingual-cased-ner-hrl-ONNX` | `onnx/model_int8.onnx` (178 MB) | AFL-3.0 | Kişi/kurum/yer çıkarma (K4) |
| `embedding` | `Qdrant/paraphrase-multilingual-MiniLM-L12-v2-onnx-Q` | `model_optimized.onnx` (235 MB) | Apache-2.0 | 384 boyutlu semantik vektör (K5) |

Revizyonlar ve özetler: `crates/osiris-pipeline/src/models.rs`. İşçi modelleri
`OSIRIS_MODEL_DIR` altında arar (compose: `./models` → `/app/models`, salt-okunur). Model yoksa
yalnızca regex varlık çıkarma çalışır, vektör üretilmez. Sözleşme:
[docs/SOZLESME.md](../docs/SOZLESME.md) §10–11.

**Lisans notu (NER):** AFL-3.0 izin vericidir ancak FSF'ye göre GPL ile uyumsuz kabul
edilir. Model OSIRIS ile birlikte dağıtılmaz; çalışma zamanında kullanıcı tarafından
indirilir. Model dosyasını OSIRIS'le birlikte paketleyip dağıtacaksanız lisansı
ayrıca değerlendirin.
