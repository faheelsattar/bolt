use alloy::{
    network::EthereumWallet,
    providers::{Provider, ProviderBuilder},
    signers::local::PrivateKeySigner,
};
use ethereum_consensus::crypto::PublicKey as BlsPublicKey;
use eyre::Context;
use tracing::{info, warn};

use crate::{
    cli::{Chain, ValidatorsCommand, ValidatorsSubcommand},
    common::{hash::compress_bls_pubkey, request_confirmation},
    contracts::{bolt::BoltValidators, deployments_for_chain},
};

impl ValidatorsCommand {
    pub async fn run(self) -> eyre::Result<()> {
        match self.subcommand {
            ValidatorsSubcommand::Register {
                max_committed_gas_limit,
                pubkeys_path,
                admin_private_key,
                authorized_operator,
                rpc_url,
            } => {
                let signer = PrivateKeySigner::from_bytes(&admin_private_key)
                    .wrap_err("valid private key")?;

                let provider = ProviderBuilder::new()
                    .with_recommended_fillers()
                    .wallet(EthereumWallet::from(signer))
                    .on_http(rpc_url.clone());

                let chain_id = provider.get_chain_id().await?;
                let chain = Chain::from_id(chain_id)
                    .unwrap_or_else(|| panic!("chain id {} not supported", chain_id));

                let bolt_validators_address = deployments_for_chain(chain).bolt.validators;

                let pubkeys_file = std::fs::File::open(&pubkeys_path)?;
                let keys: Vec<BlsPublicKey> = serde_json::from_reader(pubkeys_file)?;
                let pubkey_hashes: Vec<_> = keys.iter().map(compress_bls_pubkey).collect();

                info!(
                    validators = ?keys.len(),
                    ?max_committed_gas_limit,
                    ?authorized_operator,
                    ?chain,
                    "Registering validators into bolt",
                );

                let bolt_validators =
                    BoltValidators::new(bolt_validators_address, provider.clone());

                request_confirmation();

                let pending = bolt_validators
                    .batchRegisterValidatorsUnsafe(
                        pubkey_hashes,
                        max_committed_gas_limit,
                        authorized_operator,
                    )
                    .send()
                    .await?;

                info!(
                    hash = ?pending.tx_hash(),
                    "batchRegisterValidatorsUnsafe transaction sent, awaiting receipt..."
                );
                let receipt = pending.get_receipt().await?;
                if !receipt.status() {
                    eyre::bail!("Transaction failed: {:?}", receipt)
                }

                info!("Successfully registered validators into bolt");

                Ok(())
            }
            ValidatorsSubcommand::Status { rpc_url, pubkeys_path, pubkeys } => {
                let provider = ProviderBuilder::new().on_http(rpc_url);
                let chain_id = provider.get_chain_id().await?;
                let chain = Chain::from_id(chain_id)
                    .unwrap_or_else(|| panic!("chain id {} not supported", chain_id));
                let registry = deployments_for_chain(chain).bolt.validators;

                let mut bls_pubkeys = Vec::new();

                if let Some(pubkeys_path) = pubkeys_path {
                    let pubkeys_file = std::fs::File::open(&pubkeys_path)?;
                    let keys: Vec<BlsPublicKey> = serde_json::from_reader(pubkeys_file)?;
                    bls_pubkeys.extend(keys);
                }

                for bytes in pubkeys {
                    let key = BlsPublicKey::try_from(bytes.as_ref())?;
                    bls_pubkeys.push(key);
                }

                info!(pubkeys = bls_pubkeys.len(), %registry, ?chain, "Checking status of validators");

                let pubkey_hashes: Vec<_> = bls_pubkeys.iter().map(compress_bls_pubkey).collect();

                let bolt_validators = BoltValidators::new(registry, provider);

                for (hash, pubkey) in pubkey_hashes.iter().zip(bls_pubkeys.iter()) {
                    match bolt_validators.getValidatorByPubkeyHash(*hash).call().await.map(|v| v._0)
                    {
                        Ok(info) => {
                            info!(%pubkey, operator = %info.authorizedOperator, controller = %info.controller, gas_limit = info.maxCommittedGasLimit, "Validator registered");
                        }
                        Err(_e) => {
                            warn!(%pubkey, "Validator not registered");
                        }
                    }
                }

                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use alloy::{
        node_bindings::Anvil,
        primitives::{Address, B256, U256},
        providers::{ext::AnvilApi, ProviderBuilder},
        signers::k256::ecdsa::SigningKey,
    };
    use reqwest::Url;

    use crate::cli::{ValidatorsCommand, ValidatorsSubcommand};

    #[tokio::test]
    async fn test_register_validators() {
        let rpc_url = "https://holesky.drpc.org";
        let anvil = Anvil::default().fork(rpc_url).spawn();
        let anvil_url = Url::parse(&anvil.endpoint()).expect("valid URL");
        let provider = ProviderBuilder::new().on_http(anvil_url.clone());

        let mut rnd = rand::thread_rng();
        let secret_key = SigningKey::random(&mut rnd);
        let account = Address::from_private_key(&secret_key);

        provider.anvil_set_balance(account, U256::from(u64::MAX)).await.expect("set balance");

        let command = ValidatorsCommand {
            subcommand: ValidatorsSubcommand::Register {
                max_committed_gas_limit: 30_000_000,
                admin_private_key: B256::try_from(secret_key.to_bytes().as_slice()).unwrap(),
                authorized_operator: account,
                pubkeys_path: "./test_data/pubkeys.json".parse().unwrap(),
                rpc_url: anvil_url,
            },
        };

        command.run().await.expect("run command");
    }
}
