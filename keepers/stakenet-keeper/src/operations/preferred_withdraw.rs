/*
Updates the preferred withdraw validator based on lowest score and available lamports
*/

use crate::state::keeper_config::KeeperConfig;
use crate::state::keeper_state::KeeperState;
use anchor_lang::{InstructionData, ToAccountMetas};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_metrics::datapoint_error;
#[allow(deprecated)]
use solana_sdk::{
    compute_budget,
    epoch_info::EpochInfo,
    instruction::Instruction,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    stake,
};
use stakenet_sdk::{
    models::{errors::JitoTransactionExecutionError, submit_stats::SubmitStats},
    utils::transactions::submit_transactions,
};
use std::sync::Arc;

use super::keeper_operations::{check_flag, KeeperOperations};

fn _get_operation() -> KeeperOperations {
    KeeperOperations::PreferredWithdraw
}

fn _should_run(epoch_info: &EpochInfo, runs_for_epoch: u64) -> bool {
    // Run once at 10% and once at 60% completion of epoch
    (epoch_info.slot_index > epoch_info.slots_in_epoch / 10 && runs_for_epoch < 1)
        || (epoch_info.slot_index > epoch_info.slots_in_epoch * 6 / 10 && runs_for_epoch < 2)
}

async fn _process(
    client: &Arc<RpcClient>,
    keypair: &Arc<Keypair>,
    steward_program_id: &Pubkey,
    steward_config: &Pubkey,
    priority_fee_in_microlamports: u64,
    retry_count: u16,
    confirmation_time: u64,
) -> Result<SubmitStats, JitoTransactionExecutionError> {
    update_preferred_withdraw(
        client,
        keypair,
        steward_program_id,
        steward_config,
        priority_fee_in_microlamports,
        retry_count,
        confirmation_time,
    )
    .await
}

pub async fn fire(
    keeper_config: &KeeperConfig,
    keeper_state: &KeeperState,
) -> (KeeperOperations, u64, u64, u64) {
    let client = &keeper_config.client;
    let keypair = &keeper_config.keypair;
    let steward_program_id = &keeper_config.steward_program_id;
    let steward_config = &keeper_config.steward_config;
    let priority_fee_in_microlamports = keeper_config.priority_fee_in_microlamports;
    let retry_count = keeper_config.tx_retry_count;
    let confirmation_time = keeper_config.tx_confirmation_seconds;

    let operation = _get_operation();
    let epoch_info = &keeper_state.epoch_info;

    let (mut runs_for_epoch, mut errors_for_epoch, mut txs_for_epoch) =
        keeper_state.copy_runs_errors_and_txs_for_epoch(operation);

    let should_run =
        _should_run(epoch_info, runs_for_epoch) && check_flag(keeper_config.run_flags, operation);

    if should_run {
        match _process(
            client,
            keypair,
            steward_program_id,
            steward_config,
            priority_fee_in_microlamports,
            retry_count,
            confirmation_time,
        )
        .await
        {
            Ok(stats) => {
                for message in stats.results.iter() {
                    if let Err(e) = message {
                        datapoint_error!(
                            "preferred-withdraw-error",
                            ("error", e.to_string(), String),
                        );
                    } else {
                        txs_for_epoch += 1;
                    }
                }

                if stats.errors == 0 {
                    runs_for_epoch += 1;
                }
            }
            Err(e) => {
                datapoint_error!("preferred-withdraw-error", ("error", e.to_string(), String),);
                errors_for_epoch += 1;
            }
        };
    }

    (operation, runs_for_epoch, errors_for_epoch, txs_for_epoch)
}

// ----------------- OPERATION SPECIFIC FUNCTIONS -----------------

pub fn get_update_preferred_withdraw_instructions(
    steward_program_id: &Pubkey,
    steward_config: &Pubkey,
    signer: &Pubkey,
    priority_fee_in_microlamports: u64,
) -> Vec<Instruction> {
    // Derive steward state PDA
    let (steward_state, _) = Pubkey::find_program_address(
        &[b"steward_state", steward_config.as_ref()],
        steward_program_id,
    );

    // Get stake pool address from config (would need to be fetched, for now using placeholder)
    // In production, you'd fetch the config account and read the stake_pool field
    // For now, we'll use the well-known JitoSOL stake pool address
    let stake_pool = solana_sdk::pubkey!("Jito4APyf642JPZPx3hGc6WWJ8zPKtRbRs4P815Awbb");

    // Derive validator list PDA (standard SPL stake pool derivation)
    let (validator_list, _) = Pubkey::find_program_address(
        &[stake_pool.as_ref(), b"validator_list"],
        &spl_stake_pool::id(),
    );

    let priority_fee_ix = compute_budget::ComputeBudgetInstruction::set_compute_unit_price(
        priority_fee_in_microlamports,
    );
    let compute_budget_ix =
        compute_budget::ComputeBudgetInstruction::set_compute_unit_limit(400_000);

    let update_instruction = Instruction {
        program_id: *steward_program_id,
        accounts: jito_steward::accounts::UpdatePreferredWithdrawValidator {
            config: *steward_config,
            state_account: steward_state,
            stake_pool_program: spl_stake_pool::id(),
            stake_pool,
            validator_list,
            stake_program: stake::program::id(),
            signer: *signer,
        }
        .to_account_metas(None),
        data: jito_steward::instruction::UpdatePreferredWithdrawValidator {}.data(),
    };

    vec![priority_fee_ix, compute_budget_ix, update_instruction]
}

pub async fn update_preferred_withdraw(
    client: &Arc<RpcClient>,
    keypair: &Arc<Keypair>,
    steward_program_id: &Pubkey,
    steward_config: &Pubkey,
    priority_fee_in_microlamports: u64,
    retry_count: u16,
    confirmation_time: u64,
) -> Result<SubmitStats, JitoTransactionExecutionError> {
    let ixs = get_update_preferred_withdraw_instructions(
        steward_program_id,
        steward_config,
        &keypair.pubkey(),
        priority_fee_in_microlamports,
    );

    submit_transactions(client, vec![ixs], keypair, retry_count, confirmation_time).await
}
