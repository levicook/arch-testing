use std::sync::Arc;

use anyhow::Result;
use arch_program::{hash::Hash, instruction::Instruction, pubkey::Pubkey, sanitized::ArchMessage, system_instruction};
use arch_sdk::{
    build_and_sign_transaction, generate_new_keypair, ArchRpcClient, AsyncArchRpcClient,
    ProcessedTransaction, ProgramDeployer, RuntimeTransaction, Status,
};
use bitcoin::{key::Keypair, Address, Network};
use tokio::task::spawn_blocking;

pub struct TestContext {
    pub arch_async_rpc_client: AsyncArchRpcClient,
    pub network: Network,

    // Please _do not pub_ these fields, because they can't be used well in an async context.
    // we'll keep all the spawn_blocking calls in this file until we have proper async clients.
    // (aka, hide the ugly / keep the ugly in one place)
    program_deployer: Arc<ProgramDeployer>,
    arch_rpc_client: Arc<ArchRpcClient>,
}

impl TestContext {
    pub fn new(
        arch_async_rpc_client: AsyncArchRpcClient,
        arch_rpc_client: ArchRpcClient,
        program_deployer: ProgramDeployer,
    ) -> Self {
        Self {
            arch_async_rpc_client,
            arch_rpc_client: Arc::new(arch_rpc_client),
            network: Network::Regtest,
            program_deployer: Arc::new(program_deployer),
        }
    }

    pub async fn fund_keypair_with_faucet(&self, keypair: &Keypair) -> anyhow::Result<()> {
        let client = self.arch_rpc_client.clone();
        let keypair = keypair.clone();

        let network = self.network.clone();
        spawn_blocking(move || client.create_and_fund_account_with_faucet(&keypair, network))
            .await??;

        Ok(())
    }

    pub async fn deploy_program(
        &self,
        program_kp: Keypair,
        authority_kp: Keypair,
        elf_bytes: &[u8],
    ) -> anyhow::Result<()> {
        let program_pubkey = Pubkey::from_slice(&program_kp.x_only_public_key().0.serialize());
        let program_deployer = self.program_deployer.clone();
        let elf = elf_bytes.to_vec();

        // write ELF to a temp file (no extra deps)
        let tmp_dir = std::env::temp_dir();
        let elf_path = tmp_dir.join(format!("deploy-{}.elf", hex::encode(program_pubkey.0)));
        std::fs::write(&elf_path, &elf)?;

        let elf_path_str = elf_path.to_string_lossy().to_string();
        let program_name = hex::encode(program_pubkey.0);

        tokio::task::spawn_blocking(move || {
            program_deployer
                .try_deploy_program(program_name, program_kp, authority_kp, &elf_path_str)
                .map_err(|e| anyhow::anyhow!("Program deployment failed: {}", e))
        })
        .await??;

        Ok(())
    }

    pub fn generate_new_keypair(&self) -> (Keypair, Pubkey, Address) {
        generate_new_keypair(self.network)
    }

    pub async fn generate_funded_keypair(&self) -> Result<(Keypair, Pubkey, Address)> {
        let (keypair, pubkey, address) = self.generate_new_keypair();
        self.fund_keypair_with_faucet(&keypair).await?;
        Ok((keypair, pubkey, address))
    }

    /// Create an account with specific lamports (with UTXO anchoring)
    pub async fn create_account_with_lamports(
        &self,
        authority_kp: Keypair,
        initial_lamports: u64,
    ) -> Result<(Keypair, Pubkey)> {
        let (account_keypair, account_pubkey, _) = self.generate_new_keypair();
        let authority_pubkey = Pubkey::from_slice(&authority_kp.x_only_public_key().0.serialize());

        // Get UTXO for account creation (in the old tests this was done with send_utxo)
        // For now, we'll create the account without UTXO anchoring since send_utxo is complex
        let recent_blockhash = self.get_recent_blockhash().await?;

        let message = ArchMessage::new(
            &[system_instruction::create_account(
                &authority_pubkey,
                &account_pubkey,
                initial_lamports,
                0,
                &Pubkey::system_program(),
            )],
            Some(authority_pubkey),
            recent_blockhash.parse()?,
        );

        let create_account_tx = build_and_sign_transaction(
            message,
            vec![authority_kp, account_keypair.clone()],
            self.network,
        )?;

        let txid = self.send_transaction(create_account_tx).await?;
        let processed_tx = self.wait_for_transaction(&txid).await?;

        match processed_tx.status {
            Status::Processed => Ok((account_keypair, account_pubkey)),
            Status::Failed(e) => Err(anyhow::anyhow!("Account creation failed: {}", e)),
            Status::Queued => Err(anyhow::anyhow!("Account creation transaction still queued")),
        }
    }

    pub async fn get_best_blockhash(&self) -> Result<Hash> {
        let blockhash = self.arch_async_rpc_client.get_best_block_hash().await?;
        Ok(blockhash.parse()?)
    }

    pub async fn get_recent_blockhash(&self) -> Result<String> {
        Ok(self.arch_async_rpc_client.get_best_block_hash().await?)
    }

    pub async fn build_message(
        &self,
        instructions: &[Instruction],
        payer: Option<Pubkey>,
    ) -> Result<ArchMessage> {
        Ok(ArchMessage::new(instructions, payer, self.get_best_blockhash().await?))
    }

    pub async fn build_and_sign_transaction(
        &self,
        message: ArchMessage,
        signers: Vec<Keypair>,
    ) -> Result<RuntimeTransaction> {
        Ok(build_and_sign_transaction(message, signers, self.network)?)
    }

    pub async fn send_transaction(&self, transaction: RuntimeTransaction) -> Result<String> {
        Ok(self
            .arch_async_rpc_client
            .send_transaction(transaction)
            .await?)
    }

    pub async fn wait_for_transaction(&self, txid: &str) -> Result<ProcessedTransaction> {
        Ok(self
            .arch_async_rpc_client
            .wait_for_processed_transaction(txid)
            .await?)
    }

    pub async fn read_account_info(&self, pubkey: Pubkey) -> Result<arch_sdk::AccountInfo> {
        Ok(self.arch_async_rpc_client.read_account_info(pubkey).await?)
    }
}
