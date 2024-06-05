use clap::Parser;
use commands::{
    commands::{Args, Commands},
    init_config::command_init_config,
};
use solana_client::rpc_client::RpcClient;
use std::time::Duration;

pub mod commands;

fn main() {
    let args = Args::parse();
    let client = RpcClient::new_with_timeout(args.json_rpc_url.clone(), Duration::from_secs(60));
    match args.commands {
        Commands::InitConfig(args) => command_init_config(args, client),
    };
}
