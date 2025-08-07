use std::time::Duration;

use anyhow::{Context, Result};
use backoff::{ExponentialBackoff, retry};
use bitcoincore_rpc::{Client, RpcApi};
use testcontainers::{
    ContainerAsync, GenericImage, ImageExt,
    core::{ContainerPort, logs::LogFrame},
    runners::AsyncRunner,
};
use tokio::task::spawn_blocking;

pub const DEFAULT_CONTAINER_NAME: &str = "arch-testing-bitcoin-container";
pub const DEFAULT_IMAGE_NAME: &str = "bitcoin/bitcoin";
pub const DEFAULT_IMAGE_TAG: &str = "29.0";
pub const DEFAULT_RPC_PORT: u16 = 18443;
pub const DEFAULT_STARTUP_TIMEOUT: Duration = Duration::from_secs(60);
pub const DEFAULT_TCP_PORT: u16 = 18444;

#[derive(Debug, Clone)]
pub struct BitcoinContainerConfig {
    pub container_name: String,
    pub image_name: String,
    pub image_tag: String,
    pub rpc_port: u16,
    pub rpc_user: String,
    pub rpc_password: String,
    pub tcp_port: u16,
    pub startup_timeout: Duration,
}

impl BitcoinContainerConfig {
    pub fn docker_network_rpc_url(&self) -> String {
        format!("http://host.docker.internal:{}", self.rpc_port)
    }

    pub fn docker_network_tcp_address(&self) -> String {
        format!("host.docker.internal:{}", self.tcp_port)
    }

    pub fn local_network_rpc_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.rpc_port)
    }

    pub fn local_network_tcp_address(&self) -> String {
        format!("127.0.0.1:{}", self.tcp_port)
    }

    /// Map ArchNetworkMode to Bitcoin network flag
    pub fn bitcoin_network_flag(&self) -> &'static str {
        "-regtest=1"
    }
}

impl Default for BitcoinContainerConfig {
    fn default() -> Self {
        Self {
            container_name: DEFAULT_CONTAINER_NAME.to_string(),
            image_name: DEFAULT_IMAGE_NAME.to_string(),
            image_tag: DEFAULT_IMAGE_TAG.to_string(),
            rpc_port: DEFAULT_RPC_PORT,
            rpc_user: "bitcoind_username".to_string(),
            rpc_password: "bitcoind_password".to_string(),
            startup_timeout: DEFAULT_STARTUP_TIMEOUT,
            tcp_port: DEFAULT_TCP_PORT,
        }
    }
}

impl From<&BitcoinContainerConfig> for bitcoincore_rpc::Auth {
    fn from(config: &BitcoinContainerConfig) -> Self {
        bitcoincore_rpc::Auth::UserPass(config.rpc_user.clone(), config.rpc_password.clone())
    }
}

pub struct BitcoinContainer {
    pub container: ContainerAsync<GenericImage>,
    pub client: Client,

    config: BitcoinContainerConfig,
}

impl BitcoinContainer {
    pub async fn start(config: &BitcoinContainerConfig) -> Result<Self> {
        let container = start_container(config).await?;

        let rpc_url = config.local_network_rpc_url();

        let client = Client::new(&rpc_url, config.into())
            .with_context(|| format!("Failed to create rpc_client for {}", rpc_url))?;

        wait_for_rpc_ready(&rpc_url, config).await?;

        match client.create_wallet("testwallet", None, None, None, None) {
            Ok(_) => {
                tracing::info!("Successfully created testwallet");
            }
            Err(e) => {
                tracing::error!("Failed to create testwallet: {}", e);
                tracing::error!("Error details: {:?}", e);
                return Err(anyhow::anyhow!("Failed to create testwallet: {}", e));
            }
        }

        let address = client
            .get_new_address(None, None)
            .context("Failed to get new address")?
            .assume_checked();

        client
            .generate_to_address(100, &address)
            .with_context(|| format!("Failed to generate to address: {}", address))?;

        Ok(Self {
            container,
            client,
            config: config.clone(),
        })
    }

