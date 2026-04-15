use ed25519_dalek::{SigningKey, VerifyingKey};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;

use crate::crypto;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredKeypair {
    pub public_key: String,
    pub secret_key: String,
}

/// Wrapper around Ed25519 keypair with convenience accessors.
pub struct NodeKeypair {
    pub signing: SigningKey,
}

/// Derive X25519 private key from an Ed25519 keypair (for key agreement).
pub fn x25519_private_key(keypair: &NodeKeypair) -> [u8; 32] {
    let secret_bytes: [u8; 32] = keypair.signing.to_bytes();
    crypto::ed25519_to_x25519_private(&secret_bytes)
}

/// Directory where identity files live: ~/.bridges/identity/
fn identity_dir() -> PathBuf {
    let base = directories::BaseDirs::new().expect("cannot determine home directory");
    base.home_dir().join(".bridges").join("identity")
}

fn keypair_path() -> PathBuf {
    identity_dir().join("keypair.json")
}

/// Generate a fresh Ed25519 keypair and persist it.
pub fn generate_keypair() -> (SigningKey, VerifyingKey) {
    let signing = SigningKey::generate(&mut OsRng);
    let verifying = signing.verifying_key();
    save_keypair(&signing);
    (signing, verifying)
}

/// Load existing keypair or generate a new one.
pub fn load_or_create_keypair() -> (SigningKey, VerifyingKey) {
    let path = keypair_path();
    if path.exists() {
        let data = fs::read_to_string(&path).expect("failed to read keypair file");
        let stored: StoredKeypair = serde_json::from_str(&data).expect("invalid keypair JSON");
        let secret_bytes = bs58::decode(&stored.secret_key)
            .into_vec()
            .expect("invalid base58 secret key");
        let secret: [u8; 32] = secret_bytes
            .try_into()
            .expect("secret key must be 32 bytes");
        let signing = SigningKey::from_bytes(&secret);
        let verifying = signing.verifying_key();
        (signing, verifying)
    } else {
        generate_keypair()
    }
}

fn save_keypair(signing: &SigningKey) {
    let dir = identity_dir();
    fs::create_dir_all(&dir).expect("failed to create identity dir");
    let stored = StoredKeypair {
        public_key: bs58::encode(signing.verifying_key().as_bytes()).into_string(),
        secret_key: bs58::encode(signing.to_bytes()).into_string(),
    };
    let json = serde_json::to_string_pretty(&stored).unwrap();
    fs::write(keypair_path(), json).expect("failed to write keypair");
}

/// Derive a node ID: `kd_` + base58(sha256(pubkey)[:20])
pub fn derive_node_id(public_key: &VerifyingKey) -> String {
    let mut hasher = Sha256::new();
    hasher.update(public_key.as_bytes());
    let hash = hasher.finalize();
    let truncated = &hash[..20];
    format!("kd_{}", bs58::encode(truncated).into_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_id_prefix() {
        let signing = SigningKey::generate(&mut OsRng);
        let id = derive_node_id(&signing.verifying_key());
        assert!(id.starts_with("kd_"));
    }
}
