use anchor_lang::{InstructionData, ToAccountMetas};
use anyhow::Result;
use solana_client::rpc_client::RpcClient;
use solana_program::instruction::Instruction;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    compute_budget::ComputeBudgetInstruction,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    sysvar,
    transaction::Transaction,
};

pub async fn update_cluster_history(
    rpc_url: &str,
    payer: &Keypair,
) -> Result<()> {
    let client = RpcClient::new_with_commitment(rpc_url, CommitmentConfig::confirmed());

    // Derive cluster history account PDA
    let (cluster_history_account, _) = Pubkey::find_program_address(
        &[b"cluster-history"],
        &validator_history::id(),
    );

    println!("Updating cluster history account: {}", cluster_history_account);

    // Create the copy_cluster_info instruction
    let ix = Instruction {
        program_id: validator_history::id(),
        accounts: validator_history::accounts::CopyClusterInfo {
            cluster_history_account,
            slot_history: sysvar::slot_history::id(),
            signer: payer.pubkey(),
        }
        .to_account_metas(None),
        data: validator_history::instruction::CopyClusterInfo {}.data(),
    };

    // Get recent blockhash
    let blockhash = client.get_latest_blockhash()?;

    // Create and send transaction
    let tx = Transaction::new_signed_with_payer(
        &[
            ComputeBudgetInstruction::request_heap_frame(256 * 1024),
            ComputeBudgetInstruction::set_compute_unit_limit(1_400_000),
            ix,
        ],
        Some(&payer.pubkey()),
        &[payer],
        blockhash,
    );

    let signature = client.send_and_confirm_transaction(&tx)?;
    println!("Successfully updated cluster history. Signature: {}", signature);

    Ok(())
}