    pub async fn shutdown(&self) -> Result<()> {
        tracing::trace!(
            "Stopping bitcoin container: {} (image: {}:{})",
            self.config.container_name,
            self.config.image_name,
            self.config.image_tag
        );

        self.container.stop().await.map_err(|shutdown_err| {
            anyhow::anyhow!(
                "Failed to stop bitcoin container: {} (image: {}:{})\nShutdown error: {}",
                self.config.container_name,
                self.config.image_name,
                self.config.image_tag,
                shutdown_err
            )
        })
    }
}

async fn start_container(config: &BitcoinContainerConfig) -> Result<ContainerAsync<GenericImage>> {
    tracing::trace!(
        "Starting bitcoin container: {} (image: {}:{})",
        config.container_name,
        config.image_name,
        config.image_tag
    );

    // PLEASE DO NOT REMOVE THIS LOG CONSUMER (yet)
    let log_consumer = |log_frame: &LogFrame| match log_frame {
        LogFrame::StdOut(bytes) => {
            let output = String::from_utf8_lossy(bytes);
            tracing::info!("bitcoind> {}", output.trim());
        }
        LogFrame::StdErr(bytes) => {
            let output = String::from_utf8_lossy(bytes);
            tracing::info!("bitcoind> {}", output.trim());
        }
    };

    // Build command args conditionally based on network mode
    let mut cmd_args = vec![
        "bitcoind".to_string(),
        "-datadir=/var/lib/bitcoin-core".to_string(),
        "-fallbackfee=0.00000001".to_string(),
        "-printtoconsole".to_string(),
    ];

    // Add network flag only if it's not empty (mainnet has no flag)
    let network_flag = config.bitcoin_network_flag();
    if !network_flag.is_empty() {
        cmd_args.push(network_flag.to_string());
    }

    cmd_args.extend_from_slice(&[
        "-rpcallowip=0.0.0.0/0".to_string(),
        "-rpcbind=0.0.0.0".to_string(),
        format!("-rpcport={}", config.rpc_port),
        format!("-rpcuser={}", config.rpc_user),
        format!("-rpcpassword={}", config.rpc_password),
    ]);

    let container = GenericImage::new(&config.image_name, &config.image_tag)
        .with_mapped_port(config.rpc_port, ContainerPort::Tcp(config.rpc_port))
        .with_container_name(&config.container_name)
        .with_startup_timeout(config.startup_timeout)
        .with_log_consumer(log_consumer)
        .with_env_var("BITCOIN_DATA", "/var/lib/bitcoin-core")
        .with_cmd(cmd_args)
        .start()
        .await
        .context("Failed to start Bitcoin container")?;

    tracing::debug!(
        "Started bitcoin container: {} (image: {}:{})",
        config.container_name,
        config.image_name,
        config.image_tag
    );

    Ok(container)
}

/// Wait for the RPC server to be ready using exponential backoff
// TODO why can't we just accept a client here?
async fn wait_for_rpc_ready(rpc_url: &str, config: &BitcoinContainerConfig) -> Result<()> {
    let backoff = ExponentialBackoff::default();
    let rpc_url = rpc_url.to_string();
    let auth = bitcoincore_rpc::Auth::from(config);

    spawn_blocking(move || {
        retry(backoff, || {
            match Client::new(&rpc_url, auth.clone()).and_then(|client| client.get_block_count()) {
                Ok(_) => {
                    tracing::info!("Bitcoin RPC server is ready!");
                    Ok(())
                }
                Err(e) => {
                    tracing::debug!("Bitcoin RPC not ready yet: {}", e);
                    Err(backoff::Error::transient(anyhow::anyhow!(
                        "RPC not ready: {}",
                        e
                    )))
                }
            }
        })
    })
    .await
    .context("Failed to spawn blocking task")?
    .map_err(|e| {
        anyhow::anyhow!(
            "Bitcoin RPC server failed to become ready within timeout: {}",
            e
        )
    })
}
