use alloy::signers::k256::sha2::{Digest, Sha256};
use ethereum_consensus::crypto::{PublicKey as BlsPublicKey, Signature as BlsSignature};
use eyre::Result;
use serde::Serialize;

use crate::{cli::Chain, common::signing::verify_commit_boost_root};

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

impl SignedMessage {
    /// Verify the signature of a signed message
    pub fn verify_signature(&self, chain: Chain) -> Result<()> {
        match self {
            Self::Delegation(signed_delegation) => {
                let signer_pubkey = signed_delegation.message.validator_pubkey.clone();
                let digest = signed_delegation.message.digest();

                let blst_sig =
                    blst::min_pk::Signature::from_bytes(signed_delegation.signature.as_ref())
                        .map_err(|e| eyre::eyre!("Failed to parse signature: {:?}", e))?;

                // Verify the signature
                verify_commit_boost_root(signer_pubkey, digest, &blst_sig, &chain)
            }
            Self::Revocation(signed_revocation) => {
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
