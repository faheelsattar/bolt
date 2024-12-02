use alloy::{
    primitives::B256,
    signers::k256::sha2::{Digest, Sha256},
};
use ethereum_consensus::crypto::{
    PublicKey as BlsPublicKey, SecretKey as BlsSecretKey, Signature as BlsSignature,
};
use eyre::{bail, Result};
use lighthouse_eth2_keystore::Keystore;
use serde::Serialize;
use tracing::{debug, warn};

use crate::{
    cli::{Action, Chain, DelegateCommand, SecretsSource},
    common::{
        dirk::Dirk,
        keystore::{keystore_paths, KeystoreError, KeystoreSecret},
        parse_bls_public_key,
        signing::{
            compute_commit_boost_signing_root, compute_domain_from_mask, verify_commit_boost_root,
        },
        write_to_file,
    },
};

impl DelegateCommand {
    /// Run the `delegate` command.
    pub async fn run(self) -> Result<()> {
        match self.source {
            SecretsSource::SecretKeys { secret_keys } => {
                let delegatee_pubkey = parse_bls_public_key(&self.delegatee_pubkey)?;
                let signed_messages = generate_from_local_keys(
                    &secret_keys,
                    delegatee_pubkey,
                    self.chain,
                    self.action,
                )?;
                debug!("Signed {} messages with local keys", signed_messages.len());

                // Verify signatures
                for message in &signed_messages {
                    verify_message_signature(message, self.chain)?;
                }

                write_to_file(&self.out, &signed_messages)?;
                println!("Signed delegation messages generated and saved to {}", self.out);
            }
            SecretsSource::LocalKeystore { opts } => {
                let keystore_secret = KeystoreSecret::from_keystore_options(&opts)?;
                let delegatee_pubkey = parse_bls_public_key(&self.delegatee_pubkey)?;
                let signed_messages = generate_from_keystore(
                    &opts.path,
                    keystore_secret,
                    delegatee_pubkey,
                    self.chain,
                    self.action,
                )?;
                debug!("Signed {} messages with keystore", signed_messages.len());

                // Verify signatures
                for message in &signed_messages {
                    verify_message_signature(message, self.chain)?;
                }

                write_to_file(&self.out, &signed_messages)?;
                println!("Signed delegation messages generated and saved to {}", self.out);
            }
            SecretsSource::Dirk { opts } => {
                let mut dirk = Dirk::connect(opts.url, opts.tls_credentials).await?;

                let delegatee_pubkey = parse_bls_public_key(&self.delegatee_pubkey)?;
                let signed_messages = generate_from_dirk(
                    &mut dirk,
                    delegatee_pubkey,
                    opts.wallet_path,
                    opts.passphrases,
                    self.chain,
                    self.action,
                )
                .await?;
                debug!("Signed {} messages with Dirk", signed_messages.len());

                // Verify signatures
                for message in &signed_messages {
                    verify_message_signature(message, self.chain)?;
                }

                write_to_file(&self.out, &signed_messages)?;
                println!("Signed delegation messages generated and saved to {}", self.out);
            }
        }

        Ok(())
    }
}

/// Generate signed delegations/revocations using local BLS private keys
///
/// - Use the provided private keys from either CLI or env variable
/// - Create message
/// - Compute the signing roots and sign the messages
/// - Return the signed messages
pub fn generate_from_local_keys(
    secret_keys: &[String],
    delegatee_pubkey: BlsPublicKey,
    chain: Chain,
    action: Action,
) -> Result<Vec<SignedMessage>> {
    let mut signed_messages = Vec::with_capacity(secret_keys.len());

    for sk in secret_keys {
        let sk = BlsSecretKey::try_from(sk.trim().to_string())?;

        match action {
            Action::Delegate => {
                let message = DelegationMessage::new(sk.public_key(), delegatee_pubkey.clone());
                let signing_root = compute_commit_boost_signing_root(message.digest(), &chain)?;
                let signature = sk.sign(signing_root.0.as_ref());
                let signed = SignedDelegation { message, signature };
                signed_messages.push(SignedMessage::Delegation(signed))
            }
            Action::Revoke => {
                let message = RevocationMessage::new(sk.public_key(), delegatee_pubkey.clone());
                let signing_root = compute_commit_boost_signing_root(message.digest(), &chain)?;
                let signature = sk.sign(signing_root.0.as_ref());
                let signed = SignedRevocation { message, signature };
                signed_messages.push(SignedMessage::Revocation(signed));
            }
        }
    }

    Ok(signed_messages)
}

