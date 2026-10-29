use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use std::{
    env,
    net::{IpAddr, SocketAddr},
    path::PathBuf,
};
use url::Url;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthMode {
    Builtin,
    Proxy,
    Off,
}

#[derive(Clone)]
pub struct Config {
    pub listen: SocketAddr,
    pub origin: String,
    pub auth: AuthMode,
    pub trusted_proxy: IpAddr,
    pub trust_client_ip_header: bool,
    pub proxy_secret: Option<String>,
    pub data_dir: PathBuf,
    pub master_key: [u8; 32],
    pub gemini_url: Url,
    pub openai_url: Url,
    pub gemini_model: String,
    pub openai_model: String,
    pub max_sessions: usize,
    pub cooldown_seconds: u64,
}

impl Config {
    pub fn load() -> Result<Self, String> {
        let listen: SocketAddr = env_value("SPEECHING_LISTEN", "127.0.0.1:8787")
            .parse()
            .map_err(|_| "Invalid SPEECHING_LISTEN address.")?;
        let auth = parse_auth(&env_value("SPEECHING_AUTH", "builtin"))?;
        if auth == AuthMode::Off && !listen.ip().is_loopback() {
            return Err("Authentication-off mode must listen on a loopback address.".into());
        }
        let trusted_proxy = env_value("SPEECHING_TRUSTED_PROXY", "127.0.0.1")
            .parse()
            .map_err(|_| "Invalid SPEECHING_TRUSTED_PROXY address.")?;
        let trust_client_ip_header =
            env_value("SPEECHING_TRUST_CLIENT_IP_HEADER", "false") == "true";
        let proxy_secret = read_proxy_secret()?;
        validate_proxy_configuration(auth, trust_client_ip_header, proxy_secret.as_deref())?;
        let origin = validate_origin(&env_value("SPEECHING_ORIGIN", "http://127.0.0.1:8787"))?;
        let master_key = read_master_key()?;
        let gemini_url = provider_url(&env_value("SPEECHING_GEMINI_URL", "wss://generativelanguage.googleapis.com/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent"))?;
        let openai_url = provider_url(&env_value(
            "SPEECHING_OPENAI_URL",
            "wss://api.openai.com/v1/realtime",
        ))?;
        let max_sessions = env_value("SPEECHING_MAX_SESSIONS", "8")
            .parse::<usize>()
            .map_err(|_| "Invalid SPEECHING_MAX_SESSIONS.")?
            .clamp(1, 64);
        let cooldown_seconds = env_value("SPEECHING_COOLDOWN_SECONDS", "300")
            .parse::<u64>()
            .map_err(|_| "Invalid SPEECHING_COOLDOWN_SECONDS.")?
            .clamp(30, 86_400);
        Ok(Self {
            listen,
            origin,
            auth,
            trusted_proxy,
            trust_client_ip_header,
            proxy_secret,
            data_dir: data_dir(),
            master_key,
            gemini_url,
            openai_url,
            gemini_model: env_value("SPEECHING_GEMINI_MODEL", "gemini-3.8-live"),
            openai_model: env_value("SPEECHING_OPENAI_MODEL", "gpt-realtime-2.1"),
            max_sessions,
            cooldown_seconds,
        })
    }
}

