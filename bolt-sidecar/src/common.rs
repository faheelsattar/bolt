use std::{
    fmt::{self, Display},
    fs::read_to_string,
    future::Future,
    ops::Deref,
    path::Path,
    time::Duration,
};

use alloy::{hex, primitives::U256, signers::k256::ecdsa::SigningKey};
use blst::min_pk::SecretKey;
use rand::{Rng, RngCore};
use reth_primitives::PooledTransactionsElement;
use serde::{Deserialize, Deserializer};
use tokio_retry::{
    strategy::{jitter, ExponentialBackoff},
    Retry,
};

use crate::{
    primitives::{AccountState, TransactionExt},
    state::ValidationError,
};

/// The version of the Bolt sidecar binary.
pub const CARGO_PKG_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Calculates the max_basefee `slot_diff` blocks in the future given a current basefee (in wei).
/// Returns None if an overflow would occur.
/// Cfr. https://github.com/flashbots/ethers-provider-flashbots-bundle/blob/7ddaf2c9d7662bef400151e0bfc89f5b13e72b4c/src/index.ts#L308
///
/// NOTE: this increase is correct also for the EIP-4844 blob base fee:
/// See https://eips.ethereum.org/EIPS/eip-4844#base-fee-per-blob-gas-update-rule
pub fn calculate_max_basefee(current: u128, block_diff: u64) -> Option<u128> {
    // Define the multiplier and divisor for fixed-point arithmetic
    let multiplier: u128 = 1125; // Represents 112.5%
    let divisor: u128 = 1000;
    let mut max_basefee = current;

    for _ in 0..block_diff {
        // Check for potential overflow when multiplying
        if max_basefee > u128::MAX / multiplier {
            return None; // Overflow would occur
        }

        // Perform the multiplication and division (and add 1 to round up)
        max_basefee = max_basefee * multiplier / divisor + 1;
    }

    Some(max_basefee)
}

/// Calculates the max transaction cost (gas + value) in wei.
///
/// - For EIP-1559 transactions: `max_fee_per_gas * gas_limit + tx_value`.
/// - For legacy transactions: `gas_price * gas_limit + tx_value`.
/// - For EIP-4844 blob transactions: `max_fee_per_gas * gas_limit + tx_value + max_blob_fee_per_gas
///   * blob_gas_used`.
pub fn max_transaction_cost(transaction: &PooledTransactionsElement) -> U256 {
    let gas_limit = transaction.gas_limit() as u128;

    let mut fee_cap = transaction.max_fee_per_gas();
    fee_cap += transaction.max_priority_fee_per_gas().unwrap_or(0);

    if let Some(eip4844) = transaction.as_eip4844() {
        fee_cap += eip4844.max_fee_per_blob_gas + eip4844.blob_gas() as u128;
    }

    U256::from(gas_limit * fee_cap) + transaction.value()
}

/// This function validates a transaction against an account state. It checks 2 things:
/// 1. The nonce of the transaction must be higher than the account's nonce, but not higher than
///    current + 1.
/// 2. The balance of the account must be higher than the transaction's max cost.
pub fn validate_transaction(
    account_state: &AccountState,
    transaction: &PooledTransactionsElement,
) -> Result<(), ValidationError> {
    // Check if the nonce is correct (should be the same as the transaction count)
    if transaction.nonce() < account_state.transaction_count {
        return Err(ValidationError::NonceTooLow(
            account_state.transaction_count,
            transaction.nonce(),
        ));
    }

    if transaction.nonce() > account_state.transaction_count {
        return Err(ValidationError::NonceTooHigh(
            account_state.transaction_count,
            transaction.nonce(),
        ));
    }

    // Check if the balance is enough
    if max_transaction_cost(transaction) > account_state.balance {
        return Err(ValidationError::InsufficientBalance);
    }

    // Check if the account has code (i.e. is a smart contract)
    if account_state.has_code {
        return Err(ValidationError::AccountHasCode);
    }

    Ok(())
}

#[derive(Clone, Debug)]
pub struct BlsSecretKeyWrapper(pub SecretKey);

impl BlsSecretKeyWrapper {
    pub fn random() -> Self {
        let mut rng = rand::thread_rng();
        let mut ikm = [0u8; 32];
        rng.fill_bytes(&mut ikm);
        Self(SecretKey::key_gen(&ikm, &[]).unwrap())
    }
}

impl<'de> Deserialize<'de> for BlsSecretKeyWrapper {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let sk = String::deserialize(deserializer)?;
        Ok(Self::from(sk.as_str()))
    }
}

