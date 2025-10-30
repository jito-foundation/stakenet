use std::{sync::Arc, time::Duration};

use anyhow::Result;
use clap::Parser;
use commands::{
    actions::{
        init_directed_stake_meta::command_init_directed_stake_meta,
        init_directed_stake_ticket::command_init_directed_stake_ticket,
        init_directed_stake_whitelist::command_init_directed_stake_whitelist,
    },
    command_args::{Args, Commands},
    compute_directed_stake_meta::command_compute_directed_stake_meta,
    info::{
        view_directed_stake_meta::command_view_directed_stake_meta,
        view_directed_stake_tickets::command_view_directed_stake_tickets,
        view_directed_stake_whitelist::command_view_directed_stake_whitelist,
    },
};
use dotenvy::dotenv;
use solana_client::nonblocking::rpc_client::RpcClient;

use crate::commands::actions::update_directed_stake_ticket::command_update_directed_stake_ticket;

pub mod cli_signer;
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
    let _global_signer = args.signer.as_deref();
    let result = match args.commands {
        Commands::ViewDirectedStakeTickets(args) => {
            command_view_directed_stake_tickets(args, &client, program_id).await
        }
        Commands::ViewDirectedStakeWhitelist(args) => {
            command_view_directed_stake_whitelist(args, &client, program_id).await
        }
        Commands::ViewDirectedStakeMeta(args) => {
            command_view_directed_stake_meta(args, &client, program_id).await
        }
        Commands::ComputeDirectedStakeMeta(args) => {
            command_compute_directed_stake_meta(args, &client, program_id).await
        }
        Commands::InitDirectedStakeMeta(args) => {
            command_init_directed_stake_meta(args, &client, program_id).await
        }
        Commands::InitDirectedStakeWhitelist(args) => {
            command_init_directed_stake_whitelist(args, &client, program_id).await
        }
        Commands::InitDirectedStakeTicket(args) => {
            command_init_directed_stake_ticket(args, &client, program_id).await
        }
        Commands::UpdateDirectedStakeTicket(args) => {
            command_update_directed_stake_ticket(args, client, program_id).await
        }
    };

    if let Err(e) = result {
        eprintln!("\n❌ Error: \n\n{:?}\n", e);
        std::process::exit(1);
    }

    Ok(())
}