/// Generate signed delegations/revocations using a keystore file
///
/// - Read the keystore file
/// - Decrypt the keypairs using the password
/// - Create messages
/// - Compute the signing roots and sign the message
/// - Return the signed message
pub fn generate_from_keystore(
    keys_path: &str,
    keystore_secret: KeystoreSecret,
    delegatee_pubkey: BlsPublicKey,
    chain: Chain,
    action: Action,
) -> Result<Vec<SignedMessage>> {
    let keystores_paths = keystore_paths(keys_path)?;
    let mut signed_messages = Vec::with_capacity(keystores_paths.len());
    debug!("Found {} keys in the keystore", keystores_paths.len());

    for path in keystores_paths {
        let ks = Keystore::from_json_file(path).map_err(KeystoreError::Eth2Keystore)?;
        let password = keystore_secret.get(ks.pubkey()).ok_or(KeystoreError::MissingPassword)?;
        let kp = ks.decrypt_keypair(password.as_bytes()).map_err(KeystoreError::Eth2Keystore)?;
        let validator_pubkey = BlsPublicKey::try_from(kp.pk.serialize().to_vec().as_ref())?;
        let validator_private_key = kp.sk;

        match action {
            Action::Delegate => {
                let message = DelegationMessage::new(validator_pubkey, delegatee_pubkey.clone());
                let signing_root = compute_commit_boost_signing_root(message.digest(), &chain)?;
                let signature = validator_private_key.sign(signing_root.0.into());
                let signature = BlsSignature::try_from(signature.serialize().as_ref())?;
                let signed = SignedDelegation { message, signature };
                signed_messages.push(SignedMessage::Delegation(signed));
            }
            Action::Revoke => {
                let message = RevocationMessage::new(validator_pubkey, delegatee_pubkey.clone());
                let signing_root = compute_commit_boost_signing_root(message.digest(), &chain)?;
                let signature = validator_private_key.sign(signing_root.0.into());
                let signature = BlsSignature::try_from(signature.serialize().as_ref())?;
                let signed = SignedRevocation { message, signature };
                signed_messages.push(SignedMessage::Revocation(signed));
            }
        }
    }

    Ok(signed_messages)
}

/// Generate signed delegations/revocations using a remote Dirk signer
pub async fn generate_from_dirk(
    dirk: &mut Dirk,
    delegatee_pubkey: BlsPublicKey,
    account_path: String,
    passphrases: Option<Vec<String>>,
    chain: Chain,
    action: Action,
) -> Result<Vec<SignedMessage>> {
    // first read the accounts from the remote keystore
    let accounts = dirk.list_accounts(account_path).await?;
    debug!(
        normal = %accounts.accounts.len(),
        distributed = %accounts.distributed_accounts.len(),
        "Found remote accounts to sign with",
    );

    let total_accounts = accounts.accounts.len() + accounts.distributed_accounts.len();
    let mut signed_messages = Vec::with_capacity(total_accounts);

    // specify the signing domain (needs to be included in the signing request)
    let domain = B256::from(compute_domain_from_mask(chain.fork_version()));

    // Collect all account names and pubkeys (regular and distributed accounts)
    let all_accounts_info = accounts
        .accounts
        .into_iter()
        .map(|acc| (acc.name, acc.public_key))
        .chain(
            accounts
                .distributed_accounts
                .into_iter()
                .map(|acc| (acc.name, acc.composite_public_key)),
        )
        .collect::<Vec<_>>();

    for (name, pubkey_bytes) in all_accounts_info {
        // for each available pubkey we control, sign a delegation message
        let pubkey = BlsPublicKey::try_from(pubkey_bytes.as_slice())?;

        // Note: before signing, we must unlock the account
        if let Some(passphrases) = &passphrases {
            try_unlock_account(dirk, name.clone(), passphrases).await?;
        } else {
            bail!("A passphrase is required in order to sign messages remotely with Dirk");
        }

        match action {
            Action::Delegate => {
                let message = DelegationMessage::new(pubkey.clone(), delegatee_pubkey.clone());
                let signing_root = message.digest().into(); // Dirk does the hash tree root internally
                let signature = dirk.request_signature(name.clone(), signing_root, domain).await?;
                let signed = SignedDelegation { message, signature };
                signed_messages.push(SignedMessage::Delegation(signed));
            }
            Action::Revoke => {
                let message = RevocationMessage::new(pubkey.clone(), delegatee_pubkey.clone());
                let signing_root = message.digest().into(); // Dirk does the hash tree root internally
                let signature = dirk.request_signature(name.clone(), signing_root, domain).await?;
                let signed = SignedRevocation { message, signature };
                signed_messages.push(SignedMessage::Revocation(signed));
            }
        }

        // Try to lock the account back after signing
        if let Err(err) = dirk.lock_account(name.clone()).await {
            warn!("Failed to lock account after signing {}: {:?}", name, err);
        }
    }

    Ok(signed_messages)
}

