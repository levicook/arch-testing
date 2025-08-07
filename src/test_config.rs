use std::time::Duration;

use crate::containers::{
    BitcoinContainerConfig, LocalValidatorContainerConfig, TitanContainerConfig,
};

pub const MAX_SETUP_TIMEOUT: Duration = Duration::from_secs(120); // 2 minutes
pub const MAX_TEST_TIMEOUT: Duration = Duration::from_secs(300); // 5 minutes

pub const DEFAULT_SETUP_TIMEOUT: Duration = Duration::from_secs(15); // 15 seconds for container startup and sync
pub const DEFAULT_TEST_TIMEOUT: Duration = Duration::from_secs(30); // 30 seconds for test execution

/// Test configuration
#[derive(Debug, Clone)]
pub struct TestRunnerConfig {
    pub bitcoin_image_name: String,
    pub bitcoin_image_tag: String,
    pub titan_image_name: String,
    pub titan_image_tag: String,
    pub validator_image_name: String,
    pub validator_image_tag: String,

    pub setup_timeout: Duration,
    pub test_timeout: Duration,

    // Port configuration
    pub bitcoin_rpc_port: u16,
    pub titan_http_port: u16,
    pub titan_tcp_port: u16,
    pub validator_rpc_port: u16,
    pub validator_websocket_port: u16,
}

impl TestRunnerConfig {
    pub fn new() -> anyhow::Result<Self> {
        let default_bitcoin_config = BitcoinContainerConfig::default();
        let default_titan_config = TitanContainerConfig::default();
        let default_validator_config = LocalValidatorContainerConfig::default();

        Ok(Self {
            bitcoin_image_name: default_bitcoin_config.image_name,
            bitcoin_image_tag: default_bitcoin_config.image_tag,
            bitcoin_rpc_port: default_bitcoin_config.rpc_port,

            titan_http_port: default_titan_config.http_port,
            titan_image_name: default_titan_config.image_name,
            titan_image_tag: default_titan_config.image_tag,
            titan_tcp_port: default_titan_config.tcp_port,

            validator_image_name: default_validator_config.image_name,
            validator_image_tag: default_validator_config.image_tag,
            validator_rpc_port: default_validator_config.rpc_port,
            validator_websocket_port: default_validator_config.websocket_port,

            setup_timeout: DEFAULT_SETUP_TIMEOUT,
            test_timeout: DEFAULT_TEST_TIMEOUT,
        })
    }
}

impl From<TestRunnerConfig> for BitcoinContainerConfig {
    fn from(config: TestRunnerConfig) -> Self {
        let default_bitcoin_config = BitcoinContainerConfig::default();
        Self {
            container_name: default_bitcoin_config.container_name,
            image_name: config.bitcoin_image_name,
            image_tag: config.bitcoin_image_tag,
            rpc_password: default_bitcoin_config.rpc_password,
            rpc_port: config.bitcoin_rpc_port,
            rpc_user: default_bitcoin_config.rpc_user,
            startup_timeout: config.setup_timeout,
            tcp_port: default_bitcoin_config.tcp_port,
        }
    }
}

impl From<TestRunnerConfig> for TitanContainerConfig {
    fn from(config: TestRunnerConfig) -> Self {
        let default_titan_config = TitanContainerConfig::default();
        Self {
            container_name: default_titan_config.container_name,
            image_name: config.titan_image_name,
            image_tag: config.titan_image_tag,
            http_port: config.titan_http_port,
            tcp_port: config.titan_tcp_port,
            startup_timeout: config.setup_timeout,
        }
    }
}

impl From<TestRunnerConfig> for LocalValidatorContainerConfig {
    fn from(config: TestRunnerConfig) -> Self {
        let default_validator_config = LocalValidatorContainerConfig::default();
        Self {
            container_name: default_validator_config.container_name,
            image_name: config.validator_image_name,
            image_tag: config.validator_image_tag,
            rpc_port: config.validator_rpc_port,
            websocket_port: config.validator_websocket_port,
            startup_timeout: config.setup_timeout,
        }
    }
}
