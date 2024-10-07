use std::{net::SocketAddr, str::FromStr, sync::Arc};

use clap::Parser;
use solana_program::pubkey::Pubkey;
use solana_rpc_client::nonblocking::rpc_client::RpcClient;
use tracing::{info, instrument};

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
pub struct Args {
    /// Bind address for the server
    #[arg(long, env, default_value_t = SocketAddr::from_str("0.0.0.0:7001").unwrap())]
    pub bind_addr: SocketAddr,

    /// RPC url
    #[arg(long, env, default_value = "https://api.mainnet-beta.solana.com")]
    pub json_rpc_url: String,

    /// Validator history program ID (Pubkey as base58 string)
    #[arg(
        long,
        env,
        default_value = "HistoryJTGbKQD2mRgLZ3XhqHnN811Qpez8X9kCcGHoa"
    )]
    pub validator_history_program_id: Pubkey,
}

#[tokio::main]
#[instrument]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    tracing_subscriber::fmt().init();

    info!("args: {:?}", args);

    info!("starting server at {}", args.bind_addr);

    let rpc_client = RpcClient::new(args.json_rpc_url.clone());
    info!("started rpc client at {}", args.json_rpc_url);

    let state = Arc::new(jito_stakenet_api::router::RouterState {
        validator_history_program_id: args.validator_history_program_id,
        rpc_client,
    });

    let app = jito_stakenet_api::router::get_routes(state);

    axum::Server::bind(&args.bind_addr)
        .serve(app.into_make_service_with_connect_info::<SocketAddr>())
        .await?;

    Ok(())
}
