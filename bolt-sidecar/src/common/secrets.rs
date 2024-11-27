//! Secret key types wrappers for BLS, ECDSA and JWT.

use std::{
    fmt::{self, Display},
    fs::read_to_string,
    ops::Deref,
    path::Path,
};

use alloy::{hex, signers::k256::ecdsa::SigningKey};
use blst::min_pk::SecretKey;
use rand::{Rng, RngCore};
use serde::{Deserialize, Deserializer};

/// A warpper for BLS secret key.
#[derive(Clone, Debug)]
pub struct BlsSecretKeyWrapper(pub SecretKey);

impl BlsSecretKeyWrapper {
    /// Generate a new random BLS secret key.
    pub fn random() -> Self {
        let mut rng = rand::thread_rng();
        let mut ikm = [0u8; 32];
        rng.fill_bytes(&mut ikm);
        Self(SecretKey::key_gen(&ikm, &[]).unwrap())
    }
}

impl<'de> Deserialize<'de> for BlsSecretKeyWrapper {
    fn deserialize<D>(deserializer: D) -> Result<BlsSecretKeyWrapper, D::Error>
    where
        D: Deserializer<'de>,
    {
        let sk = String::deserialize(deserializer)?;
        Ok(BlsSecretKeyWrapper::from(sk.as_str()))
    }
}

impl From<&str> for BlsSecretKeyWrapper {
    fn from(sk: &str) -> Self {
        let hex_sk = sk.strip_prefix("0x").unwrap_or(sk);
        let sk = SecretKey::from_bytes(&hex::decode(hex_sk).expect("valid hex")).expect("valid sk");
        BlsSecretKeyWrapper(sk)
    }
}

impl Deref for BlsSecretKeyWrapper {
    type Target = SecretKey;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl fmt::Display for BlsSecretKeyWrapper {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", hex::encode_prefixed(self.0.to_bytes()))
    }
}

/// A warpper for ECDSA secret key.
#[derive(Clone, Debug)]
pub struct EcdsaSecretKeyWrapper(pub SigningKey);

impl EcdsaSecretKeyWrapper {
    /// Generate a new random ECDSA secret key.
    pub fn random() -> Self {
        Self(SigningKey::random(&mut rand::thread_rng()))
    }
}

impl<'de> Deserialize<'de> for EcdsaSecretKeyWrapper {
    fn deserialize<D>(deserializer: D) -> Result<EcdsaSecretKeyWrapper, D::Error>
    where
        D: Deserializer<'de>,
    {
        let sk = String::deserialize(deserializer)?;
        Ok(EcdsaSecretKeyWrapper::from(sk.as_str()))
    }
}

impl From<&str> for EcdsaSecretKeyWrapper {
    fn from(sk: &str) -> Self {
        let hex_sk = sk.strip_prefix("0x").unwrap_or(sk);
        let bytes = hex::decode(hex_sk).expect("valid hex");
        let sk = SigningKey::from_slice(&bytes).expect("valid sk");
        EcdsaSecretKeyWrapper(sk)
    }
}

impl Display for EcdsaSecretKeyWrapper {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", hex::encode_prefixed(self.0.to_bytes()))
    }
}

impl Deref for EcdsaSecretKeyWrapper {
    type Target = SigningKey;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// A warpper for JWT secret key.
#[derive(Debug, Clone)]
pub struct JwtSecretConfig(pub String);

impl Default for JwtSecretConfig {
    fn default() -> Self {
        let random_bytes: [u8; 32] = rand::thread_rng().gen();
        let secret = hex::encode(random_bytes);
        Self(secret)
    }
}

impl From<&str> for JwtSecretConfig {
    fn from(jwt: &str) -> Self {
        let jwt = if jwt.starts_with("0x") {
            jwt.trim_start_matches("0x").to_string()
        } else if Path::new(&jwt).exists() {
            read_to_string(jwt)
                .unwrap_or_else(|_| panic!("Failed reading JWT secret file: {:?}", jwt))
                .trim_start_matches("0x")
                .to_string()
        } else {
            jwt.to_string()
        };

        assert!(jwt.len() == 64, "Engine JWT secret must be a 32 byte hex string");

        Self(jwt)
    }
}

impl<'de> Deserialize<'de> for JwtSecretConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let jwt = String::deserialize(deserializer)?;
        Ok(Self::from(jwt.as_str()))
    }
}

impl Deref for JwtSecretConfig {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Display for JwtSecretConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x{}", self.0)
    }
}
