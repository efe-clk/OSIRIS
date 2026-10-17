//! Tor/I2P proxy seçimi — Python `security.resolve_proxy` ile aynı (operatör env'i kazanır).

use serde_json::Value;

const LOCAL: [&str; 2] = ["127.0.0.1", "::1"];

/// `resolve_proxy(cfg_host, cfg_port, env_var, default_port, label)`.
/// `env` = operatörün `OSIRIS_TOR_SOCKS` / `OSIRIS_I2P_HTTP_PROXY` değeri (`host:port`).
pub fn resolve_proxy(
    cfg_host: Option<&Value>,
    cfg_port: Option<&Value>,
    env: Option<&str>,
    env_var: &str,
    default_port: u16,
    label: &str,
) -> Result<(String, u16), String> {
    let env = env.map(str::trim).unwrap_or("");
    let host_in = match cfg_host {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) if s.is_empty() => String::new(),
        Some(v) => osiris_types::pyval::py_str(v).trim().to_owned(),
    };
    if !env.is_empty() {
        let bad = || format!("{env_var} 'host:port' biçiminde olmalı");
        let (env_host, port_s) = env.rsplit_once(':').ok_or_else(bad)?;
        let env_port: i64 =
            osiris_types::pyval::py_int(&Value::String(port_s.into())).map_err(|_| bad())?;
        if env_host.is_empty() || !(1..=65535).contains(&env_port) {
            return Err(bad());
        }
        let env_host = env_host.trim_matches(|c| c == '[' || c == ']').to_owned();
        if !host_in.is_empty() && !LOCAL.contains(&host_in.as_str()) && host_in != env_host {
            return Err(format!(
                "{label} proxy host'u yalnızca operatörün belirlediği {env_var} olabilir"
            ));
        }
        return Ok((env_host, env_port as u16));
    }
    let host = if host_in.is_empty() { "127.0.0.1".to_owned() } else { host_in };
    if !LOCAL.contains(&host.as_str()) {
        return Err(format!("{label} proxy host yalnızca localhost olabilir"));
    }
    let port = match cfg_port {
        None | Some(Value::Null) => i64::from(default_port),
        Some(Value::String(s)) if s.is_empty() => i64::from(default_port),
        Some(v) => {
            osiris_types::pyval::py_int(v).map_err(|_| format!("Geçersiz {label} proxy portu"))?
        }
    };
    if !(1..=65535).contains(&port) {
        return Err(format!("Geçersiz {label} proxy portu"));
    }
    Ok((host, port as u16))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn python_resolve_proxy_gibi() {
        let r = |h: Value, p: Value, env: Option<&str>| {
            resolve_proxy(Some(&h), Some(&p), env, "OSIRIS_TOR_SOCKS", 9050, "SOCKS5")
        };
        assert_eq!(r(json!(null), json!(null), None), Ok(("127.0.0.1".into(), 9050)));
        assert_eq!(r(json!("::1"), json!("9150"), None), Ok(("::1".into(), 9150)));
        assert!(r(json!("10.0.0.5"), json!(null), None).is_err());
        assert_eq!(r(json!("127.0.0.1"), json!(1234), Some("tor:9050")), Ok(("tor".into(), 9050)));
        assert!(r(json!("evil.example.com"), json!(9050), Some("tor:9050")).is_err());
        assert_eq!(
            r(json!(null), json!(null), Some("[fd00::5]:9050")),
            Ok(("fd00::5".into(), 9050))
        );
    }
}
