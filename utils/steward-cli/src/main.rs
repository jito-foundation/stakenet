use std::{sync::Arc, time::Duration};

use anyhow::Result;
use clap::Parser;
use commands::{
    actions::{
        add_to_blacklist::command_add_to_blacklist,
        auto_add_validator_from_pool::command_auto_add_validator_from_pool,
        auto_remove_validator_from_pool::command_auto_remove_validator_from_pool,
        close_steward::command_close_steward,
        instant_remove_validator::command_instant_remove_validator,
        manually_copy_all_vote_accounts::command_manually_copy_all_vote_accounts,
        manually_copy_vote_accounts::command_manually_copy_vote_account,
        manually_remove_validator::command_manually_remove_validator, pause::command_pause,
        remove_bad_validators::command_remove_bad_validators,
        remove_from_blacklist::command_remove_from_blacklist, reset_state::command_reset_state,
        reset_validator_lamport_balances::command_reset_validator_lamport_balances,
        resume::command_resume, revert_staker::command_revert_staker,
        set_staker::command_set_staker, update_authority::command_update_authority,
        update_config::command_update_config,
        update_priority_fee_config::command_update_priority_fee_config,
        update_validator_list_balance::command_update_validator_list_balance,
    },
    command_args::{Args, Commands},
    cranks::{
        compute_delegations::command_crank_compute_delegations,
        compute_instant_unstake::command_crank_compute_instant_unstake,
        compute_score::command_crank_compute_score,
        epoch_maintenance::command_crank_epoch_maintenance, idle::command_crank_idle,
        rebalance::command_crank_rebalance, steward::command_crank_steward,
    },
    info::{
        diff_backtest::command_diff_backtest,
        export_backtest::command_export_backtest,
        view_backtest::command_view_backtest,
        view_config::command_view_config,
        view_next_index_to_remove::command_view_next_index_to_remove,
        view_priority_fee_config::command_view_priority_fee_config, view_state::command_view_state,
    },
    init::{init_steward::command_init_steward, realloc_state::command_realloc_state},
};
use dotenvy::dotenv;
use solana_client::nonblocking::rpc_client::RpcClient;

pub mod commands;
pub mod utils;

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok(); // Loads in .env file
    
    // Initialize logger to show info! and debug! messages
    env_logger::init();
    let args = Args::parse();
    let client = Arc::new(RpcClient::new_with_timeout(
        args.json_rpc_url.clone(),
        Duration::from_secs(60),
    ));

    let program_id = args.program_id;
    let result = match args.commands {
        // ---- Views ----
        Commands::ViewConfig(args) => command_view_config(args, &client, program_id).await,
        Commands::ViewPriorityFeeConfig(args) => {
            command_view_priority_fee_config(args, &client, program_id).await
        }
        Commands::ViewState(args) => command_view_state(args, &client, program_id).await,
        Commands::ViewNextIndexToRemove(args) => {
            command_view_next_index_to_remove(args, &client, program_id).await
        }
        Commands::ViewBacktest(args) => {
            command_view_backtest(&client, program_id, args.backtest_parameters).await
        }
        Commands::DiffBacktest(args) => {
            command_diff_backtest(args).await
        }
        Commands::ExportBacktest(args) => {
            command_export_backtest(args).await
        }

        // --- Helpers ---
        Commands::ManuallyCopyVoteAccount(args) => {
            command_manually_copy_vote_account(args, &client, program_id).await
        }

        // --- Actions ---
        Commands::CloseSteward(args) => command_close_steward(args, &client, program_id).await,
        Commands::InitSteward(args) => command_init_steward(args, &client, program_id).await,
        Commands::UpdateConfig(args) => command_update_config(args, &client, program_id).await,
        Commands::UpdatePriorityFeeConfig(args) => {
            command_update_priority_fee_config(args, &client, program_id).await
        }
        Commands::UpdateAuthority(args) => {
            command_update_authority(args, &client, program_id).await
        }
        Commands::SetStaker(args) => command_set_staker(args, &client, program_id).await,
        Commands::RevertStaker(args) => command_revert_staker(args, &client, program_id).await,
        Commands::Pause(args) => command_pause(args, &client, program_id).await,
        Commands::Resume(args) => command_resume(args, &client, program_id).await,
        Commands::ReallocState(args) => command_realloc_state(args, &client, program_id).await,
        Commands::ResetState(args) => command_reset_state(args, &client, program_id).await,
        Commands::ResetValidatorLamportBalances(args) => {
            command_reset_validator_lamport_balances(args, &client, program_id).await
        }
        Commands::ManuallyRemoveValidator(args) => {
            command_manually_remove_validator(args, &client, program_id).await
        }
        Commands::ManuallyCopyAllVoteAccounts(args) => {
            command_manually_copy_all_vote_accounts(args, &client, program_id).await
        }
        Commands::InstantRemoveValidator(args) => {
            command_instant_remove_validator(args, &client, program_id).await
        }
        Commands::AutoRemoveValidatorFromPool(args) => {
            command_auto_remove_validator_from_pool(args, &client, program_id).await
        }
        Commands::AutoAddValidatorFromPool(args) => {
            command_auto_add_validator_from_pool(args, &client, program_id).await
        }
        Commands::RemoveBadValidators(args) => {
            command_remove_bad_validators(args, &client, program_id).await
        }
        Commands::AddToBlacklist(args) => command_add_to_blacklist(args, &client, program_id).await,
        Commands::RemoveFromBlacklist(args) => {
            command_remove_from_blacklist(args, &client, program_id).await
        }
        Commands::UpdateValidatorListBalance(args) => {
            command_update_validator_list_balance(&client, args, program_id).await
        }

        // --- Cranks ---
        Commands::CrankSteward(args) => command_crank_steward(args, &client, program_id).await,
        Commands::CrankEpochMaintenance(args) => {
            command_crank_epoch_maintenance(args, &client, program_id).await
        }
        Commands::CrankComputeScore(args) => {
            command_crank_compute_score(args, &client, program_id).await
        }
        Commands::CrankComputeDelegations(args) => {
            command_crank_compute_delegations(args, &client, program_id).await
        }
        Commands::CrankIdle(args) => command_crank_idle(args, &client, program_id).await,
        Commands::CrankComputeInstantUnstake(args) => {
            command_crank_compute_instant_unstake(args, &client, program_id).await
        }
        Commands::CrankRebalance(args) => command_crank_rebalance(args, &client, program_id).await,
    };

    if let Err(e) = result {
        eprintln!("\n❌ Error: \n\n{:?}\n", e);
        std::process::exit(1);
    }

    Ok(())
}
