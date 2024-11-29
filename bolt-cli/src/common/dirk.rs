use std::fs;

use alloy::primitives::B256;
use ethereum_consensus::crypto::bls::Signature as BlsSignature;
use eyre::{bail, Context, Result};
use tonic::transport::{Certificate, Channel, ClientTlsConfig, Identity};
use tracing::debug;

use crate::{
    cli::TlsCredentials,
    pb::eth2_signer_api::{
        AccountManagerClient, ListAccountsRequest, ListAccountsResponse, ListerClient,
        LockAccountRequest, ResponseState, SignRequest, SignRequestId, SignerClient,
        UnlockAccountRequest,
    },
};

/// A Dirk remote signer.
///
/// Available services:
/// - `Lister`: List accounts in the keystore.
/// - `Signer`: Request a signature from the remote signer.
/// - `AccountManager`: Manage accounts in the keystore (lock and unlock accounts).
///
/// Reference: https://github.com/attestantio/dirk
#[derive(Clone)]
pub struct Dirk {
    lister: ListerClient<Channel>,
    signer: SignerClient<Channel>,
    account_mng: AccountManagerClient<Channel>,
}

impl Dirk {
    /// Connect to the DIRK server with the given address and TLS credentials.
    pub async fn connect(addr: String, credentials: TlsCredentials) -> Result<Self> {
        let addr = addr.parse()?;
        let tls_config = compose_credentials(credentials)?;
        let conn = Channel::builder(addr).tls_config(tls_config)?.connect().await?;

        let lister = ListerClient::new(conn.clone());
        let signer = SignerClient::new(conn.clone());
        let account_mng = AccountManagerClient::new(conn);

        Ok(Self { lister, signer, account_mng })
    }

    /// List all accounts in the keystore.
    pub async fn list_accounts(&mut self, wallet_path: String) -> Result<ListAccountsResponse> {
        // Request all accounts in the given path. Only one path at a time
        // as done in https://github.com/wealdtech/go-eth2-wallet-dirk/blob/182f99b22b64d01e0d4ae67bf47bb055763465d7/grpc.go#L121
        let req = ListAccountsRequest { paths: vec![wallet_path] };
        let res = self.lister.list_accounts(req).await?.into_inner();

        if !matches!(res.state(), ResponseState::Succeeded) {
            bail!("Failed to list accounts: {:?}", res);
        }

        debug!(
            accounts = %res.accounts.len(),
            distributed_accounts = %res.distributed_accounts.len(),
            "List accounts request succeeded"
        );

        Ok(res)
    }

    /// Unlock an account in the keystore with the given passphrase.
    pub async fn unlock_account(
        &mut self,
        account_name: String,
        passphrase: String,
    ) -> Result<bool> {
        let pf_bytes = passphrase.as_bytes().to_vec();
        let req = UnlockAccountRequest { account: account_name.clone(), passphrase: pf_bytes };
        let res = self.account_mng.unlock(req).await?.into_inner();

        match res.state() {
            ResponseState::Succeeded => {
                debug!("Unlock request succeeded for account {}", account_name);
                Ok(true)
            }
            ResponseState::Denied => {
                debug!("Unlock request denied for account {}", account_name);
                Ok(false)
            }
            ResponseState::Unknown => bail!("Unknown response from unlock account: {:?}", res),
            ResponseState::Failed => bail!("Failed to unlock account: {:?}", res),
        }
    }

    /// Lock an account in the keystore.
    pub async fn lock_account(&mut self, account_name: String) -> Result<bool> {
        let req = LockAccountRequest { account: account_name.clone() };
        let res = self.account_mng.lock(req).await?.into_inner();

        match res.state() {
            ResponseState::Succeeded => {
                debug!("Lock request succeeded for account {}", account_name);
                Ok(true)
            }
            ResponseState::Denied => {
                debug!("Lock request denied for account {}", account_name);
                Ok(false)
            }
            ResponseState::Unknown => bail!("Unknown response from lock account: {:?}", res),
            ResponseState::Failed => bail!("Failed to lock account: {:?}", res),
        }
    }

