# models/

ONNX modelleri **repoya gömülmez**; sabit Hugging Face revizyonlarından indirilip
özetle (sha256 / git blob) doğrulanır:

```bash
python models/download.py            # hepsi (~420 MB)
python models/download.py --only ner # yalnızca NER
```

| Ad | Kaynak | Dosya | Lisans | Kullanım |
|---|---|---|---|---|
| `ner` | `onnx-community/bert-base-multilingual-cased-ner-hrl-ONNX` | `onnx/model_int8.onnx` (178 MB) | AFL-3.0 | Kişi/kurum/yer çıkarma (K4) |
| `embedding` | `Qdrant/paraphrase-multilingual-MiniLM-L12-v2-onnx-Q` | `model_optimized.onnx` (235 MB) | Apache-2.0 | 384 boyutlu semantik vektör (K5) |

Pipeline modelleri `OSIRIS_MODEL_DIR` altında arar (varsayılan bu dizin). Sözleşme:
[docs/SOZLESME.md](../docs/SOZLESME.md) §10–11.

**Lisans notu (NER):** AFL-3.0 izin vericidir ancak FSF'ye göre GPL ile uyumsuz kabul
edilir. Model OSIRIS ile birlikte dağıtılmaz; çalışma zamanında kullanıcı tarafından
indirilir. Model dosyasını OSIRIS'le birlikte paketleyip dağıtacaksanız lisansı
ayrıca değerlendirin.
