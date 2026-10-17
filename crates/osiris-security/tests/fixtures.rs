//! `fixtures/security/cases.json` — Python referansıyla aynı sonuç ve aynı hata iletisi.

use osiris_security::{CheckError, Resolver};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::PathBuf;

struct MapResolver(HashMap<String, Vec<String>>);

impl Resolver for MapResolver {
    type Error = std::convert::Infallible;

    fn resolve(&self, host: &str) -> Result<Vec<String>, Self::Error> {
        Ok(self.0.get(host).cloned().unwrap_or_default())
    }
}

fn outcome<T: serde::Serialize>(r: Result<T, String>) -> Value {
    match r {
        Ok(v) => json!({ "ok": v }),
        Err(e) => json!({ "error": e }),
    }
}

fn denied<T>(r: Result<T, CheckError<std::convert::Infallible>>) -> Result<T, String> {
    r.map_err(|e| match e {
        CheckError::Denied(d) => d.0,
        CheckError::Resolver(never) => match never {},
    })
}

#[test]
fn security_fixtures() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/security/cases.json");
    let data: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let dns: HashMap<String, Vec<String>> = serde_json::from_value(data["dns"].clone()).unwrap();
    let resolver = MapResolver(dns);
    let cases = data["cases"].as_array().unwrap();
    assert!(cases.len() > 50);
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let args = &case["args"];
        let s = |k: &str| args[k].as_str().unwrap_or_default().to_owned();
        let n = |k: &str| args[k].as_u64().unwrap() as usize;
        let got = match case["fn"].as_str().unwrap() {
            "assert_safe_url" => outcome(denied(osiris_security::assert_safe_url(
                &s("url"),
                osiris_security::UrlPolicy {
                    allow_onion: args["allow_onion"].as_bool().unwrap(),
                    allow_i2p: args["allow_i2p"].as_bool().unwrap_or(false),
                },
                &resolver,
            ))),
            "resolve_safe_ips" => {
                outcome(denied(osiris_security::resolve_safe_ips(&s("host"), &resolver)))
            }
            "sanitize_domain" => {
                outcome(osiris_security::sanitize_domain(&s("value")).map_err(|e| e.0))
            }
            "sanitize_hostname" => outcome(
                osiris_security::sanitize_hostname(&s("value"), n("max_len")).map_err(|e| e.0),
            ),
            "sanitize_irc_token" => outcome(
                osiris_security::sanitize_irc_token(&s("value"), n("max_len")).map_err(|e| e.0),
            ),
            "sanitize_channel" => {
                outcome(osiris_security::sanitize_channel(&s("value")).map_err(|e| e.0))
            }
            "mask_secret" => outcome::<String>(Ok(osiris_security::mask_secret(
                args["value"].as_str(),
                n("keep"),
            ))),
            other => panic!("bilinmeyen fn: {other}"),
        };
        assert_eq!(got, case["expected"], "{name}");
    }
}