    /// Request a signature from the remote signer.
    pub async fn request_signature(
        &mut self,
        account_name: String,
        hash: B256,
        domain: B256,
    ) -> Result<BlsSignature> {
        let req = SignRequest {
            data: hash.to_vec(),
            domain: domain.to_vec(),
            id: Some(SignRequestId::Account(account_name.clone())),
        };

        let res = self.signer.sign(req).await?.into_inner();

        if !matches!(res.state(), ResponseState::Succeeded) {
            bail!("Failed to sign data: {:?}", res);
        }
        if res.signature.is_empty() {
            bail!("Empty signature returned");
        }

        let sig = BlsSignature::try_from(res.signature.as_slice())
            .wrap_err("Failed to parse signature")?;

        debug!("Signature request succeeded for account {}", account_name);
        Ok(sig)
    }
}

/// Compose the TLS credentials from the given paths.
fn compose_credentials(creds: TlsCredentials) -> Result<ClientTlsConfig> {
    let client_cert = fs::read(creds.client_cert_path).wrap_err("Failed to read client cert")?;
    let client_key = fs::read(creds.client_key_path).wrap_err("Failed to read client key")?;

    // Create client identity (certificate + key)
    let identity = Identity::from_pem(&client_cert, &client_key);

    // Configure the TLS client
    let mut tls_config = ClientTlsConfig::new().identity(identity);

    // Add CA certificate if provided
    if let Some(ca_path) = creds.ca_cert_path {
        let ca_cert = fs::read(ca_path).wrap_err("Failed to read CA certificate")?;
        tls_config = tls_config.ca_certificate(Certificate::from_pem(&ca_cert));
    }

    Ok(tls_config)
}

#[cfg(test)]
pub mod test_util {
    use eyre::{bail, Context};
    use rustls::crypto::CryptoProvider;
    use std::{
        fs,
        process::{Child, Command},
        time::Duration,
    };

    use super::Dirk;
    use crate::cli::TlsCredentials;

    /// Initialize the default TLS provider for the tests if not already set.
    pub fn try_init_tls_provider() {
        // Init the default rustls provider
        if CryptoProvider::get_default().is_none() {
            let _ = rustls::crypto::ring::default_provider().install_default();
        }
    }

    /// Start a DIRK test server for testing (run on localhost:9091).
    /// This is a single instance (non distributed).
    ///
    /// Returns the DIRK client and the corresponding server process handle.
    pub async fn start_single_dirk_test_server() -> eyre::Result<(Dirk, Child)> {
        try_init_tls_provider();

        // Check if dirk is installed (in $PATH)
        if Command::new("dirk").arg("--help").status().is_err() {
            bail!("DIRK is not installed in $PATH");
        }

        let test_data_dir = env!("CARGO_MANIFEST_DIR").to_string() + "/test_data/dirk_single";

        // read the template json file from test_data
        let template_path = test_data_dir.clone() + "/dirk.template.json";
        let template = fs::read_to_string(template_path).wrap_err("Failed to read template")?;

        // change the occurrence of $PWD to the current working directory in the template
        let new_file = test_data_dir.clone() + "/dirk.json";
        let new_content = template.replace("$PWD", &test_data_dir);
        fs::write(new_file, new_content).wrap_err("Failed to write dirk config file")?;

        // Start the DIRK server in the background
        let dirk_proc = Command::new("dirk").arg("--base-dir").arg(&test_data_dir).spawn()?;

        // Wait for some time for the server to start up
        tokio::time::sleep(Duration::from_secs(3)).await;

        let url = "https://localhost:9091".to_string();

        let cred = TlsCredentials {
            client_cert_path: test_data_dir.clone() + "/client1.crt",
            client_key_path: test_data_dir.clone() + "/client1.key",
            ca_cert_path: Some(test_data_dir.clone() + "/security/ca.crt"),
        };

        let dirk = Dirk::connect(url, cred).await?;

        Ok((dirk, dirk_proc))
    }

