use std::time::Duration;

use anyhow::{Context, Result};
use arch_sdk::AsyncArchRpcClient;
use backoff::{future::retry, ExponentialBackoff};
use testcontainers::{
    core::{logs::LogFrame, ContainerPort},
    runners::AsyncRunner,
    ContainerAsync, GenericImage, ImageExt,
};

use super::titan_container::TitanContainerConfig;

pub const DEFAULT_CONTAINER_NAME: &str = "arch-testing-local-validator-container";
pub const DEFAULT_IMAGE_NAME: &str = "ghcr.io/arch-network/local_validator";
pub const DEFAULT_IMAGE_TAG: &str = "0.5.8";
pub const DEFAULT_RPC_PORT: u16 = 9002;
pub const DEFAULT_WEBSOCKET_PORT: u16 = 29002;
pub const DEFAULT_STARTUP_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone)]
pub struct LocalValidatorContainerConfig {
    pub container_name: String,
    pub image_name: String,
    pub image_tag: String,
    pub rpc_port: u16,
    pub websocket_port: u16,
    pub startup_timeout: Duration,
}

impl Default for LocalValidatorContainerConfig {
    fn default() -> Self {
        Self {
            container_name: DEFAULT_CONTAINER_NAME.to_string(),
            image_name: DEFAULT_IMAGE_NAME.to_string(),
            image_tag: DEFAULT_IMAGE_TAG.to_string(),
            rpc_port: DEFAULT_RPC_PORT,
            websocket_port: DEFAULT_WEBSOCKET_PORT,
            startup_timeout: DEFAULT_STARTUP_TIMEOUT,
        }
    }
}

impl LocalValidatorContainerConfig {
    pub fn local_network_rpc_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.rpc_port)
    }

    pub fn local_network_websocket_url(&self) -> String {
        format!("ws://127.0.0.1:{}", self.websocket_port)
    }

    pub fn docker_network_rpc_url(&self) -> String {
        format!("http://host.docker.internal:{}", self.rpc_port)
    }

    pub fn docker_network_websocket_url(&self) -> String {
        format!("ws://host.docker.internal:{}", self.websocket_port)
    }
}

pub struct LocalValidatorContainer {
    pub container: ContainerAsync<GenericImage>,
    pub client: AsyncArchRpcClient,
    config: LocalValidatorContainerConfig,
}

impl LocalValidatorContainer {
    pub async fn start(
        config: &LocalValidatorContainerConfig,
        titan_config: &TitanContainerConfig,
    ) -> Result<Self> {
        let container = start_local_validator_container(config, titan_config).await?;
        let config = config.clone();
        let client = AsyncArchRpcClient::new(&config.local_network_rpc_url());

        wait_for_rpc_ready(&client).await?;

        Ok(Self {
            container,
            client,
            config,
        })
    }

    pub async fn shutdown(&self) -> Result<()> {
        tracing::trace!(
            "Stopping local validator container: {} (image: {}:{})",
            self.config.container_name,
            self.config.image_name,
            self.config.image_tag
        );

        self.container.stop().await.map_err(|shutdown_err| {
            anyhow::anyhow!(
                "Failed to stop local validator container: {} (image: {}:{})\nShutdown error: {}",
                self.config.container_name,
                self.config.image_name,
                self.config.image_tag,
                shutdown_err
            )
        })
    }

    pub fn rpc_url(&self) -> String {
        self.config.local_network_rpc_url()
    }

    pub fn websocket_url(&self) -> String {
        self.config.local_network_websocket_url()
    }
}

pub(super) async fn start_local_validator_container(
    config: &LocalValidatorContainerConfig,
    titan_config: &TitanContainerConfig,
) -> Result<ContainerAsync<GenericImage>> {
    tracing::trace!(
        "Starting local validator container: {} (image: {}:{})",
        config.container_name,
        config.image_name,
        config.image_tag
    );

    // PLEASE DO NOT REMOVE THIS LOG CONSUMER (yet)
    let log_consumer = |log_frame: &LogFrame| match log_frame {
        LogFrame::StdOut(bytes) => {
            let output = String::from_utf8_lossy(bytes);
            tracing::info!("local_validator> {}", output.trim());
        }
        LogFrame::StdErr(bytes) => {
            let output = String::from_utf8_lossy(bytes);
            tracing::info!("local_validator> {}", output.trim());
        }
    };

    let titan_endpoint = titan_config.docker_network_http_url();
    let titan_socket_endpoint = titan_config.docker_network_tcp_address();

    let container = GenericImage::new(&config.image_name, &config.image_tag)
        .with_mapped_port(config.rpc_port, ContainerPort::Tcp(config.rpc_port))
        .with_mapped_port(
            config.websocket_port,
            ContainerPort::Tcp(config.websocket_port),
        )
        .with_startup_timeout(config.startup_timeout)
        .with_container_name(&config.container_name)
        .with_log_consumer(log_consumer)
        .with_env_var("RUST_BACKTRACE", "full")
        .with_cmd([
            "/bin/local_validator".to_string(),
            "--network-mode=localnet".to_string(),
            "--rpc-bind-ip=0.0.0.0".to_string(),
            format!("--rpc-bind-port={}", config.rpc_port),
            format!("--titan-endpoint={}", titan_endpoint),
            format!("--titan-socket-endpoint={}", titan_socket_endpoint),
        ])
        .start()
        .await
        .context("Failed to start local validator container")?;

    tracing::trace!(
        "Started local validator container: {} (image: {}:{})",
        config.container_name,
        config.image_name,
        config.image_tag
    );

    Ok(container)
}

async fn wait_for_rpc_ready(client: &AsyncArchRpcClient) -> Result<()> {
    retry(ExponentialBackoff::default(), || async {
        match client.get_block_count().await {
            Ok(_) => {
                tracing::info!("LocalValidator RPC server is ready!");
                Ok(())
            }
            Err(e) => {
                tracing::debug!("LocalValidator RPC not ready yet: {}", e);
                Err(backoff::Error::transient(anyhow::anyhow!(
                    "RPC not ready: {}",
                    e
                )))
            }
        }
    })
    .await
    .context("LocalValidator RPC server failed to become ready within timeout")
}
