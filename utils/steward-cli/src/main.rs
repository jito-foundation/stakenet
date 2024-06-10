use anyhow::Result;
use clap::Parser;
use commands::{
    commands::{Args, Commands},
    cranks::{
        compute_delegations::command_crank_compute_delegations,
        compute_instant_unstake::command_crank_compute_instant_unstake,
        compute_score::command_crank_compute_score, idle::command_crank_idle,
        rebalance::command_crank_rebalance,
    },
    init_config::command_init_config,
    init_state::command_init_state,
    update_config::command_update_config,
    view_config::command_view_config,
    view_state::command_view_state,
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
    let client = RpcClient::new_with_timeout(args.json_rpc_url.clone(), Duration::from_secs(60));
    let program_id = args.program_id;
    let _ = match args.commands {
        Commands::InitConfig(args) => command_init_config(args, client, program_id).await,
        Commands::UpdateConfig(args) => command_update_config(args, client, program_id).await,
        Commands::ViewConfig(args) => command_view_config(args, client, program_id).await,
        Commands::InitState(args) => command_init_state(args, client, program_id).await,
        Commands::ViewState(args) => command_view_state(args, client, program_id).await,
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
