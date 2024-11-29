use alloy::{
    network::EthereumWallet,
    node_bindings::WEI_IN_ETHER,
    primitives::{utils::format_ether, Bytes},
    providers::{Provider, ProviderBuilder, WalletProvider},
    signers::{local::PrivateKeySigner, SignerSync},
};
use eyre::Context;
use tracing::{info, warn};

use crate::{
    cli::{
        Chain, EigenLayerSubcommand, OperatorsCommand, OperatorsSubcommand, SymbioticSubcommand,
    },
    common::{bolt_manager::BoltManagerContract, request_confirmation},
    contracts::{
        bolt::{
            BoltEigenLayerMiddleware,
            BoltSymbioticMiddleware::{self},
            SignatureWithSaltAndExpiry,
        },
        deployments_for_chain,
        eigenlayer::{
            AVSDirectory, IStrategy::IStrategyInstance, IStrategyManager::IStrategyManagerInstance,
        },
        erc20::IERC20::IERC20Instance,
        strategy_to_address,
        symbiotic::IOptInService,
    },
};

impl OperatorsCommand {
    pub async fn run(self) -> eyre::Result<()> {
        match self.subcommand {
            OperatorsSubcommand::EigenLayer { subcommand } => match subcommand {
                EigenLayerSubcommand::Deposit {
                    rpc_url,
                    strategy,
                    amount,
                    operator_private_key,
                } => {
                    let signer = PrivateKeySigner::from_bytes(&operator_private_key)
                        .wrap_err("valid private key")?;
                    let operator = signer.address();

                    let provider = ProviderBuilder::new()
                        .with_recommended_fillers()
                        .wallet(EthereumWallet::from(signer))
                        .on_http(rpc_url.clone());

                    let chain_id = provider.get_chain_id().await?;
                    let chain = Chain::from_id(chain_id)
                        .unwrap_or_else(|| panic!("chain id {} not supported", chain_id));

                    let deployments = deployments_for_chain(chain);

                    let strategy_address =
                        strategy_to_address(strategy, deployments.eigen_layer.supported_strategies);
                    let strategy_contract =
                        IStrategyInstance::new(strategy_address, provider.clone());
                    let strategy_manager_address = deployments.eigen_layer.strategy_manager;
                    let strategy_manager =
                        IStrategyManagerInstance::new(strategy_manager_address, provider.clone());

                    let token = strategy_contract.underlyingToken().call().await?.token;

                    let amount = amount * WEI_IN_ETHER;

                    info!(%strategy, %token, amount = format_ether(amount), ?operator, "Depositing funds into EigenLayer strategy");

                    request_confirmation();

                    let token_erc20 = IERC20Instance::new(token, provider.clone());

                    let balance = token_erc20
                        .balanceOf(provider.clone().default_signer_address())
                        .call()
                        .await?
                        ._0;

                    info!("Operator token balance: {}", format_ether(balance));

                    let result =
                        token_erc20.approve(strategy_manager_address, amount).send().await?;

                    info!(hash = ?result.tx_hash(), "Approving transfer of {} {:?}, awaiting receipt...", amount, strategy);
                    let result = result.watch().await?;
                    info!("Approval transaction included. Transaction hash: {:?}", result);

                    let result = strategy_manager
                        .depositIntoStrategy(strategy_address, token, amount)
                        .send()
                        .await?;

                    info!(hash = ?result.tx_hash(), "Submitted deposit transaction, awaiting receipt...");
                    let receipt = result.get_receipt().await?;

                    if !receipt.status() {
                        eyre::bail!("Transaction failed: {:?}", receipt)
                    }

                    info!("Succesfully deposited collateral into strategy");

                    Ok(())
                }
                EigenLayerSubcommand::Register {
                    rpc_url,
                    operator_rpc,
                    salt,
                    expiry,
                    operator_private_key,
                } => {
                    let signer = PrivateKeySigner::from_bytes(&operator_private_key)
                        .wrap_err("valid private key")?;

                    let provider = ProviderBuilder::new()
                        .with_recommended_fillers()
                        .wallet(EthereumWallet::from(signer.clone()))
                        .on_http(rpc_url.clone());

                    let chain_id = provider.get_chain_id().await?;
                    let chain = Chain::from_id(chain_id)
                        .unwrap_or_else(|| panic!("chain id {} not supported", chain_id));

                    info!(operator = %signer.address(), rpc = %operator_rpc, ?chain, "Registering EigenLayer operator");

                    request_confirmation();

                    let deployments = deployments_for_chain(chain);

                    let bolt_avs_address = deployments.bolt.eigenlayer_middleware;
                    let bolt_eigenlayer_middleware =
                        BoltEigenLayerMiddleware::new(bolt_avs_address, provider.clone());

                    let avs_directory =
                        AVSDirectory::new(deployments.eigen_layer.avs_directory, provider.clone());
                    let signature_digest_hash = avs_directory
                        .calculateOperatorAVSRegistrationDigestHash(
                            provider.clone().default_signer_address(),
                            bolt_avs_address,
                            salt,
                            expiry,
                        )
                        .call()
                        .await?
                        ._0;

                    let signature =
                        Bytes::from(signer.sign_hash_sync(&signature_digest_hash)?.as_bytes());
                    let signature = SignatureWithSaltAndExpiry { signature, expiry, salt };

                    let result = bolt_eigenlayer_middleware
                        .registerOperator(operator_rpc.to_string(), signature)
                        .send()
                        .await?;

                    info!(
                        hash = ?result.tx_hash(),
                        "registerOperator transaction sent, awaiting receipt..."
                    );

                    let receipt = result.get_receipt().await?;
                    if !receipt.status() {
                        eyre::bail!("Transaction failed: {:?}", receipt)
                    }

                    info!("Succesfully registered EigenLayer operator");

                    Ok(())
                }
                EigenLayerSubcommand::Deregister { rpc_url, operator_private_key } => {
                    let signer = PrivateKeySigner::from_bytes(&operator_private_key)
                        .wrap_err("valid private key")?;

                    let provider = ProviderBuilder::new()
                        .with_recommended_fillers()
                        .wallet(EthereumWallet::from(signer.clone()))
                        .on_http(rpc_url.clone());

                    let chain_id = provider.get_chain_id().await?;
                    let chain = Chain::from_id(chain_id)
                        .unwrap_or_else(|| panic!("chain id {} not supported", chain_id));

                    info!(operator = %signer.address(), ?chain, "Deregistering EigenLayer operator");

                    request_confirmation();

                    let deployments = deployments_for_chain(chain);

                    let bolt_avs_address = deployments.bolt.eigenlayer_middleware;
                    let bolt_eigenlayer_middleware =
                        BoltEigenLayerMiddleware::new(bolt_avs_address, provider.clone());

                    let result = bolt_eigenlayer_middleware.deregisterOperator().send().await?;

                    info!(
                        hash = ?result.tx_hash(),
                        "deregisterOperator transaction sent, awaiting receipt..."
                    );

                    let receipt = result.get_receipt().await?;
                    if !receipt.status() {
                        eyre::bail!("Transaction failed: {:?}", receipt)
                    }

                    info!("Succesfully deregistered EigenLayer operator");

                    Ok(())
                }
                EigenLayerSubcommand::Status { rpc_url: rpc, address } => {
                    let provider = ProviderBuilder::new().on_http(rpc.clone());
                    let chain_id = provider.get_chain_id().await?;
                    let chain = Chain::from_id(chain_id)
                        .unwrap_or_else(|| panic!("chain id {} not supported", chain_id));

                    let deployments = deployments_for_chain(chain);
                    let bolt_manager =
                        BoltManagerContract::new(deployments.bolt.manager, provider.clone());
                    if bolt_manager.isOperator(address).call().await?._0 {
                        info!(?address, "EigenLayer operator is registered");
                    } else {
                        warn!(?address, "Operator not registered");
                    }

                    Ok(())
                }
            },
            OperatorsSubcommand::Symbiotic { subcommand } => match subcommand {
                SymbioticSubcommand::Register { operator_rpc, operator_private_key, rpc_url } => {
                    let signer = PrivateKeySigner::from_bytes(&operator_private_key)
                        .wrap_err("valid private key")?;

                    let provider = ProviderBuilder::new()
                        .with_recommended_fillers()
                        .wallet(EthereumWallet::from(signer.clone()))
                        .on_http(rpc_url);

                    let chain_id = provider.get_chain_id().await?;
                    let chain = Chain::from_id(chain_id)
                        .unwrap_or_else(|| panic!("chain id {} not supported", chain_id));

                    let deployments = deployments_for_chain(chain);

                    info!(operator = %signer.address(), rpc = %operator_rpc, ?chain, "Registering Symbiotic operator");

                    request_confirmation();

                    // Check if operator is opted in to the bolt network
                    if !IOptInService::new(
                        deployments.symbiotic.network_opt_in_service,
                        provider.clone(),
                    )
                    .isOptedIn(signer.address(), deployments.symbiotic.network)
                    .call()
                    .await?
                    ._0
                    {
                        eyre::bail!(
                            "Operator with address {} not opted in to the bolt network ({})",
                            signer.address(),
                            deployments.symbiotic.network
                        );
                    }

                    let middleware = BoltSymbioticMiddleware::new(
                        deployments.bolt.symbiotic_middleware,
                        provider.clone(),
                    );

                    let pending =
                        middleware.registerOperator(operator_rpc.to_string()).send().await?;

                    info!(
                        hash = ?pending.tx_hash(),
                        "registerOperator transaction sent, awaiting receipt..."
                    );

                    let receipt = pending.get_receipt().await?;
                    if !receipt.status() {
                        eyre::bail!("Transaction failed: {:?}", receipt)
                    }

                    info!("Succesfully registered Symbiotic operator");

                    Ok(())
                }
                SymbioticSubcommand::Deregister { rpc_url, operator_private_key } => {
                    let signer = PrivateKeySigner::from_bytes(&operator_private_key)
                        .wrap_err("valid private key")?;

                    let provider = ProviderBuilder::new()
                        .with_recommended_fillers()
                        .wallet(EthereumWallet::from(signer.clone()))
                        .on_http(rpc_url);

                    let chain_id = provider.get_chain_id().await?;
                    let chain = Chain::from_id(chain_id)
                        .unwrap_or_else(|| panic!("chain id {} not supported", chain_id));

                    let deployments = deployments_for_chain(chain);

                    info!(operator = %signer.address(), ?chain, "Deregistering Symbiotic operator");

                    request_confirmation();

                    let middleware = BoltSymbioticMiddleware::new(
                        deployments.bolt.symbiotic_middleware,
                        provider.clone(),
                    );

                    let pending = middleware.deregisterOperator().send().await?;

                    info!(
                        hash = ?pending.tx_hash(),
                        "deregisterOperator transaction sent, awaiting receipt..."
                    );

                    let receipt = pending.get_receipt().await?;
                    if !receipt.status() {
                        eyre::bail!("Transaction failed: {:?}", receipt)
                    }

                    info!("Succesfully deregistered Symbiotic operator");

                    Ok(())
                }
                SymbioticSubcommand::Status { rpc_url, address } => {
                    let provider = ProviderBuilder::new().on_http(rpc_url.clone());
                    let chain_id = provider.get_chain_id().await?;
                    let chain = Chain::from_id(chain_id)
                        .unwrap_or_else(|| panic!("chain id {} not supported", chain_id));

                    let deployments = deployments_for_chain(chain);
                    let bolt_manager =
                        BoltManagerContract::new(deployments.bolt.manager, provider.clone());
                    if bolt_manager.isOperator(address).call().await?._0 {
                        info!(?address, "Symbiotic operator is registered");
                    } else {
                        warn!(?address, "Operator not registered");
                    }

                    Ok(())
                }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::process::{Command, Output};

    use crate::{
        cli::{
            Chain, EigenLayerSubcommand, OperatorsCommand, OperatorsSubcommand, SymbioticSubcommand,
        },
        contracts::{
            deployments_for_chain,
            eigenlayer::{DelegationManager, IStrategy, OperatorDetails},
            strategy_to_address, EigenLayerStrategy,
        },
    };
    use alloy::{
        network::EthereumWallet,
        primitives::{address, keccak256, utils::parse_units, Address, B256, U256},
        providers::{ext::AnvilApi, Provider, ProviderBuilder, WalletProvider},
        signers::local::PrivateKeySigner,
        sol_types::SolValue,
    };
    use rand::Rng;

    #[tokio::test]
    async fn test_eigenlayer_flow() {
        let _ = tracing_subscriber::fmt().try_init();
        let mut rnd = rand::thread_rng();
        let secret_key = B256::from(rnd.gen::<[u8; 32]>());
        let wallet = PrivateKeySigner::from_bytes(&secret_key).expect("valid private key");

        let rpc_url = "https://holesky.drpc.org";
        let provider = ProviderBuilder::new()
            .with_recommended_fillers()
            .wallet(EthereumWallet::from(wallet))
            .on_anvil_with_config(|anvil| anvil.fork(rpc_url));
        let anvil_url = provider.client().transport().url();

        let account = provider.default_signer_address();

        // Add balance to the operator
        provider.anvil_set_balance(account, U256::from(u64::MAX)).await.expect("set balance");

        let deployments = deployments_for_chain(Chain::Holesky);

        let weth_strategy_address = strategy_to_address(
            EigenLayerStrategy::WEth,
            deployments.eigen_layer.supported_strategies,
        );
        let strategy = IStrategy::new(weth_strategy_address, provider.clone());
        let weth_address = strategy.underlyingToken().call().await.expect("underlying token").token;

        // Mock WETH balance using the Anvil API.
        let hashed_slot = keccak256((account, U256::from(3)).abi_encode());
        let mocked_balance: U256 = parse_units("100.0", "ether").expect("parse ether").into();
        provider
            .anvil_set_storage_at(weth_address, hashed_slot.into(), mocked_balance.into())
            .await
            .expect("to set storage");

        let random_address = Address::from(rnd.gen::<[u8; 20]>());

        // 1. Register the operator into EigenLayer. This should be done by the operator using the
        //    EigenLayer CLI, but we do it here for testing purposes.

        let delegation_manager =
            DelegationManager::new(deployments.eigen_layer.delegation_manager, provider.clone());
        let receipt = delegation_manager
            .registerAsOperator(
                OperatorDetails {
                    earningsReceiver: random_address,
                    delegationApprover: Address::ZERO,
                    stakerOptOutWindowBlocks: 32,
                },
                "https://bolt.chainbound.io/rpc".to_string(),
            )
            .send()
            .await
            .expect("to send register as operator")
            .get_receipt()
            .await
            .expect("to get receipt for register as operator");

        assert!(receipt.status(), "operator should be registered");
        println!("Registered operator with address {}", account);

        let is_operator = delegation_manager
            .isOperator(account)
            .call()
            .await
            .expect("to check if operator is registered")
            ._0;
        println!("is operator {}", is_operator);

        // 2. Deposit into the strategy

        let deposit_into_strategy = OperatorsCommand {
            subcommand: OperatorsSubcommand::EigenLayer {
                subcommand: EigenLayerSubcommand::Deposit {
                    rpc_url: anvil_url.parse().expect("valid url"),
                    operator_private_key: secret_key,
                    strategy: EigenLayerStrategy::WEth,
                    amount: U256::from(1),
                },
            },
        };

        deposit_into_strategy.run().await.expect("to deposit into strategy");

        // 3. Register the operator into Bolt AVS

        let register_operator = OperatorsCommand {
            subcommand: OperatorsSubcommand::EigenLayer {
                subcommand: EigenLayerSubcommand::Register {
                    rpc_url: anvil_url.parse().expect("valid url"),
                    operator_private_key: secret_key,
                    operator_rpc: "https://bolt.chainbound.io/rpc".parse().expect("valid url"),
                    salt: B256::ZERO,
                    expiry: U256::MAX,
                },
            },
        };

        register_operator.run().await.expect("to register operator");

        // 4. Check operator registration
        let check_operator_registration = OperatorsCommand {
            subcommand: OperatorsSubcommand::EigenLayer {
                subcommand: EigenLayerSubcommand::Status {
                    rpc_url: anvil_url.parse().expect("valid url"),
                    address: account,
                },
            },
        };

        check_operator_registration.run().await.expect("to check operator registration");

        let deregister_operator = OperatorsCommand {
            subcommand: OperatorsSubcommand::EigenLayer {
                subcommand: EigenLayerSubcommand::Deregister {
                    rpc_url: anvil_url.parse().expect("valid url"),
                    operator_private_key: secret_key,
                },
            },
        };

        deregister_operator.run().await.expect("to deregister operator");

        let check_operator_registration = OperatorsCommand {
            subcommand: OperatorsSubcommand::EigenLayer {
                subcommand: EigenLayerSubcommand::Status {
                    rpc_url: anvil_url.parse().expect("valid url"),
                    address: account,
                },
            },
        };

        check_operator_registration.run().await.expect("to check operator registration");
    }

    /// Ignored since it requires Symbiotic CLI: https://docs.symbiotic.fi/guides/cli/#installation
    /// To run this test, install the CLI, and then move the binary in the `symbiotic-cli` directory
    /// which is git-ignored for this purpose.
    #[tokio::test]
    #[ignore = "requires Symbiotic CLI installed"]
    async fn test_symbiotic_flow() {
        let mut rnd = rand::thread_rng();
        let secret_key = B256::from(rnd.gen::<[u8; 32]>());
        let wallet = PrivateKeySigner::from_bytes(&secret_key).expect("valid private key");

        let rpc_url = "https://rpc-holesky.bolt.chainbound.io/rpc";
        let provider = ProviderBuilder::new()
            .with_recommended_fillers()
            .wallet(EthereumWallet::from(wallet))
            .on_anvil_with_config(|anvil| anvil.fork(rpc_url));
        let anvil_url = provider.client().transport().url();

        let account = provider.default_signer_address();

        // Add balance to the operator
        provider.anvil_set_balance(account, U256::from(u64::MAX)).await.expect("set balance");

        let deployments = deployments_for_chain(Chain::Holesky);

        let weth_address = address!("94373a4919B3240D86eA41593D5eBa789FEF3848");

        // Mock WETH balance using the Anvil API.
        let hashed_slot = keccak256((account, U256::from(3)).abi_encode());
        let mocked_balance: U256 = parse_units("100.0", "ether").expect("parse ether").into();
        provider
            .anvil_set_storage_at(weth_address, hashed_slot.into(), mocked_balance.into())
            .await
            .expect("to set storage");

        let print_output = |output: Output| {
            println!("{}", String::from_utf8_lossy(&output.stdout));
        };

        // We now follow the steps described in the Holesky guide

        let register_operator = Command::new("python3")
            .arg("symbiotic-cli/symb.py")
            .arg("--chain")
            .arg("holesky")
            .arg("--provider")
            .arg(anvil_url)
            .arg("register-operator")
            .arg("--private-key")
            .arg(secret_key.to_string())
            .output()
            .expect("to register operator");

        print_output(register_operator);

        let opt_in_network = Command::new("python3")
            .arg("symbiotic-cli/symb.py")
            .arg("--chain")
            .arg("holesky")
            .arg("--provider")
            .arg(anvil_url)
            .arg("opt-in-network")
            .arg("--private-key")
            .arg(secret_key.to_string())
            .arg(deployments.symbiotic.network.to_string())
            .output()
            .expect("to opt-in-network");

        print_output(opt_in_network);

        let vault = deployments.symbiotic.supported_vaults[3]; // WETH vault

        let opt_in_vault = Command::new("python3")
            .arg("symbiotic-cli/symb.py")
            .arg("--chain")
            .arg("holesky")
            .arg("--provider")
            .arg(anvil_url)
            .arg("opt-in-vault")
            .arg("--private-key")
            .arg(secret_key.to_string())
            .arg(vault.to_string())
            .output()
            .expect("to opt-in-vault");

        print_output(opt_in_vault);

        let deposit = Command::new("python3")
            .arg("symbiotic-cli/symb.py")
            .arg("--chain")
            .arg("holesky")
            .arg("--provider")
            .arg(anvil_url)
            .arg("deposit")
            .arg("--private-key")
            .arg(secret_key.to_string())
            .arg(vault.to_string())
            .arg("1") // 1 ether
            .output()
            .expect("to opt-in-vault");

        print_output(deposit);

        let register_into_bolt = OperatorsCommand {
            subcommand: OperatorsSubcommand::Symbiotic {
                subcommand: SymbioticSubcommand::Register {
                    rpc_url: anvil_url.parse().expect("valid url"),
                    operator_private_key: secret_key,
                    operator_rpc: "https://bolt.chainbound.io".parse().expect("valid url"),
                },
            },
        };

        register_into_bolt.run().await.expect("to register into bolt");

        let check_status = OperatorsCommand {
            subcommand: OperatorsSubcommand::Symbiotic {
                subcommand: SymbioticSubcommand::Status {
                    rpc_url: anvil_url.parse().expect("valid url"),
                    address: account,
                },
            },
        };

        check_status.run().await.expect("to check operator status");
    }
}
