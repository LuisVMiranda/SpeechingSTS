use aes_gcm::{
    aead::{Aead, KeyInit, Payload},
    Aes256Gcm, Nonce,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::{rngs::OsRng, RngCore};
use sha2::{Digest, Sha256};

pub fn random_token(bytes: usize) -> String {
    let mut value = vec![0; bytes];
    OsRng.fill_bytes(&mut value);
    URL_SAFE_NO_PAD.encode(value)
}

pub fn owner_id(namespace: &str, identity: &str) -> String {
    let digest = Sha256::digest(format!("{namespace}\0{identity}").as_bytes());
    URL_SAFE_NO_PAD.encode(digest)
}

pub fn seal(key: &[u8; 32], owner: &str, provider: &str, secret: &str) -> Result<String, String> {
    if secret.trim().is_empty() || secret.len() > 2_560 {
        return Err("Enter a valid API key of at most 2,560 bytes.".into());
    }
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| "Invalid master key.")?;
    let mut nonce = [0; 12];
    OsRng.fill_bytes(&mut nonce);
    let associated = format!("{owner}\0{provider}");
    let encrypted = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: secret.trim().as_bytes(),
                aad: associated.as_bytes(),
            },
        )
        .map_err(|_| "Could not encrypt API key.")?;
    let mut encoded = nonce.to_vec();
    encoded.extend(encrypted);
    Ok(URL_SAFE_NO_PAD.encode(encoded))
}

pub fn open(key: &[u8; 32], owner: &str, provider: &str, encoded: &str) -> Result<String, String> {
    let data = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| "Saved API key is invalid.")?;
    if data.len() < 28 {
        return Err("Saved API key is invalid.".into());
    }
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| "Invalid master key.")?;
    let associated = format!("{owner}\0{provider}");
    let plain = cipher
        .decrypt(
            Nonce::from_slice(&data[..12]),
            Payload {
                msg: &data[12..],
                aad: associated.as_bytes(),
            },
        )
        .map_err(|_| "Could not decrypt the saved API key.")?;
    String::from_utf8(plain).map_err(|_| "Saved API key is invalid.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn secrets_are_encrypted_and_bound_to_owner_and_provider() {
        let key = [7; 32];
        let first = seal(&key, "alice", "gemini", "example-secret").unwrap();
        let second = seal(&key, "alice", "gemini", "example-secret").unwrap();
        assert_ne!(first, second);
        assert!(!first.contains("example-secret"));
        assert_eq!(
            open(&key, "alice", "gemini", &first).unwrap(),
            "example-secret"
        );
        assert!(open(&key, "bob", "gemini", &first).is_err());
        assert!(open(&key, "alice", "openai", &first).is_err());
        assert!(open(&[8; 32], "alice", "gemini", &first).is_err());
    }
    #[test]
    fn randomized_identities_and_ciphertexts_remain_isolated() {
        for index in 0..1_000 {
            let owner = owner_id("builtin", &format!("user-{index}"));
            let other = owner_id("proxy", &format!("user-{index}"));
            assert_ne!(owner, other);
            let sealed = seal(&[3; 32], &owner, "gemini", "test-key").unwrap();
            assert!(open(&[3; 32], &other, "gemini", &sealed).is_err());
        }
    }
}
