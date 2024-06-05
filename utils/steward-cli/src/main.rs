use clap::Parser;
use commands::{
    commands::{Args, Commands},
    init_config::command_init_config,
    init_state::command_init_state,
    update_config::command_update_config,
    view_config::command_view_config,
    view_state::command_view_state,
};
use dotenv::dotenv;
use solana_client::rpc_client::RpcClient;
use std::time::Duration;

pub mod commands;

fn main() {
    dotenv().ok(); // Loads in .env file
    let args = Args::parse();
    let client = RpcClient::new_with_timeout(args.json_rpc_url.clone(), Duration::from_secs(60));
    let program_id = args.program_id;
    match args.commands {
        Commands::InitConfig(args) => command_init_config(args, client, program_id),
        Commands::UpdateConfig(args) => command_update_config(args, client, program_id),
        Commands::ViewConfig(args) => command_view_config(args, client, program_id),
        Commands::InitState(args) => command_init_state(args, client, program_id),
        Commands::ViewState(args) => command_view_state(args, client, program_id),
    };
}
