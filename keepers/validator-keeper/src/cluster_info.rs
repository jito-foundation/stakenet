use std::sync::Arc;

use anchor_lang::{InstructionData, ToAccountMetas};
use keeper_core::{submit_transactions, SubmitStats, TransactionExecutionError};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    compute_budget, instruction::Instruction, pubkey::Pubkey, signature::Keypair, signer::Signer,
};
use validator_history::state::ClusterHistory;

use crate::{derive_cluster_history_address, PRIORITY_FEE};

pub fn get_update_cluster_info_instructions(
    program_id: &Pubkey,
    keypair: &Pubkey,
) -> Vec<Instruction> {
    let cluster_history_account = derive_cluster_history_address(program_id);

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
            signer: keypair.clone(),
        }
        .to_account_metas(None),
        data: validator_history::instruction::CopyClusterInfo {}.data(),
    };

    vec![
        priority_fee_ix,
        heap_request_ix,
        compute_budget_ix,
        update_instruction,
    ]
}

pub async fn update_cluster_info(
    client: &Arc<RpcClient>,
    keypair: &Arc<Keypair>,
    program_id: &Pubkey,
) -> Result<SubmitStats, TransactionExecutionError> {

    let ixs = get_update_cluster_info_instructions(program_id, &keypair.pubkey());

    //TODO why not use submit_instructions?
    submit_transactions(
        client,
        vec![ixs],
        keypair,
    )
    .await
}
