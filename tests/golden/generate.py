"""Golden fixture'ların beklenen çıktılarını Python referans uygulamasından üretir.

Kullanım:  python tests/golden/generate.py          # tüm `expected` alanlarını yeniden yazar
           python tests/golden/generate.py --check  # fark varsa 1 ile çıkar (yazmaz)

Beklenen bir değer DEĞİŞİYORSA bu bir sözleşme değişikliğidir: docs/SOZLESME.md
güncellenmeli ve diff gözden geçirilmelidir.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import harness  # noqa: E402


def build() -> dict[Path, object]:
    out: dict[Path, object] = {}
    for area, runner in harness.RUNNERS.items():
        path = harness.FIXTURES / area / "cases.json"
        data = harness.load(path)
        for case in data["cases"]:
            inp = {k: v for k, v in case.items() if k != "expected"}
            case["expected"] = runner(inp)
        out[path] = data
    if harness.embedding_available():  # model yoksa mevcut beklenenler korunur
        path = harness.FIXTURES / "embedding" / "cases.json"
        data = harness.load(path)
        for case in data["cases"]:
            case["expected"] = harness.run_embedding({k: v for k, v in case.items() if k != "expected"})
        out[path] = data
    else:
        print("UYARI: embedding modeli yok (python models/download.py) — embedding fixture'ları atlandı")
    if harness.ner_available():
        path = harness.FIXTURES / "ner" / "cases.json"
        data = harness.load(path)
        for case in data["cases"]:
            case["expected"] = harness.run_ner({k: v for k, v in case.items() if k != "expected"})
        out[path] = data
    else:
        print("UYARI: NER modeli yok (python models/download.py --only ner) — NER fixture'ları atlandı")
    for path in sorted((harness.FIXTURES / "plugins").glob("*.json")):
        case = harness.load(path)
        inp = {k: v for k, v in case.items() if k not in ("expected", "expected_network")}
        result, report = harness.run_plugin(inp)
        case["expected"] = result
        case["expected_network"] = report
        out[path] = case
    return out


def main() -> int:
    check = "--check" in sys.argv
    changed = []
    for path, data in build().items():
        old = harness.load(path)
        if old != data:
            changed.append(path.relative_to(harness.ROOT))
            if not check:
                harness.dump(path, data)
    for p in changed:
        print(("FARK: " if check else "yazıldı: ") + str(p))
    return 1 if (check and changed) else 0


if __name__ == "__main__":
    raise SystemExit(main())
