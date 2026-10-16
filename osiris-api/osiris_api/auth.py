"""JWT kimlik doğrulama — HS256, yalnızca standart kütüphane (doküman §10.3).

Tasarım notları:
- `alg` sabit `HS256` kabul edilir; `none` dahil başka alg reddedilir
  (algoritma-karışıklığı saldırılarına karşı).
- İmza karşılaştırması `hmac.compare_digest` ile sabit sürede yapılır.
- Gizli anahtar: yalnızca `OSIRIS_JWT_SECRET` (API anahtarına düşme yedeği yok).
"""

from __future__ import annotations

import base64
import hashlib
import hmac
import json
import time

_ISSUER = "osiris"

# Rol hiyerarşisi (doküman §10.3 — RBAC). Sayı büyüdükçe yetki artar.
ROLES: dict[str, int] = {"viewer": 1, "analyst": 2, "admin": 3}


def role_satisfies(have: str, need: str) -> bool:
    """`have` rolü `need` eşiğini karşılıyor mu? Bilinmeyen rol = yetkisiz."""
    return ROLES.get(have, 0) >= ROLES.get(need, 99)


def hash_api_key(raw_key: str) -> str:
    """API anahtarının SHA-256 özeti (DB'de ham değer saklanmaz)."""
    if not raw_key:
        raise ValueError("Anahtar boş olamaz")
    return hashlib.sha256(raw_key.encode()).hexdigest()


def _b64url_encode(data: bytes) -> str:
    return base64.urlsafe_b64encode(data).rstrip(b"=").decode("ascii")


def _b64url_decode(data: str) -> bytes:
    if not data or len(data) > 8192:
        raise ValueError("Geçersiz base64url")
    padding = "=" * (-len(data) % 4)
    try:
        return base64.urlsafe_b64decode(data + padding)
    except (ValueError, base64.binascii.Error) as exc:
        raise ValueError("Geçersiz base64url") from exc


def create_token(subject: str, secret: str, ttl_seconds: int = 3600,
                 role: str = "viewer") -> dict[str, object]:
    """Kısa ömürlü erişim jetonu üretir."""
    if not secret or not subject:
        raise ValueError("subject ve secret gerekli")
    if role not in ROLES:
        raise ValueError(f"Geçersiz rol: {role!r}")
    ttl_seconds = max(60, min(int(ttl_seconds), 86400))
    now = int(time.time())
    header = {"alg": "HS256", "typ": "JWT"}
    claims = {
        "iss": _ISSUER,
        "sub": str(subject)[:200],
        "role": role,
        "iat": now,
        "exp": now + ttl_seconds,
    }
    h_b64 = _b64url_encode(json.dumps(header, separators=(",", ":")).encode())
    c_b64 = _b64url_encode(json.dumps(claims, separators=(",", ":")).encode())
    signing_input = f"{h_b64}.{c_b64}".encode("ascii")
    sig = hmac.new(secret.encode(), signing_input, hashlib.sha256).digest()
    return {
        "access_token": f"{h_b64}.{c_b64}.{_b64url_encode(sig)}",
        "token_type": "bearer",
        "expires_in": ttl_seconds,
    }


def verify_token(token: str, secret: str, leeway_seconds: int = 60) -> str:
    """Jetonu doğrular, `sub` döner. Bozuksa ValueError yükseltir."""
    if not secret:
        raise ValueError("JWT gizli anahtarı tanımsız")
    if not token or len(token) > 8192:
        raise ValueError("Jeton boş veya çok uzun")
    parts = token.split(".")
    if len(parts) != 3:
        raise ValueError("Jeton biçimi geçersiz")
    h_b64, c_b64, sig_b64 = parts
    try:
        header = json.loads(_b64url_decode(h_b64))
        claims = json.loads(_b64url_decode(c_b64))
        signature = _b64url_decode(sig_b64)
    except ValueError as exc:
        raise ValueError("Jeton çözümlenemedi") from exc
    if not isinstance(header, dict) or header.get("alg") != "HS256":
        raise ValueError("Desteklenmeyen imza algoritması")
    expected = hmac.new(
        secret.encode(), f"{h_b64}.{c_b64}".encode("ascii"), hashlib.sha256
    ).digest()
    if not hmac.compare_digest(expected, signature):
        raise ValueError("İmza geçersiz")
    if not isinstance(claims, dict):
        raise ValueError("Claim biçimi geçersiz")
    now = int(time.time())
    exp = claims.get("exp")
    if not isinstance(exp, int) or now > exp + leeway_seconds:
        raise ValueError("Jeton süresi dolmuş")
    sub = claims.get("sub")
    if not sub or not isinstance(sub, str):
        raise ValueError("Subject yok")
    return sub


def verify_token_with_role(token: str, secret: str,
                           leeway_seconds: int = 60) -> tuple[str, str]:
    """Jetonu doğrular, `(sub, role)` döner.

    `role` claim'i olmayan eski jetonlar `viewer` sayılır (geriye uyumlu).
    Bilinmeyen rol reddedilir (fail-closed).
    """
    sub = verify_token(token, secret, leeway_seconds)
    parts = token.split(".")
    claims = json.loads(_b64url_decode(parts[1]))
    role = claims.get("role", "viewer")
    if role not in ROLES:
        raise ValueError(f"Bilinmeyen rol: {role!r}")
    return sub, role
