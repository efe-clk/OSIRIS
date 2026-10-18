# models/

Bu dizin OSIRIS'in kullandığı iki ONNX modelini tutar. Modeller **depoya ve Docker imajına
gömülmez**: sabit Hugging Face revizyonlarından indirilir ve özetleriyle (sha256 / git blob sha1)
doğrulanır. Doğrulanamayan dosya hedefe yazılmaz. Dizin içeriği `.gitignore`'dadır.

*English summary: the two ONNX models (NER, embedding) are downloaded on demand from pinned
revisions and hash-verified; they are not shipped with the repository or the image.*

## Modeller

| Ad | Kaynak | Dosya | Boyut | Lisans | Ne için |
|----|--------|-------|-------|--------|---------|
| `ner` | `onnx-community/bert-base-multilingual-cased-ner-hrl-ONNX` | `onnx/model_int8.onnx` | 178 MB | AFL-3.0 | Kişi / kurum / yer çıkarma |
| `embedding` | `Qdrant/paraphrase-multilingual-MiniLM-L12-v2-onnx-Q` | `model_optimized.onnx` | 235 MB | Apache-2.0 | 384 boyutlu çok dilli anlamsal vektör (işçi + semantik arama) |

Revizyonlar ve özetler: `crates/osiris-pipeline/src/models.rs`. Dosyalar şu alt dizinlere
yerleşir: `ner/bert-base-multilingual-cased-ner-hrl/` ve
`embedding/paraphrase-multilingual-MiniLM-L12-v2-onnx-Q/`.

## İndirme

```bash
# Docker ile (host kullanıcısıyla ./models'e yazar; OSIRIS_UID/OSIRIS_GID, varsayılan 1000)
docker compose --profile models run --rm osiris-models

# Yerel ikiliyle
osiris-server models                 # ikisi (~420 MB); hedef $OSIRIS_MODEL_DIR ya da ./models
osiris-server models --only ner      # yalnızca NER
osiris-server models --only embedding
```

Komut tekrar çalıştırılabilir: diskte özeti doğru olan dosyalar yeniden indirilmez. İndirme önce
geçici bir dosyaya yapılır; özet tutmazsa dosya silinir ve komut hata verir.

## Kim kullanır?

| Servis | Model | Davranış |
|--------|-------|----------|
| `osiris-pipeline` | `ner`, `embedding` | NER varlıkları ve öğe vektörleri. NER yoksa ya da `OSIRIS_NER=off` ise yalnızca regex; `OSIRIS_EMBEDDING_MODEL` boşsa ya da model yoksa vektör üretilmez. |
| `osiris-api` | `embedding` | `POST /search/semantic` için sorgu vektörü; ilk aramada yüklenir. Model yoksa bu uç `503` döner, API'nin geri kalanı çalışır. |

Compose `./models`'i iki servise de `/app/models` olarak salt okunur bağlar
(`OSIRIS_MODEL_DIR=/app/models`). Modelleri sonradan kurduysanız vektörsüz öğeler için:
`docker compose run --rm osiris-pipeline pipeline --reembed`.

Model davranışının sözleşmesi: [docs/SOZLESME.md](../docs/SOZLESME.md) §10–11.

## Lisans notu (NER)

AFL-3.0 izin verici bir lisanstır, ancak FSF'ye göre GPL ile uyumsuz kabul edilir. Bu yüzden model
OSIRIS ile birlikte dağıtılmaz; çalışma zamanında kullanıcı tarafından indirilir. Model dosyasını
OSIRIS ile birlikte paketleyip dağıtacaksanız lisansı ayrıca değerlendirin.
