use anyhow::Result;
use clap::Parser;
use commands::{
    actions::{
        auto_remove_validator_from_pool::command_auto_remove_validator_from_pool,
        remove_bad_validators::command_remove_bad_validators, reset_state::command_reset_state,
        update_config::command_update_config,
    },
    commands::{Args, Commands},
    cranks::{
        compute_delegations::command_crank_compute_delegations,
        compute_instant_unstake::command_crank_compute_instant_unstake,
        compute_score::command_crank_compute_score,
        epoch_maintenance::command_crank_epoch_maintenance, idle::command_crank_idle,
        rebalance::command_crank_rebalance,
    },
    info::{view_config::command_view_config, view_state::command_view_state},
    init::{init_config::command_init_config, init_state::command_init_state},
};
use dotenv::dotenv;
use solana_client::nonblocking::rpc_client::RpcClient;
use std::time::Duration;

pub mod commands;
pub mod utils;

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok(); // Loads in .env file
    let args = Args::parse();
    let client = RpcClient::new_with_timeout(args.json_rpc_url.clone(), Duration::from_secs(60));
    let program_id = args.program_id;
    let _ = match args.commands {
        Commands::InitConfig(args) => command_init_config(args, client, program_id).await,
        Commands::UpdateConfig(args) => command_update_config(args, client, program_id).await,
        Commands::ViewConfig(args) => command_view_config(args, client, program_id).await,
        Commands::InitState(args) => command_init_state(args, client, program_id).await,
        Commands::ViewState(args) => command_view_state(args, client, program_id).await,
        Commands::ResetState(args) => command_reset_state(args, client, program_id).await,
        Commands::AutoRemoveValidatorFromPool(args) => {
            command_auto_remove_validator_from_pool(args, client, program_id).await
        }
        Commands::RemoveBadValidators(args) => {
            command_remove_bad_validators(args, client, program_id).await
        }
        Commands::CrankEpochMaintenance(args) => {
            command_crank_epoch_maintenance(args, client, program_id).await
        }
        Commands::CrankComputeScore(args) => {
            command_crank_compute_score(args, client, program_id).await
        }
        Commands::CrankComputeDelegations(args) => {
            command_crank_compute_delegations(args, client, program_id).await
        }
        Commands::CrankIdle(args) => command_crank_idle(args, client, program_id).await,
        Commands::CrankComputeInstantUnstake(args) => {
            command_crank_compute_instant_unstake(args, client, program_id).await
        }
        Commands::CrankRebalance(args) => command_crank_rebalance(args, client, program_id).await,
    };

    Ok(())
}
