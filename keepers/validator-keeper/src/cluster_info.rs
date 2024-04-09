use std::sync::Arc;

use anchor_lang::{InstructionData, ToAccountMetas};
use keeper_core::{submit_transactions, SubmitStats, TransactionExecutionError};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    compute_budget, instruction::Instruction, pubkey::Pubkey, signature::Keypair, signer::Signer,
};
use validator_history::state::ClusterHistory;

use crate::PRIORITY_FEE;

pub async fn update_cluster_info(
    client: Arc<RpcClient>,
    keypair: Arc<Keypair>,
    program_id: &Pubkey,
) -> Result<SubmitStats, TransactionExecutionError> {
    let (cluster_history_account, _) =
        Pubkey::find_program_address(&[ClusterHistory::SEED], program_id);

    let priority_fee_ix =
        compute_budget::ComputeBudgetInstruction::set_compute_unit_price(PRIORITY_FEE);
    let heap_request_ix = compute_budget::ComputeBudgetInstruction::request_heap_frame(256 * 1024);
    let compute_budget_ix =
        compute_budget::ComputeBudgetInstruction::set_compute_unit_limit(1_400_000);
    let update_instruction = Instruction {
        program_id: *program_id,
        accounts: validator_history::accounts::CopyClusterInfo {
            cluster_history_account,
            slot_history: solana_program::sysvar::slot_history::id(),
            signer: keypair.pubkey(),
        }
        .to_account_metas(None),
        data: validator_history::instruction::CopyClusterInfo {}.data(),
    };

    submit_transactions(
        &client,
        vec![vec![
            priority_fee_ix,
            heap_request_ix,
            compute_budget_ix,
            update_instruction,
        ]],
        &keypair,
    )
    .await
}
