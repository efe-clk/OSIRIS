//! Kimlik doğrulama — Python `osiris_api.auth` + `main.resolve_identity` ile aynı kurallar.
//!
//! - Sıra: operatör `X-API-Key` (sabit süreli karşılaştırma) → `Authorization: Bearer <JWT>`
//!   → DB anahtarı (`api_keys.key_hash` = SHA-256, iptal edilmemiş).
//! - JWT: yalnızca `HS256` kabul edilir (`none` dahil başkası reddedilir), sır yalnızca
//!   `OSIRIS_JWT_SECRET` (API anahtarına düşme yedeği yok), 60 sn saat kayması toleransı,
//!   `role` yoksa `viewer`, bilinmeyen rol reddedilir. Python'un ürettiği jetonlar doğrulanır.
//! - Roller: viewer < analyst < admin.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{Hmac, KeyInit, Mac};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

pub const ROLES: [&str; 3] = ["viewer", "analyst", "admin"];
const ISSUER: &str = "osiris";

/// Rol sırası (bilinmeyen = 0).
pub fn rank(role: &str) -> u8 {
    match role {
        "viewer" => 1,
        "analyst" => 2,
        "admin" => 3,
        _ => 0,
    }
}

/// `role_satisfies(have, need)` — bilinmeyen `have` yetkisiz, bilinmeyen `need` ulaşılamaz.
pub fn role_satisfies(have: &str, need: &str) -> bool {
    let need = match rank(need) {
        0 => 99,
        n => n,
    };
    rank(have) >= need
}

/// API anahtarının SHA-256 özeti (DB'de ham değer saklanmaz).
pub fn hash_api_key(raw: &str) -> String {
    let d = Sha256::digest(raw.as_bytes());
    d.iter().map(|b| format!("{b:02x}")).collect()
}

/// `hmac.compare_digest` karşılığı.
pub fn constant_eq(a: &str, b: &str) -> bool {
    a.len() == b.len() && bool::from(a.as_bytes().ct_eq(b.as_bytes()))
}

