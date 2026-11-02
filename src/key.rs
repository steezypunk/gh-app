use std::path::Path;

use base64::Engine;
use ring::rand::SystemRandom;
use ring::signature::{RSA_PKCS1_SHA256, RsaKeyPair};
use rsa::RsaPrivateKey;
use rsa::pkcs1::{DecodeRsaPrivateKey, EncodeRsaPublicKey};
use rsa::pkcs8::{DecodePrivateKey, EncodePrivateKey};
use rsa::traits::PublicKeyParts;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::config::permissions::read_private_key;
use crate::error::{Error, Result};

pub struct Signer {
    key_pair: RsaKeyPair,
    fingerprint: String,
    bits: usize,
}

impl Signer {
    pub fn from_pem(pem: &[u8]) -> Result<Self> {
        let text = std::str::from_utf8(pem)
            .map_err(|_| Error::message("private key is not valid UTF-8 PEM"))?;
        let key = RsaPrivateKey::from_pkcs1_pem(text)
            .or_else(|_| RsaPrivateKey::from_pkcs8_pem(text))
            .map_err(|e| Error::message(format!("could not parse private key: {e}")))?;

        let bits = key.size() * 8;
        if !(2048..=4096).contains(&bits) {
            return Err(Error::message(format!(
                "unsupported RSA key size: {bits} bits (expected 2048 to 4096)"
            )));
        }

        let pkcs8 = Zeroizing::new(
            key.to_pkcs8_der()
                .map_err(|e| Error::message(format!("could not encode private key: {e}")))?
                .as_bytes()
                .to_vec(),
        );
        let key_pair = RsaKeyPair::from_pkcs8(&pkcs8)
            .map_err(|e| Error::message(format!("could not load RSA key: {e}")))?;
        let fingerprint = fingerprint(&key)?;

        Ok(Self {
            key_pair,
            fingerprint,
            bits,
        })
    }

    pub fn from_file(path: &Path) -> Result<Self> {
        let pem = Zeroizing::new(read_private_key(path)?);
        Self::from_pem(&pem)
    }

    pub fn from_env(name: &str) -> Result<Self> {
        let value = std::env::var(name)
            .map_err(|_| Error::message(format!("environment variable {name} is not set")))?;
        let pem = Zeroizing::new(value.into_bytes());
        Self::from_pem(&pem)
    }

    pub fn sign(&self, message: &[u8]) -> Result<Vec<u8>> {
        let mut signature = vec![0u8; self.key_pair.public().modulus_len()];
        self.key_pair
            .sign(
                &RSA_PKCS1_SHA256,
                &SystemRandom::new(),
                message,
                &mut signature,
            )
            .map_err(|e| Error::message(format!("RSA signing failed: {e}")))?;
        Ok(signature)
    }

    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    pub fn bits(&self) -> usize {
        self.bits
    }
}

fn fingerprint(key: &RsaPrivateKey) -> Result<String> {
    let public = key.to_public_key();
    let der = public
        .to_pkcs1_der()
        .map_err(|e| Error::message(format!("could not encode public key: {e}")))?;
    let digest = Sha256::digest(der.as_bytes());
    let encoded = base64::engine::general_purpose::STANDARD.encode(digest);
    Ok(format!("SHA256:{encoded}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsa::pkcs1::EncodeRsaPrivateKey;
    use rsa::pkcs8::EncodePrivateKey;

    fn test_key() -> RsaPrivateKey {
        let mut rng = rand::thread_rng();
        RsaPrivateKey::new(&mut rng, 2048).unwrap()
    }

    #[test]
    fn accepts_pkcs1_pem() {
        let key = test_key();
        let pem = key.to_pkcs1_pem(rsa::pkcs1::LineEnding::LF).unwrap();
        let signer = Signer::from_pem(pem.as_bytes()).unwrap();
        assert_eq!(signer.bits(), 2048);
        assert!(signer.fingerprint().starts_with("SHA256:"));
    }

    #[test]
    fn accepts_pkcs8_pem() {
        let key = test_key();
        let pem = key.to_pkcs8_pem(rsa::pkcs8::LineEnding::LF).unwrap();
        let signer = Signer::from_pem(pem.as_bytes()).unwrap();
        assert_eq!(signer.bits(), 2048);
    }

    #[test]
    fn rejects_small_key() {
        let mut rng = rand::thread_rng();
        let key = RsaPrivateKey::new(&mut rng, 1024).unwrap();
        let pem = key.to_pkcs1_pem(rsa::pkcs1::LineEnding::LF).unwrap();
        assert!(Signer::from_pem(pem.as_bytes()).is_err());
    }

    #[test]
    fn signs_message() {
        let key = test_key();
        let pem = key.to_pkcs1_pem(rsa::pkcs1::LineEnding::LF).unwrap();
        let signer = Signer::from_pem(pem.as_bytes()).unwrap();
        let signature = signer.sign(b"hello").unwrap();
        assert_eq!(signature.len(), 256);
    }
}
