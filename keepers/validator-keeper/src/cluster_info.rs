use std::sync::Arc;

use anchor_lang::{InstructionData, ToAccountMetas};
use keeper_core::{submit_instructions, SubmitStats, TransactionExecutionError};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{instruction::Instruction, pubkey::Pubkey, signature::Keypair, signer::Signer};
use validator_history::state::ClusterHistory;

pub async fn update_cluster_info(
    client: Arc<RpcClient>,
    keypair: Arc<Keypair>,
    program_id: &Pubkey,
) -> Result<SubmitStats, (TransactionExecutionError, SubmitStats)> {
    let (cluster_history_account, _) =
        Pubkey::find_program_address(&[ClusterHistory::SEED], program_id);

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

    submit_instructions(&client, vec![update_instruction], &keypair).await
}
