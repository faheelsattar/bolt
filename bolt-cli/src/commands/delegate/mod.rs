use eyre::Result;
use tracing::debug;

use crate::{
    cli::{DelegateCommand, SecretsSource},
    common::{keystore::KeystoreSecret, parse_bls_public_key, write_to_file},
};

/// Types for the delegation/revocation messages.
mod types;

/// Create delegations from local BLS private keys.
mod local;

/// Create delegations from EIP-2335 filesystem keystores.
mod keystore;

/// Create delegations from remote Dirk signers.
mod dirk;

impl DelegateCommand {
    /// Run the `delegate` command.
    pub async fn run(self) -> Result<()> {
        let signed_messages = match self.source {
            SecretsSource::SecretKeys { secret_keys } => {
                let delegatee_pubkey = parse_bls_public_key(&self.delegatee_pubkey)?;
                local::generate_from_local_keys(
                    &secret_keys,
                    delegatee_pubkey,
                    self.chain,
                    self.action,
                )?
            }
            SecretsSource::LocalKeystore { opts } => {
                let keystore_secret = KeystoreSecret::from_keystore_options(&opts)?;
                let delegatee_pubkey = parse_bls_public_key(&self.delegatee_pubkey)?;
                keystore::generate_from_keystore(
                    &opts.path,
                    keystore_secret,
                    delegatee_pubkey,
                    self.chain,
                    self.action,
                )?
            }
            SecretsSource::Dirk { opts } => {
                let delegatee_pubkey = parse_bls_public_key(&self.delegatee_pubkey)?;
                dirk::generate_from_dirk(opts, delegatee_pubkey, self.chain, self.action).await?
            }
        };

        debug!("Generated {} signed messages", signed_messages.len());

        // Verify signatures
        for message in &signed_messages {
            message.verify_signature(self.chain)?;
        }

        write_to_file(&self.out, &signed_messages)?;
        println!("Signed delegation messages generated and saved to {}", self.out);

        Ok(())
    }
}
