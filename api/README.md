# StakeNet API

## Overview

The StakeNet API provides access to historical validator performance data on the Solana blockchain. This API can be useful for any website or application that needs to show validator performance history, specific epoch information, or the latest validator data.

## Getting started

### Prerequisites

- [Solana's RPC Client](https://docs.rs/solana-rpc-client/latest/solana_rpc_client/)
- [Axum](https://docs.rs/axum/latest/axum/)

### Build for release

To build the API for release, run the following command:

```bash
cargo b --release --bin jito-stakenet-api
```

### Check available options

To view the options available for configuring the API:

```bash
./target/release/jito-stakenet-api --help

# Usage: jito-stakenet-api [OPTIONS]
# 
# Options:
#       --bind-addr <BIND_ADDR>
#           Bind address for the server [env: BIND_ADDR=] [default: 0.0.0.0:7001]
#       --json-rpc-url <JSON_RPC_URL>
#           RPC url [env: JSON_RPC_URL=] [default: https://api.mainnet-beta.solana.com]
#       --validator-history-program-id <VALIDATOR_HISTORY_PROGRAM_ID>
#           Validator history program ID (Pubkey as base58 string) [env: VALIDATOR_HISTORY_PROGRAM_ID=] [default: HistoryJTGbKQD2mRgLZ3XhqHnN811Qpez8X9kCcGHoa]
#   -h, --help
#           Print help
#   -V, --version
#           Print version
```

### Running the API

Once built, run the API using the following command:

```bash
./target/release/jito-stakenet-api
```

You can now send requests to http://localhost:7001 (or whichever address/port you specify in --bind-addr).

## API Endpoints

You can check `vote_account` [here](https://www.jito.network/stakenet/history/).

|HTTP Method|Endpoint                                               |Description                                        |
|-----------|-------------------------------------------------------|---------------------------------------------------|
|GET        |/api/v1/validator_history/{vote_account}               |Fetch all validator histories by vote account      |
|GET        |/api/v1/validator_history/{vote_account}?epoch={epoch} |Fetch specific epoch history by vote account       |
|GET        |/api/v1/validator_history/{vote_account}/latest        |Fetch the latest validator history by vote account |


### Example Requests

#### Get all histories by vote_account:

```
curl http://localhost:7001/api/v1/validator_history/{vote_account}
```

#### Get specific epoch history by vote_account:

```
curl http://localhost:7001/api/v1/validator_history/{vote_account}?epoch=500
```

#### Get latest history by vote_account:

```
curl http://localhost:7001/api/v1/validator_history/{vote_account}?epoch=500
```

## Tips for Developers

### Add a New Route

If you want to add a new route to the router, you do it in `api/src/router.rs`:

```rust
// api/src/router.rs

let validator_history_routes = Router::new()
    .route(
        "/:vote_account",
        get(get_all_validator_histories::get_all_validator_histories),
    )
    .route(
        "/:vote_account/latest",
        get(get_latest_validator_history::get_latest_validator_history),
    )
    .route(
        "/:vote_account/new_route",
        get(new_method),
    );
```

### Caching Validator History

You can implement a caching layer for the validator histories using the [Moka](https://docs.rs/moka/latest/moka/index.html) library. Here's an example of adding caching to the server.

#### Step 1: Add Moka Dependency

```bash
cargo add moka --features future
```

#### Step 2: Update State to Include Cache

```rust
// api/src/router.rs

pub struct RouterState {
    pub validator_history_program_id: Pubkey,
    pub rpc_client: RpcClient,

    // add cache
    pub cache: Cache<Pubkey, ValidatorHistoryResponse>,
}
```

#### Step 3: Modify Main To Use Cache

```rust
// api/src/bin/main.rs

#[tokio::main]
#[instrument]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    tracing_subscriber::fmt().init();

    info!("args: {:?}", args);

    info!("starting server at {}", args.bind_addr);

    let rpc_client = RpcClient::new(args.json_rpc_url.clone());
    info!("started rpc client at {}", args.json_rpc_url);

    // Create a cache that can store up to u64::MAX entries.
    let cache: Cache<Pubkey, ValidatorHistoryResponse> = Cache::new(u64::MAX);

    let state = Arc::new(jito_stakenet_api::router::RouterState {
        validator_history_program_id: args.validator_history_program_id,
        rpc_client,
        cache,
    });

    let app = jito_stakenet_api::router::get_routes(state);

    axum::Server::bind(&args.bind_addr)
        .serve(app.into_make_service_with_connect_info::<SocketAddr>())
        .await?;

    Ok(())
}
```

#### Step 4: Use Cache in Handlers

```rust
// api/src/router/get_all_validator_histories.rs

pub(crate) async fn get_all_validator_histories(
    State(state): State<Arc<RouterState>>,
    Path(vote_account): Path<String>,
    Query(epoch_query): Query<EpochQuery>,
) -> crate::Result<impl IntoResponse> {
    let vote_account = Pubkey::from_str(&vote_account)?;
    let history_account =
        get_validator_history_address(&vote_account, &state.validator_history_program_id);

    // Check history_account's pubkey key in cache
    match state.cache.get(&history_account).await {
        Some(history) => Ok(Json(history)),
        None => {
            let account = state.rpc_client.get_account(&history_account).await?;
            let validator_history = ValidatorHistory::try_deserialize(&mut account.data.as_slice())
                .map_err(|e| {
                    warn!("error deserializing ValidatorHistory: {:?}", e);
                    ApiError::ValidatorHistoryError("Error parsing ValidatorHistory".to_string())
                })?;

            let history_entries: Vec<ValidatorHistoryEntryResponse> = match epoch_query.epoch {
                Some(epoch) => validator_history
                    .history
                    .arr
                    .iter()
                    .filter_map(|entry| {
                        if epoch == entry.epoch {
                            Some(ValidatorHistoryEntryResponse::from_validator_history_entry(
                                entry,
                            ))
                        } else {
                            None
                        }
                    })
                    .collect(),
                None => validator_history
                    .history
                    .arr
                    .iter()
                    .map(ValidatorHistoryEntryResponse::from_validator_history_entry)
                    .collect(),
            };

            let history = ValidatorHistoryResponse::from_validator_history(
                validator_history,
                history_entries,
            );

            // Insert new history in cache
            state.cache.insert(history_account, history.clone()).await;

            Ok(Json(history))
        }
    }
}
```
