use std::{path::PathBuf, sync::Arc};

use anchor_lang::{InstructionData, ToAccountMetas};
use anyhow::anyhow;
use clap::Parser;
use solana_client::nonblocking::rpc_client::RpcClient;
#[allow(deprecated)]
use solana_sdk::{
    compute_budget, instruction::Instruction, signature::read_keypair_file, signer::Signer,
    transaction::Transaction,
};
use stakenet_sdk::utils::accounts::get_cluster_history_address;

#[derive(Parser)]
#[command(about = "Crank to copy cluster info data to cluster history accounts")]
pub struct CrankCopyClusterInfo {
    /// Path to keypair for transaction signing
    #[arg(short, long, env, default_value = "~/.config/solana/id.json")]
    keypair_path: PathBuf,
}

pub async fn run(args: CrankCopyClusterInfo, rpc_url: String) -> anyhow::Result<()> {
    let keypair = read_keypair_file(args.keypair_path)
        .map_err(|e| anyhow!("Failed reading keypair file: {e}"))?;
    let keypair = Arc::new(keypair);
    let client = RpcClient::new(rpc_url);
    let client = Arc::new(client);

    let cluster_history_address = get_cluster_history_address(&validator_history::id());
    let priority_fee_ix = compute_budget::ComputeBudgetInstruction::set_compute_unit_price(20000);
    let heap_request_ix = compute_budget::ComputeBudgetInstruction::request_heap_frame(256 * 1024);
    let compute_budget_ix =
        compute_budget::ComputeBudgetInstruction::set_compute_unit_limit(1_400_000);
    let update_instruction = Instruction {
        program_id: validator_history::id(),
        accounts: validator_history::accounts::CopyClusterInfo {
            cluster_history_account: cluster_history_address,
            slot_history: solana_program::sysvar::slot_history::id(),
            signer: keypair.pubkey(),
        }
        .to_account_metas(None),
        data: validator_history::instruction::CopyClusterInfo {}.data(),
    };

    let hash = client
        .get_latest_blockhash()
        .await
        .expect("Failed to fetch latest blockhash");
    let transaction = Transaction::new_signed_with_payer(
        &[
            priority_fee_ix,
            heap_request_ix,
            compute_budget_ix,
            update_instruction,
        ],
        Some(&keypair.pubkey()),
        &[keypair],
        hash,
    );
    client
        .send_and_confirm_transaction_with_spinner(&transaction)
        .await?;

    Ok(())
}