    /// Start a multi-node DIRK test server for testing.
    /// This is a distributed instance with multiple nodes.
    ///
    /// Returns the DIRK client and the corresponding server process handles.
    ///
    /// NOTE: in order for the example certificates to work on your machine, you need to
    /// modify the /etc/hosts file to include the following entry:
    ///
    /// ```text
    /// 127.0.0.1       localhost localhost-1 localhost-2 localhost-3
    /// ```
    ///
    /// This is because we map 3 different server certificates to localhost in order
    /// to test the multi-node functionality of DIRK.
    pub async fn start_multi_dirk_test_server() -> eyre::Result<(Dirk, Vec<Child>)> {
        try_init_tls_provider();

        // Check if dirk is installed (in $PATH)
        if Command::new("dirk").arg("--help").status().is_err() {
            bail!("DIRK is not installed in $PATH");
        }

        let test_data_dir = env!("CARGO_MANIFEST_DIR").to_string() + "/test_data/dirk_multi";

        // directories containing the individual configuration for each instance
        let dirk_ids = ["1", "2", "3"];
        let mut dirk_procs = Vec::new();

        for dirk_id in dirk_ids {
            // Example: /test_data/dirk_multi/1
            let dirk_dir = test_data_dir.clone() + &format!("/{}", dirk_id);

            // read the template yml file from test_data
            let template_path = dirk_dir.clone() + "/dirk.template.yml";
            let template = fs::read_to_string(template_path).wrap_err("Failed to read template")?;

            // change the occurrence of $PWD to the current working directory in the template
            let new_file = dirk_dir.clone() + "/dirk.yml";
            let new_content = template.replace("$PWD", &test_data_dir);
            fs::write(new_file, new_content).wrap_err("Failed to write dirk config file")?;

            let dirk_proc = Command::new("dirk").arg("--base-dir").arg(&dirk_dir).spawn()?;
            dirk_procs.push(dirk_proc);

            // Wait for some time for each server to start up
            tokio::time::sleep(Duration::from_secs(2)).await;
        }

        // Note: the first server is used for the client connection
        let url = "https://localhost-1:8881".to_string();
        let cred = TlsCredentials {
            client_cert_path: test_data_dir.clone() + "/client/localhost.crt",
            client_key_path: test_data_dir.clone() + "/client/localhost.key",
            ca_cert_path: Some(test_data_dir.clone() + "/1/security/ca.crt"),
        };

        let dirk = Dirk::connect(url, cred).await?;

        Ok((dirk, dirk_procs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test connecting to a DIRK server and listing available accounts.
    ///
    /// ```shell
    /// cargo test --package bolt --bin bolt -- common::dirk::tests::test_dirk_single_connection_e2e
    /// --exact --show-output --ignored
    /// ```
    #[tokio::test]
    #[ignore = "Requires Dirk to be installed on the system"]
    async fn test_dirk_single_connection_e2e() -> eyre::Result<()> {
        let (mut dirk, mut dirk_proc) = test_util::start_single_dirk_test_server().await?;

        let accounts = dirk.list_accounts("wallet1".to_string()).await?;
        println!("Dirk Accounts: {:?}", accounts);

        // make sure to stop the dirk server
        dirk_proc.kill()?;

        Ok(())
    }

    /// Test unlocking an account in the DIRK server.
    /// This test requires a running DIRK server.
    ///
    /// ```shell
    /// cargo test --package bolt --bin bolt -- common::dirk::tests::test_unlock_account_e2e
    /// --exact --show-output --ignored
    /// ```
    #[tokio::test]
    #[ignore = "Requires Dirk to be installed on the system"]
    async fn test_unlock_account_e2e() -> eyre::Result<()> {
        let (mut dirk, mut dirk_proc) = test_util::start_single_dirk_test_server().await?;

        let account_name = "account1".to_string();
        let passphrase = "secret".to_string();

        let unlocked = dirk.unlock_account(account_name, passphrase).await?;
        println!("Account unlocked: {}", unlocked);

        // make sure to stop the dirk server
        dirk_proc.kill()?;

        Ok(())
    }

    /// Test locking an account in the DIRK server.
    ///
    /// ```shell
    /// cargo test --package bolt --bin bolt -- common::dirk::tests::test_dirk_multi_connection_e2e
    /// --exact --show-output --ignored
    /// ```
    ///
    /// NOTE: in order for the example certificates to work on your machine, you need to
    /// modify the /etc/hosts file to include the following entry:
    ///
    /// ```text
    /// 127.0.0.1       localhost localhost-1 localhost-2 localhost-3
    /// ```
    ///
    /// This is because we map 3 different server certificates to localhost in order
    /// to test the multi-node functionality of DIRK.
    #[tokio::test]
    #[ignore = "Requires Dirk to be installed on the system"]
    async fn test_dirk_multi_connection_e2e() -> eyre::Result<()> {
        let (mut dirk, mut dirk_procs) = test_util::start_multi_dirk_test_server().await?;

        let accounts = dirk.list_accounts("DistributedWallet1/1".to_string()).await?;
        println!("Dirk Accounts: {:?}", accounts);

        // make sure to stop the dirk servers
        for proc in &mut dirk_procs {
            proc.kill()?;
        }

        Ok(())
    }
}
