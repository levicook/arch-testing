use std::future::Future;

use anyhow::{Context, Result, anyhow};
use arch_sdk::{ArchRpcClient, AsyncArchRpcClient};
use bitcoin::Network;
use tokio::time::timeout;

use crate::{
    containers::{
        BitcoinContainer, BitcoinContainerConfig, LocalValidatorContainer,
        LocalValidatorContainerConfig, TitanContainer, TitanContainerConfig,
    },
    init_tracing,
    test_config::{MAX_SETUP_TIMEOUT, MAX_TEST_TIMEOUT, TestRunnerConfig},
    test_context::TestContext,
};

pub struct TestRunner {
    bitcoin_container: Option<BitcoinContainer>,
    titan_container: Option<TitanContainer>,
    local_validator_conainer: Option<LocalValidatorContainer>,
}

impl TestRunner {
    pub async fn run<F, Fut>(test_fn: F)
    where
        F: FnOnce(TestContext) -> Fut,
        Fut: Future<Output = Result<()>>,
    {
        let config = TestRunnerConfig::new().expect("Failed to create test config");
        Self::run_with_config(config, test_fn).await;
    }

    pub async fn run_with_config<F, Fut>(config: TestRunnerConfig, test_fn: F)
    where
        F: FnOnce(TestContext) -> Fut,
        Fut: Future<Output = Result<()>>,
    {
        init_tracing();

        let mut ctx = Self {
            bitcoin_container: None,
            titan_container: None,
            local_validator_conainer: None,
        };

        let setup_result = ctx.setup_with_timeout(&config).await;

        let final_result = match setup_result {
            Ok(_) => ctx.test_with_timeout(&config, test_fn).await,
            Err(setup_err) => Err(setup_err),
        };

        // IMPORTANT: Always teardown, regardless of {setup, test} success or failure
        ctx.teardown().await;

        if let Err(e) = final_result {
            panic!("Test run failed: {}", e);
        }
    }

    // fn build_async_program_deployer(&self) -> Result<AsyncProgramDeployer> {
    //     Ok(AsyncProgramDeployer::new(
    //         &self.get_rpc_url()?,
    //         Network::Regtest,
    //     ))
    // }

    fn build_async_arch_rpc_client(&self) -> Result<AsyncArchRpcClient> {
        Ok(AsyncArchRpcClient::new(&self.get_rpc_url()?))
    }

    fn build_arch_rpc_client(&self) -> Result<ArchRpcClient> {
        Ok(ArchRpcClient::new(&self.get_rpc_url()?))
    }

    fn get_rpc_url(&self) -> Result<String> {
        let validator = self.get_validator()?;
        Ok(validator.rpc_url())
    }

    fn get_validator(&self) -> Result<&LocalValidatorContainer> {
        self.local_validator_conainer
            .as_ref()
            .ok_or(anyhow!("Validator not found"))
    }

    async fn setup_with_timeout(&mut self, config: &TestRunnerConfig) -> Result<()> {
        let setup_timeout = if config.setup_timeout > MAX_SETUP_TIMEOUT {
            tracing::warn!(
                "Configured setup_timeout {:?} exceeds maximum {:?}. Capping at maximum",
                config.setup_timeout,
                MAX_SETUP_TIMEOUT
            );
            MAX_SETUP_TIMEOUT
        } else {
            config.setup_timeout
        };

        match timeout(setup_timeout, self.setup_internal(config)).await {
            Ok(result) => result,
            Err(e) => Err(e.into()),
        }
    }

    async fn setup_internal(&mut self, config: &TestRunnerConfig) -> Result<()> {
        let bitcoin_config = BitcoinContainerConfig::from(config.clone());
        self.bitcoin_container = Some(
            BitcoinContainer::start(&bitcoin_config).await?, //
        );
        tracing::debug!("Bitcoin container started");

        let titan_config = TitanContainerConfig::from(config.clone());
        self.titan_container = Some(
            TitanContainer::start(&bitcoin_config, &titan_config).await?, //
        );
        tracing::debug!("Titan container started");

        let local_validator_config = LocalValidatorContainerConfig::from(config.clone());
        self.local_validator_conainer = Some(
            LocalValidatorContainer::start(&local_validator_config, &titan_config).await?, //
        );
        tracing::debug!("Validator container started");

        Ok(())
    }

    async fn test_with_timeout<F, Fut>(&self, config: &TestRunnerConfig, test_fn: F) -> Result<()>
    where
        F: FnOnce(TestContext) -> Fut,
        Fut: Future<Output = Result<()>>,
    {
        // todo: let config = config.normalize();
        let test_timeout = if config.test_timeout > MAX_TEST_TIMEOUT {
            tracing::warn!(
                "Configured test_timeout of {:?} exceeds maximum: {:?}. Capping at maximum",
                config.test_timeout,
                MAX_TEST_TIMEOUT
            );
            MAX_TEST_TIMEOUT
        } else {
            config.test_timeout
        };

        let ctx = TestContext::new(
            self.build_async_arch_rpc_client()?,
            self.build_arch_rpc_client()?,
            // self.build_async_program_deployer()?,
            Network::Regtest,
        );

        match timeout(test_timeout, test_fn(ctx)).await {
            Ok(test_result) => test_result,
            Err(e) => Err(e.into()),
        }
    }

    async fn teardown(&mut self) {
        tracing::trace!("Starting teardown...");

        // Stop validator container first
        if let Some(validator_container) = self.local_validator_conainer.take() {
            validator_container
                .shutdown()
                .await
                .context("Failed to stop validator container")
                .unwrap();
        }

        // Stop Titan container
        if let Some(titan_container) = self.titan_container.take() {
            titan_container
                .shutdown()
                .await
                .context("Failed to stop titan container")
                .unwrap();
        }

        // Stop Bitcoin container
        if let Some(bitcoin_container) = self.bitcoin_container.take() {
            bitcoin_container
                .shutdown()
                .await
                .context("Failed to stop bitcoin container")
                .unwrap();
        }

        tracing::debug!("Completed teardown");
    }
}