/// `osiris_<token_urlsafe(32)>` — 32 rastgele bayt, base64url (dolgusuz).
pub fn new_api_key() -> Result<String, getrandom::Error> {
    let mut buf = [0u8; 32];
    getrandom::fill(&mut buf)?;
    Ok(format!("osiris_{}", URL_SAFE_NO_PAD.encode(buf)))
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn sign(secret: &str, input: &str) -> Vec<u8> {
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(secret.as_bytes())
        .expect("HMAC her anahtar uzunluğunu kabul eder");
    mac.update(input.as_bytes());
    mac.finalize().into_bytes().to_vec()
}

/// `create_token` — 60 sn ile 24 saat arası ömür.
pub fn create_token(
    subject: &str,
    secret: &str,
    ttl_seconds: i64,
    role: &str,
) -> Result<Value, String> {
    if secret.is_empty() || subject.is_empty() {
        return Err("subject ve secret gerekli".into());
    }
    if rank(role) == 0 {
        return Err(format!("Geçersiz rol: {role:?}"));
    }
    let ttl = ttl_seconds.clamp(60, 86_400);
    let iat = now();
    let sub: String = subject.chars().take(200).collect();
    // Python json.dumps(separators=(",", ":")) ile aynı alan sırası
    let header = r#"{"alg":"HS256","typ":"JWT"}"#;
    let claims = format!(
        r#"{{"iss":"{ISSUER}","sub":{},"role":"{role}","iat":{iat},"exp":{}}}"#,
        Value::String(sub),
        iat + ttl
    );
    let input = format!("{}.{}", URL_SAFE_NO_PAD.encode(header), URL_SAFE_NO_PAD.encode(claims));
    let sig = URL_SAFE_NO_PAD.encode(sign(secret, &input));
    Ok(json!({"access_token": format!("{input}.{sig}"), "token_type": "bearer", "expires_in": ttl}))
}

fn b64(part: &str) -> Result<Vec<u8>, String> {
    if part.is_empty() || part.len() > 8192 {
        return Err("Geçersiz base64url".into());
    }
    URL_SAFE_NO_PAD.decode(part.trim_end_matches('=')).map_err(|_| "Geçersiz base64url".into())
}

/// `verify_token_with_role` → `(sub, role)`.
pub fn verify_token(token: &str, secret: &str, leeway: i64) -> Result<(String, String), String> {
    if secret.is_empty() {
        return Err("JWT gizli anahtarı tanımsız".into());
    }
    if token.is_empty() || token.len() > 8192 {
        return Err("Jeton boş veya çok uzun".into());
    }
    let parts: Vec<&str> = token.split('.').collect();
    let [h, c, s] = parts.as_slice() else { return Err("Jeton biçimi geçersiz".into()) };
    let decoded = (|| Ok::<_, String>((b64(h)?, b64(c)?, b64(s)?)))()
        .map_err(|_| "Jeton çözümlenemedi".to_owned())?;
    let header: Value = serde_json::from_slice(&decoded.0).map_err(|_| "Jeton çözümlenemedi")?;
    let claims: Value = serde_json::from_slice(&decoded.1).map_err(|_| "Jeton çözümlenemedi")?;
    if header.get("alg").and_then(Value::as_str) != Some("HS256") {
        return Err("Desteklenmeyen imza algoritması".into());
    }
    let expected = sign(secret, &format!("{h}.{c}"));
    if !bool::from(expected.as_slice().ct_eq(decoded.2.as_slice())) {
        return Err("İmza geçersiz".into());
    }
    let Value::Object(claims) = claims else { return Err("Claim biçimi geçersiz".into()) };
    match claims.get("exp").and_then(Value::as_i64) {
        Some(exp) if now() <= exp + leeway => {}
        _ => return Err("Jeton süresi dolmuş".into()),
    }
    let sub = match claims.get("sub") {
        Some(Value::String(s)) if !s.is_empty() => s.clone(),
        _ => return Err("Subject yok".into()),
    };
    let role = match claims.get("role") {
        None => "viewer".to_owned(),
        Some(Value::String(r)) if rank(r) > 0 => r.clone(),
        Some(other) => return Err(format!("Bilinmeyen rol: {other}")),
    };
    Ok((sub, role))
}

/// Çözülmüş kimlik.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ident {
    pub user: String,
    pub role: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jwt_gidis_donus_ve_reddler() {
        let t = create_token("api-client", "s3cret", 3600, "analyst").unwrap();
        let tok = t["access_token"].as_str().unwrap();
        assert_eq!(
            verify_token(tok, "s3cret", 60).unwrap(),
            ("api-client".into(), "analyst".into())
        );
        assert!(verify_token(tok, "baska", 60).is_err());
        assert!(verify_token(tok, "", 60).is_err());
        assert!(verify_token("a.b.c", "s3cret", 60).is_err());
        // alg=none
        let none = format!(
            "{}.{}.",
            URL_SAFE_NO_PAD.encode(r#"{"alg":"none"}"#),
            tok.split('.').nth(1).unwrap()
        );
        assert!(verify_token(&none, "s3cret", 60).is_err());
        // süresi dolmuş
        let h = URL_SAFE_NO_PAD.encode(r#"{"alg":"HS256","typ":"JWT"}"#);
        let c = URL_SAFE_NO_PAD.encode(r#"{"sub":"x","exp":1}"#);
        let s = URL_SAFE_NO_PAD.encode(sign("k", &format!("{h}.{c}")));
        assert_eq!(
            verify_token(&format!("{h}.{c}.{s}"), "k", 60).unwrap_err(),
            "Jeton süresi dolmuş"
        );
        // rol yok → viewer; bilinmeyen rol → red
        let exp = now() + 100;
        for (claims, want) in [
            (format!(r#"{{"sub":"x","exp":{exp}}}"#), Ok(("x".to_owned(), "viewer".to_owned()))),
            (format!(r#"{{"sub":"x","exp":{exp},"role":"root"}}"#), Err(())),
        ] {
            let c = URL_SAFE_NO_PAD.encode(claims);
            let s = URL_SAFE_NO_PAD.encode(sign("k", &format!("{h}.{c}")));
            assert_eq!(verify_token(&format!("{h}.{c}.{s}"), "k", 60).map_err(|_| ()), want);
        }
        assert!(create_token("x", "k", 10, "root").is_err());
        assert_eq!(create_token("x", "k", 10, "viewer").unwrap()["expires_in"], 60);
    }

    #[test]
    fn roller_ve_anahtarlar() {
        assert!(role_satisfies("admin", "analyst"));
        assert!(!role_satisfies("viewer", "analyst"));
        assert!(!role_satisfies("root", "viewer"));
        assert!(!role_satisfies("admin", "superuser"));
        assert_eq!(
            hash_api_key("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let k = new_api_key().unwrap();
        assert!(k.starts_with("osiris_") && k.len() == 7 + 43, "{k}");
        assert!(constant_eq("a", "a") && !constant_eq("a", "b") && !constant_eq("a", "ab"));
    }
}
