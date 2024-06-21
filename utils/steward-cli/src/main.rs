use anyhow::Result;
use clap::Parser;
use commands::{
    actions::{
        auto_add_validator_from_pool::command_auto_add_validator_from_pool,
        auto_remove_validator_from_pool::command_auto_remove_validator_from_pool,
        manually_copy_vote_accounts::command_manually_copy_vote_account,
        manually_remove_validator::command_manually_remove_validator,
        remove_bad_validators::command_remove_bad_validators, reset_state::command_reset_state,
        update_config::command_update_config,
    },
    command_args::{Args, Commands},
    cranks::{
        compute_delegations::command_crank_compute_delegations,
        compute_instant_unstake::command_crank_compute_instant_unstake,
        compute_score::command_crank_compute_score,
        epoch_maintenance::command_crank_epoch_maintenance, idle::command_crank_idle,
        rebalance::command_crank_rebalance,
    },
    info::{
        view_config::command_view_config,
        view_next_index_to_remove::command_view_next_index_to_remove,
        view_state::command_view_state,
    },
    init::{init_config::command_init_config, init_state::command_init_state},
    monkey::crank::command_crank_monkey,
};
use dotenv::dotenv;
use solana_client::nonblocking::rpc_client::RpcClient;
use std::{sync::Arc, time::Duration};

pub mod commands;
pub mod utils;

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok(); // Loads in .env file
    let args = Args::parse();
    let client = Arc::new(RpcClient::new_with_timeout(
        args.json_rpc_url.clone(),
        Duration::from_secs(60),
    ));

    let program_id = args.program_id;
    let result = match args.commands {
        // ---- Views ----
        Commands::ViewConfig(args) => command_view_config(args, &client, program_id).await,
        Commands::ViewState(args) => command_view_state(args, &client, program_id).await,
        Commands::ViewNextIndexToRemove(args) => {
            command_view_next_index_to_remove(args, &client, program_id).await
        }

        // --- Actions ---
        Commands::InitConfig(args) => command_init_config(args, &client, program_id).await,
        Commands::UpdateConfig(args) => command_update_config(args, &client, program_id).await,
        Commands::ManuallyCopyVoteAccount(args) => {
            command_manually_copy_vote_account(args, &client, program_id).await
        }
        Commands::InitState(args) => command_init_state(args, &client, program_id).await,
        Commands::ResetState(args) => command_reset_state(args, &client, program_id).await,
        Commands::ManuallyRemoveValidator(args) => {
            command_manually_remove_validator(args, &client, program_id).await
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

        // --- Cranks ---
        Commands::CrankMonkey(args) => command_crank_monkey(args, &client, program_id).await,
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

    match result {
        Ok(_) => {
            println!("\n✅ DONE\n");
        }
        Err(e) => {
            eprintln!("\n❌ Error: \n\n{:?}\n", e);
            std::process::exit(1);
        }
    }

    Ok(())
}
