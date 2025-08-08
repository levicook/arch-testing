use std::time::Duration;

use anyhow::{Context, Result};
use testcontainers::{
    core::{logs::LogFrame, ContainerPort, WaitFor},
    runners::AsyncRunner,
    ContainerAsync, GenericImage, ImageExt,
};
use titan_client::TitanClient;

use super::bitcoin_container::BitcoinContainerConfig;

pub const DEFAULT_CONTAINER_NAME: &str = "arch-testing-titan-container";
pub const DEFAULT_IMAGE_NAME: &str = "ghcr.io/saturnbtc/titan";
pub const DEFAULT_IMAGE_TAG: &str = "latest";
pub const DEFAULT_HTTP_PORT: u16 = 3030; // HTTP API port
pub const DEFAULT_TCP_PORT: u16 = 8080; // TCP subscription port
pub const DEFAULT_STARTUP_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone)]
pub struct TitanContainerConfig {
    pub container_name: String,
    pub image_name: String,
    pub image_tag: String,
    pub http_port: u16,
    pub tcp_port: u16,
    pub startup_timeout: Duration,
}

impl Default for TitanContainerConfig {
    fn default() -> Self {
        Self {
            container_name: DEFAULT_CONTAINER_NAME.to_string(),
            image_name: DEFAULT_IMAGE_NAME.to_string(),
            image_tag: DEFAULT_IMAGE_TAG.to_string(),
            http_port: DEFAULT_HTTP_PORT,
            tcp_port: DEFAULT_TCP_PORT,
            startup_timeout: DEFAULT_STARTUP_TIMEOUT,
        }
    }
}

impl TitanContainerConfig {
    pub fn local_network_http_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.http_port)
    }

    pub fn local_network_tcp_address(&self) -> String {
        format!("127.0.0.1:{}", self.tcp_port)
    }

    pub fn docker_network_http_url(&self) -> String {
        format!("http://host.docker.internal:{}", self.http_port)
    }

    pub fn docker_network_tcp_address(&self) -> String {
        format!("host.docker.internal:{}", self.tcp_port)
    }

    pub fn docker_network_http_bind(&self) -> String {
        format!("0.0.0.0:{}", self.http_port)
    }

    pub fn docker_network_tcp_bind(&self) -> String {
        format!("0.0.0.0:{}", self.tcp_port)
    }

    /// Map ArchNetworkMode to Titan chain name
    pub fn titan_chain(&self) -> &'static str {
        "regtest"
    }
}

pub struct TitanContainer {
    pub container: ContainerAsync<GenericImage>,
    pub client: TitanClient,

    config: TitanContainerConfig,
}

impl TitanContainer {
    pub async fn start(
        bitcoin_config: &BitcoinContainerConfig,
        titan_config: &TitanContainerConfig,
    ) -> Result<Self> {
        let container = start_titan_container(bitcoin_config, titan_config).await?;
        let client = TitanClient::new(&titan_config.local_network_http_url());
        let config = titan_config.clone();

        Ok(Self {
            container,
            client,
            config,
        })
    }

    pub async fn shutdown(&self) -> Result<()> {
        tracing::trace!(
            "Stopping titan container: {} (image: {}:{})",
            self.config.container_name,
            self.config.image_name,
            self.config.image_tag
        );

        self.container.stop().await.map_err(|shutdown_err| {
            anyhow::anyhow!(
                "Failed to stop titan container: {} (image: {}:{})\nShutdown error: {}",
                self.config.container_name,
                self.config.image_name,
                self.config.image_tag,
                shutdown_err
            )
        })
    }
}

pub(super) async fn start_titan_container(
    bitcoin_config: &BitcoinContainerConfig,
    titan_config: &TitanContainerConfig,
) -> Result<ContainerAsync<GenericImage>> {
    tracing::trace!(
        "Starting titan container: {} (image: {}:{})",
        titan_config.container_name,
        titan_config.image_name,
        titan_config.image_tag
    );

    // PLEASE DO NOT REMOVE THIS LOG CONSUMER (yet)
    let log_consumer = |log_frame: &LogFrame| match log_frame {
        LogFrame::StdOut(bytes) => {
            let output = String::from_utf8_lossy(bytes);
            tracing::info!("titand> {}", output.trim());
        }
        LogFrame::StdErr(bytes) => {
            let output = String::from_utf8_lossy(bytes);
            tracing::info!("titand> {}", output.trim());
        }
    };

    // consider introducing an enum so callers can decide what to wait for
    let wait_for_synced_to_tip = WaitFor::message_on_stdout(
        "Synced to tip", // logged by titan when it's caught up with bitcoind
    );

    let container = GenericImage::new(&titan_config.image_name, &titan_config.image_tag)
        .with_wait_for(wait_for_synced_to_tip)
        .with_mapped_port(
            titan_config.tcp_port,
            ContainerPort::Tcp(titan_config.tcp_port),
        )
        .with_mapped_port(
            titan_config.http_port,
            ContainerPort::Tcp(titan_config.http_port),
        )
        .with_startup_timeout(titan_config.startup_timeout)
        .with_container_name(&titan_config.container_name)
        .with_log_consumer(log_consumer)
        .with_env_var("BITCOIN_RPC_PASSWORD", &bitcoin_config.rpc_password)
        .with_env_var("BITCOIN_RPC_URL", &bitcoin_config.docker_network_rpc_url())
        .with_env_var("BITCOIN_RPC_USERNAME", &bitcoin_config.rpc_user)
        .with_env_var("CHAIN", titan_config.titan_chain())
        .with_env_var("COMMIT_INTERVAL", "5")
        .with_env_var("HTTP_LISTEN", &titan_config.docker_network_http_bind())
        .with_env_var("RUST_BACKTRACE", "full")
        .with_env_var("TCP_ADDRESS", &titan_config.docker_network_tcp_bind())
        .start()
        .await
        .context("Failed to start Titan container")?;

    tracing::trace!(
        "Started titan container: {} (image: {}:{})",
        titan_config.container_name,
        titan_config.image_name,
        titan_config.image_tag
    );

    Ok(container)
}
