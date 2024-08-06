use std::{sync::Arc, time::Duration};

use clap::Parser;
use log::{error, info};
use solana_client::nonblocking::rpc_client::RpcClient;
use stakenet_metrics_service::{
    metrics_emit::{emit_keeper_stats, emit_steward_stats, emit_validator_history_metrics},
    state::{Args, MetricsConfig, MetricsState},
};

#[tokio::main]
async fn main() {
    env_logger::init();
    let args = Args::parse();

    let client = Arc::new(RpcClient::new_with_timeout(
        args.json_rpc_url.clone(),
        Duration::from_secs(60),
    ));

    let metrics_config = MetricsConfig {
        client,
        validator_history_program_id: args.validator_history_program_id,
        tip_distribution_program_id: args.tip_distribution_program_id,
        steward_program_id: args.steward_program_id,
        steward_config: args.steward_config,
        keeper_pubkey: args.keeper_pubkey,
        metrics_interval: Duration::from_secs(args.metrics_interval),
        cluster: args.cluster,
    };

    println!("Steward program {}", args.steward_program_id);

    let mut metrics_state = MetricsState::default();

    loop {
        info!("Fetching accounts and updating state...");
        if let Err(e) = metrics_state.update_state(&metrics_config).await {
            error!("Error fetching new state: {}", e);
            tokio::time::sleep(Duration::from_secs(60)).await;
            continue;
        }

        info!("Emitting metrics...");
        if let Err(e) = emit_validator_history_metrics(&metrics_state) {
            error!("Error emitting validator history metrics: {}", e);
        }

        if let Err(e) = emit_steward_stats(&metrics_state) {
            error!("Error emitting steward stats: {}", e);
        }

        if let Err(e) = emit_keeper_stats(&metrics_state) {
            error!("Error emitting keeper stats metrics: {}", e);
        }

        tokio::time::sleep(metrics_config.metrics_interval).await;
    }
}