impl From<&str> for BlsSecretKeyWrapper {
    fn from(sk: &str) -> Self {
        let hex_sk = sk.strip_prefix("0x").unwrap_or(sk);
        let sk = SecretKey::from_bytes(&hex::decode(hex_sk).expect("valid hex")).expect("valid sk");
        Self(sk)
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

#[derive(Clone, Debug)]
pub struct EcdsaSecretKeyWrapper(pub SigningKey);

impl EcdsaSecretKeyWrapper {
    /// Generate a new random ECDSA secret key.
    #[allow(dead_code)]
    pub fn random() -> Self {
        Self(SigningKey::random(&mut rand::thread_rng()))
    }
}

impl<'de> Deserialize<'de> for EcdsaSecretKeyWrapper {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let sk = String::deserialize(deserializer)?;
        Ok(Self::from(sk.as_str()))
    }
}

impl From<&str> for EcdsaSecretKeyWrapper {
    fn from(sk: &str) -> Self {
        let hex_sk = sk.strip_prefix("0x").unwrap_or(sk);
        let bytes = hex::decode(hex_sk).expect("valid hex");
        let sk = SigningKey::from_slice(&bytes).expect("valid sk");
        Self(sk)
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

/// Retry a future with exponential backoff and jitter.
pub async fn retry_with_backoff<F, T, E>(max_retries: usize, fut: impl Fn() -> F) -> Result<T, E>
where
    F: Future<Output = Result<T, E>>,
{
    let backoff = ExponentialBackoff::from_millis(100)
        .factor(2)
        .max_delay(Duration::from_secs(1))
        .take(max_retries)
        .map(jitter);

    Retry::spawn(backoff, fut).await
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use thiserror::Error;
    use tokio::{
        sync::Mutex,
        time::{Duration, Instant},
    };

    use super::*;

    #[test]
    fn test_calculate_max_basefee() {
        let current = 10_000_000_000; // 10 gwei
        let slot_diff = 9; // 9 full blocks in the future

        let result = calculate_max_basefee(current, slot_diff);
        assert_eq!(result, Some(28865075793))
    }

    #[derive(Debug, Error)]
    #[error("mock error")]
    struct MockError;

    // Helper struct to count attempts and control failure/success behavior
    struct Counter {
        count: usize,
        fail_until: usize,
    }

    impl Counter {
        fn new(fail_until: usize) -> Self {
            Self { count: 0, fail_until }
        }

        async fn retryable_fn(&mut self) -> Result<(), MockError> {
            self.count += 1;
            if self.count <= self.fail_until {
                Err(MockError)
            } else {
                Ok(())
            }
        }
    }

    #[tokio::test]
    async fn test_retry_success_without_retry() {
        let counter = Arc::new(Mutex::new(Counter::new(0)));

        let result = retry_with_backoff(5, || {
            let counter = Arc::clone(&counter);
            async move {
                let mut counter = counter.lock().await;
                counter.retryable_fn().await
            }
        })
        .await;

        assert!(result.is_ok());
        assert_eq!(counter.lock().await.count, 1, "Should succeed on first attempt");
    }

    #[tokio::test]
    async fn test_retry_until_success() {
        let counter = Arc::new(Mutex::new(Counter::new(3))); // Fail 3 times, succeed on 4th

        let result = retry_with_backoff(5, || async {
            let counter = Arc::clone(&counter);
            let mut counter = counter.lock().await;
            counter.retryable_fn().await
        })
        .await;

        assert!(result.is_ok());
        assert_eq!(counter.lock().await.count, 4, "Should retry until success on 4th attempt");
    }

    #[tokio::test]
    async fn test_max_retries_reached() {
        let counter = Arc::new(Mutex::new(Counter::new(5))); // Fail 5 times, max retries = 3

        let result = retry_with_backoff(3, || {
            let counter = Arc::clone(&counter);
            async move {
                let mut counter = counter.lock().await;
                counter.retryable_fn().await
            }
        })
        .await;

        assert!(result.is_err());
        assert_eq!(counter.lock().await.count, 4, "Should stop after max retries are reached");
    }

    #[tokio::test]
    async fn test_exponential_backoff_timing() {
        let counter = Arc::new(Mutex::new(Counter::new(3))); // Fail 3 times, succeed on 4th
        let start_time = Instant::now();

        let result = retry_with_backoff(5, || {
            let counter = Arc::clone(&counter);
            async move {
                let mut counter = counter.lock().await;
                counter.retryable_fn().await
            }
        })
        .await;

        assert!(result.is_ok());
        let elapsed = start_time.elapsed();
        assert!(
            elapsed >= Duration::from_millis(700),
            "Total backoff duration should be at least 700ms"
        );
    }
}