pub fn data_dir() -> PathBuf {
    env::var_os("SPEECHING_WEB_DATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("speeching-web-data"))
}

fn env_value(name: &str, default: &str) -> String {
    env::var(name).unwrap_or_else(|_| default.to_owned())
}

fn parse_auth(value: &str) -> Result<AuthMode, String> {
    match value {
        "builtin" => Ok(AuthMode::Builtin),
        "proxy" => Ok(AuthMode::Proxy),
        "off" => Ok(AuthMode::Off),
        _ => Err("SPEECHING_AUTH must be builtin, proxy, or off.".into()),
    }
}

fn read_master_key() -> Result<[u8; 32], String> {
    let value = if let Ok(path) = env::var("SPEECHING_MASTER_KEY_FILE") {
        std::fs::read_to_string(path).map_err(|_| "Could not read the master key file.")?
    } else {
        env::var("SPEECHING_MASTER_KEY").map_err(|_| {
            "Set SPEECHING_MASTER_KEY_FILE or SPEECHING_MASTER_KEY; see the web README."
        })?
    };
    let bytes = URL_SAFE_NO_PAD
        .decode(value.trim())
        .map_err(|_| "The master key must be unpadded base64url.")?;
    bytes
        .try_into()
        .map_err(|_| "The master key must decode to 32 bytes.".into())
}

fn validate_proxy_configuration(
    auth: AuthMode,
    trust_client_ip: bool,
    secret: Option<&str>,
) -> Result<(), String> {
    if (auth == AuthMode::Proxy || trust_client_ip) && secret.is_none() {
        return Err("Proxy identity or client-IP headers require SPEECHING_PROXY_SECRET_FILE or SPEECHING_PROXY_SECRET.".into());
    }
    Ok(())
}

fn read_proxy_secret() -> Result<Option<String>, String> {
    let value = if let Ok(path) = env::var("SPEECHING_PROXY_SECRET_FILE") {
        Some(std::fs::read_to_string(path).map_err(|_| "Could not read the proxy secret file.")?)
    } else {
        env::var("SPEECHING_PROXY_SECRET").ok()
    };
    let Some(value) = value else {
        return Ok(None);
    };
    validate_proxy_secret(value.trim()).map(Some)
}

fn validate_proxy_secret(secret: &str) -> Result<String, String> {
    if !(32..=256).contains(&secret.len()) || !secret.bytes().all(|byte| (33..=126).contains(&byte))
    {
        return Err(
            "The proxy secret must contain 32–256 printable, non-space ASCII characters.".into(),
        );
    }
    Ok(secret.to_owned())
}

fn validate_origin(value: &str) -> Result<String, String> {
    let url = Url::parse(value).map_err(|_| "Invalid SPEECHING_ORIGIN.")?;
    let loopback = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if url.scheme() != "https" && !(url.scheme() == "http" && loopback) {
        return Err("SPEECHING_ORIGIN must use HTTPS, except on localhost.".into());
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("SPEECHING_ORIGIN must contain only scheme, host, and optional port.".into());
    }
    Ok(url.origin().ascii_serialization())
}

fn provider_url(value: &str) -> Result<Url, String> {
    let url = Url::parse(value).map_err(|_| "Invalid provider URL.")?;
    let loopback = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if url.scheme() != "wss" && !(url.scheme() == "ws" && loopback) {
        return Err("Provider URLs must use wss, except on localhost.".into());
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.query().is_some()
    {
        return Err(
            "Provider URLs cannot contain credentials, query parameters, or fragments.".into(),
        );
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_origin_and_provider_urls() {
        assert!(validate_origin("https://example.com").is_ok());
        assert!(validate_origin("http://127.0.0.1:8787").is_ok());
        for bad in [
            "http://example.com",
            "https://example.com/path",
            "https://user@example.com",
        ] {
            assert!(validate_origin(bad).is_err());
        }
        assert!(provider_url("wss://example.com/live").is_ok());
        assert!(provider_url("ws://127.0.0.1:9000").is_ok());
        for bad in [
            "ws://example.com",
            "wss://user:pass@example.com",
            "wss://example.com/?key=x",
        ] {
            assert!(provider_url(bad).is_err());
        }
    }

    #[test]
    fn randomized_origin_validation_never_accepts_remote_cleartext() {
        for index in 0..1_000 {
            let input = format!("http://host{index}.example:{}/", 1000 + index);
            assert!(validate_origin(&input).is_err());
        }
    }
    #[test]
    fn proxy_secrets_must_be_long_printable_values() {
        assert!(validate_proxy_secret(&crate::crypto::random_token(32)).is_ok());
        assert!(validate_proxy_secret(&"x".repeat(32)).is_ok());
        assert!(validate_proxy_secret(&"x".repeat(256)).is_ok());
        assert!(validate_proxy_secret(&"x".repeat(257)).is_err());
        for length in 0..32 {
            assert!(validate_proxy_secret(&"x".repeat(length)).is_err());
        }
        assert!(validate_proxy_secret(&format!("{} invalid", "x".repeat(32))).is_err());
    }

    #[test]
    fn proxy_auth_and_forwarded_ip_fail_closed_without_secret() {
        assert!(validate_proxy_configuration(AuthMode::Builtin, false, None).is_ok());
        assert!(validate_proxy_configuration(AuthMode::Proxy, false, None).is_err());
        assert!(validate_proxy_configuration(AuthMode::Builtin, true, None).is_err());
        assert!(validate_proxy_configuration(AuthMode::Proxy, true, Some("configured")).is_ok());
    }

    #[test]
    fn seeded_proxy_secrets_respect_length_and_character_rules() {
        use rand::{rngs::StdRng, Rng, SeedableRng};
        let mut rng = StdRng::seed_from_u64(0x52ac_9e71);
        let chars = ['a', 'Z', '7', '-', '~', ' ', '\n', '\u{7f}', 'é'];
        for _ in 0..1000 {
            let size = rng.gen_range(0..300);
            let value: String = (0..size)
                .map(|_| chars[rng.gen_range(0..chars.len())])
                .collect();
            let valid = (32..=256).contains(&value.len())
                && value.bytes().all(|byte| (33..=126).contains(&byte));
            assert_eq!(validate_proxy_secret(&value).is_ok(), valid);
        }
    }
}