/// Event types that can be emitted by the validator pubkey to
/// signal some action on the Bolt protocol.
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
enum SignedMessageAction {
    /// Signal delegation of a validator pubkey to a delegatee pubkey.
    Delegation,
    /// Signal revocation of a previously delegated pubkey.
    Revocation,
}

/// Transparent serialization of signed messages.
/// This is used to serialize and deserialize signed messages
///
/// e.g. serde_json::to_string(&signed_message):
/// ```
/// {
///    "message": {
///       "action": 0,
///       "validator_pubkey": "0x...",
///       "delegatee_pubkey": "0x..."
///    },
///   "signature": "0x..."
/// },
/// ```
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum SignedMessage {
    Delegation(SignedDelegation),
    Revocation(SignedRevocation),
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SignedDelegation {
    pub message: DelegationMessage,
    pub signature: BlsSignature,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DelegationMessage {
    action: u8,
    pub validator_pubkey: BlsPublicKey,
    pub delegatee_pubkey: BlsPublicKey,
}

impl DelegationMessage {
    /// Create a new delegation message.
    pub fn new(validator_pubkey: BlsPublicKey, delegatee_pubkey: BlsPublicKey) -> Self {
        Self { action: SignedMessageAction::Delegation as u8, validator_pubkey, delegatee_pubkey }
    }

    /// Compute the digest of the delegation message.
    pub fn digest(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update([self.action]);
        hasher.update(self.validator_pubkey.to_vec());
        hasher.update(self.delegatee_pubkey.to_vec());

        hasher.finalize().into()
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SignedRevocation {
    pub message: RevocationMessage,
    pub signature: BlsSignature,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RevocationMessage {
    action: u8,
    pub validator_pubkey: BlsPublicKey,
    pub delegatee_pubkey: BlsPublicKey,
}

impl RevocationMessage {
    /// Create a new revocation message.
    pub fn new(validator_pubkey: BlsPublicKey, delegatee_pubkey: BlsPublicKey) -> Self {
        Self { action: SignedMessageAction::Revocation as u8, validator_pubkey, delegatee_pubkey }
    }

    /// Compute the digest of the revocation message.
    pub fn digest(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update([self.action]);
        hasher.update(self.validator_pubkey.to_vec());
        hasher.update(self.delegatee_pubkey.to_vec());

        hasher.finalize().into()
    }
}

/// Verify the signature of a signed message
pub fn verify_message_signature(message: &SignedMessage, chain: Chain) -> Result<()> {
    match message {
        SignedMessage::Delegation(signed_delegation) => {
            let signer_pubkey = signed_delegation.message.validator_pubkey.clone();
            let digest = signed_delegation.message.digest();

            let blst_sig =
                blst::min_pk::Signature::from_bytes(signed_delegation.signature.as_ref())
                    .map_err(|e| eyre::eyre!("Failed to parse signature: {:?}", e))?;

            // Verify the signature
            verify_commit_boost_root(signer_pubkey, digest, &blst_sig, &chain)
        }
        SignedMessage::Revocation(signed_revocation) => {
            let signer_pubkey = signed_revocation.message.validator_pubkey.clone();
            let digest = signed_revocation.message.digest();

            let blst_sig =
                blst::min_pk::Signature::from_bytes(signed_revocation.signature.as_ref())
                    .map_err(|e| eyre::eyre!("Failed to parse signature: {:?}", e))?;

            // Verify the signature
            verify_commit_boost_root(signer_pubkey, digest, &blst_sig, &chain)
        }
    }
}

/// Try to unlock an account using the provided passphrases
/// If the account is unlocked, return Ok(()), otherwise return an error
pub async fn try_unlock_account(
    dirk: &mut Dirk,
    account_name: String,
    passphrases: &[String],
) -> Result<()> {
    let mut unlocked = false;
    for passphrase in passphrases {
        if dirk.unlock_account(account_name.clone(), passphrase.clone()).await? {
            unlocked = true;
            break;
        }
    }

    if !unlocked {
        bail!("Failed to unlock account {}", account_name);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{
        cli::{Action, Chain},
        common::{dirk, keystore, parse_bls_public_key},
    };

    use super::{generate_from_dirk, generate_from_keystore, verify_message_signature};

    #[test]
    fn test_delegation_keystore_signer_lighthouse() -> eyre::Result<()> {
        // Read the keystore from test_data
        let keys_path = env!("CARGO_MANIFEST_DIR").to_string() + "/test_data/lighthouse/validators";
        let secrets_path = env!("CARGO_MANIFEST_DIR").to_string() + "/test_data/lighthouse/secrets";

        let keystore_secret = keystore::KeystoreSecret::from_directory(&secrets_path)?;

        let delegatee_pubkey = "0x83eeddfac5e60f8fe607ee8713efb8877c295ad9f8ca075f4d8f6f2ae241a30dd57f78f6f3863a9fe0d5b5db9d550b93";
        let delegatee_pubkey = parse_bls_public_key(delegatee_pubkey)?;
        let chain = Chain::Mainnet;

        let signed_delegations = generate_from_keystore(
            &keys_path,
            keystore_secret,
            delegatee_pubkey,
            chain,
            Action::Delegate,
        )?;

        let signed_message = signed_delegations.first().expect("to get signed delegation");

        verify_message_signature(signed_message, chain)?;

        Ok(())
    }

    /// Test generating signed delegations using a remote Dirk signer (single instance).
    ///
    /// ```shell
    /// cargo test --package bolt --bin bolt -- commands::delegate::tests::test_delegation_dirk_single
    /// --exact --show-output --ignored --nocapture
    /// ```
    #[tokio::test]
    #[ignore = "Requires Dirk to be installed on the system"]
    async fn test_delegation_dirk_single() -> eyre::Result<()> {
        let _ = tracing_subscriber::fmt::try_init();
        let (mut dirk, mut dirk_proc) = dirk::test_util::start_single_dirk_test_server().await?;

        let delegatee_pubkey = "0x83eeddfac5e60f8fe607ee8713efb8877c295ad9f8ca075f4d8f6f2ae241a30dd57f78f6f3863a9fe0d5b5db9d550b93";
        let delegatee_pubkey = parse_bls_public_key(delegatee_pubkey)?;
        let chain = Chain::Mainnet;

        let signed_delegations = generate_from_dirk(
            &mut dirk,
            delegatee_pubkey.clone(),
            "wallet1".to_string(),
            Some(vec!["secret".to_string()]),
            chain,
            Action::Delegate,
        )
        .await?;

        let signed_message = signed_delegations.first().expect("to get signed delegation");

        verify_message_signature(signed_message, chain)?;

        dirk_proc.kill()?;

        Ok(())
    }

    /// Test generating signed delegations using a remote Dirk signer (multi-node instance).
    /// This test requires multiple instances of Dirk to be running.
    ///
    /// ```shell
    /// cargo test --package bolt --bin bolt -- commands::delegate::tests::test_delegation_dirk_multi
    /// --exact --show-output --ignored --nocapture
    /// ```
    #[tokio::test]
    #[ignore = "Requires Dirk to be installed on the system"]
    async fn test_delegation_dirk_multi() -> eyre::Result<()> {
        Ok(())
    }
}
