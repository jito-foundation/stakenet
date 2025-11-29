use std::{path::PathBuf, sync::Arc};

use anchor_lang::{InstructionData, ToAccountMetas};
use anyhow::anyhow;
use clap::Parser;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{instruction::Instruction, signature::read_keypair_file, signer::Signer};
use stakenet_sdk::utils::{
    accounts::get_cluster_history_address, transactions::submit_instructions,
};

#[derive(Parser)]
#[command(about = "Crank to copy cluster info data to validator history accounts")]
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

    let cluster_history_account = get_cluster_history_address(&validator_history::id());

    let update_instruction = Instruction {
        program_id: validator_history::id(),
        accounts: validator_history::accounts::CopyClusterInfo {
            cluster_history_account,
            slot_history: solana_program::sysvar::slot_history::id(),
            signer: keypair.pubkey(),
        }
        .to_account_metas(None),
        data: validator_history::instruction::CopyClusterInfo {}.data(),
    };

    let submit_result = submit_instructions(
        &client,
        vec![update_instruction],
        &keypair,
        0,
        50,
        0,
        Some(300_000),
        false,
    )
    .await;

    println!("Submit Result: {submit_result:?}");

    Ok(())
}
