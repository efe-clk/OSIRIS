#!/usr/bin/env python3
"""Tam site DB'si üretir: Maigret + WhatsMyName → sites_full.json.

Kullanım:
  python3 tools/build_full_db.py [--maigret URL-veya-yol] [--wmn URL-veya-yol]
                                 [--out sites_full.json]

Varsayılanlar upstream main dallarından indirir. Çıktı deterministiktir
(isme göre sıralı) ve Maigret-alt-küme formatındadır (collector ile uyumlu).
"""
from __future__ import annotations

import argparse
import ast
import datetime
import json
import sys
import urllib.request
from pathlib import Path

HERE = Path(__file__).parent
DEFAULT_MAIGRET = "https://raw.githubusercontent.com/soxoj/maigret/main/maigret/resources/data.json"
DEFAULT_WMN = "https://raw.githubusercontent.com/WebBreacher/WhatsMyName/main/wmn-data.json"


def load_json(source: str) -> dict:
    if source.startswith(("http://", "https://")):
        req = urllib.request.Request(source, headers={"User-Agent": "OSIRIS-DB-Builder/1.0"})
        with urllib.request.urlopen(req, timeout=120) as resp:  # nosec B310 -- sabit allowlist disi URL degil; CLI argumani
            return json.loads(resp.read().decode("utf-8"))
    return json.loads(Path(source).read_text(encoding="utf-8"))


def as_list(value) -> list[str]:
    """Maigret'in dize-veya-liste alanlarını normalize eder (presenseStrs yazım hatası dahil)."""
    if value is None:
        return []
    if isinstance(value, list):
        return [str(v) for v in value][:10]
    if isinstance(value, str):
        s = value.strip()
        if s.startswith("["):
            try:
                parsed = ast.literal_eval(s)
                if isinstance(parsed, list):
                    return [str(v) for v in parsed][:10]
            except (ValueError, SyntaxError):
                pass
        return [s] if s else []
    return [str(value)]


def norm_tags(value) -> list[str]:
    """Etiketleri kurallı 8 kategoriye eşler (orijinali de korur)."""
    canon = {
        "dev": {"coding", "tech", "programming", "developer", "software",
                "hacking", "security", "dev"},
        "social": {"social", "dating", "messaging", "community", "network"},
        "photo": {"photo", "art", "design", "graphics", "photography"},
        "video": {"video", "streaming", "movies", "film", "tv"},
        "music": {"music", "audio", "sound"},
        "gaming": {"gaming", "games", "esports", "chess"},
        "forum": {"forum", "discussion", "qa", "q&a"},
        "blog": {"blog", "writing", "books", "news", "reading", "publishing"},
    }
    raw = [t.strip().lower().replace(" ", "-")[:30] for t in as_list(value) if t.strip()]
    out: list[str] = []
    for t in raw:
        for cat, members in canon.items():
            if t in members and cat not in out:
                out.append(cat)
        if t not in out:
            out.append(t)
    return out[:10]


def convert_maigret(data: dict) -> tuple[dict, dict]:
    sites = data.get("sites", data) if isinstance(data, dict) else {}
    out, stats = {}, {"toplam": 0, "atlanan": 0}
    for name, rule in sites.items():
        if not isinstance(rule, dict) or name.startswith("_"):
            continue
        stats["toplam"] += 1
        if rule.get("disabled"):
            stats["atlanan"] += 1
            continue
        url = rule.get("urlProbe") or rule.get("url") or ""
        if "{username}" not in str(url):
            stats["atlanan"] += 1  # motor-tabanlı veya şablonsuz
            continue
        check = str(rule.get("checkType") or "status_code")
        if check not in ("status_code", "message", "response_url"):
            stats["atlanan"] += 1
            continue
        presence = as_list(rule.get("presenceStrs")) + as_list(rule.get("presenseStrs"))
        out[str(name)] = {
            "urlMain": str(rule.get("urlMain", ""))[:500],
            "url": str(url)[:1000],
            "checkType": check,
            "presenceStrs": presence[:10],
            "absenceStrs": as_list(rule.get("absenceStrs"))[:10],
            "tags": norm_tags(rule.get("tags")),
        }
    return out, stats


def convert_wmn(data: dict) -> tuple[dict, dict]:
    sites = data.get("sites", []) if isinstance(data, dict) else []
    out, stats = {}, {"toplam": len(sites), "atlanan": 0}
    for entry in sites:
        if not isinstance(entry, dict):
            continue
        name = str(entry.get("name", "")).strip()
        url = str(entry.get("uri_check", ""))
        if not name or "{account}" not in url:
            stats["atlanan"] += 1
            continue
        e_str = str(entry.get("e_string") or "")
        m_str = str(entry.get("m_string") or "")
        if e_str or m_str:
            check, pres, absn = "message", [e_str] if e_str else [], [m_str] if m_str else []
        else:
            check, pres, absn = "status_code", [], []
        tags = [str(entry.get("cat") or "").strip().lower()[:30]]
        tags = [t for t in tags if t]
        out[name] = {
            "url": url.replace("{account}", "{username}")[:1000],
            "checkType": check,
            "presenceStrs": pres,
            "absenceStrs": absn,
            "tags": tags,
        }
    return out, stats


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description="OSIRIS tam site DB üretici")
    ap.add_argument("--maigret", default=DEFAULT_MAIGRET)
    ap.add_argument("--wmn", default=DEFAULT_WMN)
    ap.add_argument("--out", default=str(HERE.parent / "sites_full.json"))
    args = ap.parse_args(argv)

    print("Maigret indiriliyor...", flush=True)
    m_out, m_stats = convert_maigret(load_json(args.maigret))
    print(f"Maigret: {len(m_out)} site ({m_stats['atlanan']} atlandı)", flush=True)
    print("WhatsMyName indiriliyor...", flush=True)
    w_out, w_stats = convert_wmn(load_json(args.wmn))
    print(f"WMN: {len(w_out)} site ({w_stats['atlanan']} atlandı)", flush=True)

    merged = dict(m_out)
    added = 0
    for name, rule in w_out.items():
        if name not in merged:
            merged[name] = rule
            added += 1
    ordered = {"_meta": {
        "format": "maigret-subset-v1",
        "generated_at": datetime.datetime.now(datetime.UTC).isoformat(),
        "sources": {"maigret": len(m_out), "whatsmyname_unique": added},
        "total": len(merged),
    }}
    ordered.update({k: merged[k] for k in sorted(merged)})
    Path(args.out).write_text(json.dumps(ordered, ensure_ascii=False, indent=1),
                              encoding="utf-8")
    print(f"Yazıldı: {args.out} ({len(merged)} site)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
