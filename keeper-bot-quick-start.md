# Keeper Bot Quick-start

Below are the steps to configuring and running the Stakenet Keeper Bot. We recommend running it as a docker container.

## Setup

### Credentials

In the root directory create a new folder named `credentials` and then populate it with a keypair. This is keypair that signs and pays for all transactions.

```bash
mkdir credentials
solana-keygen new -o ./credentials/keypair.json
```

### ENV

In the root directory create a new folder named `config` and create a `.env` file inside of it

```bash
mkdir config
touch ./config/.env
```

Then copy into the `.env` file the contents below. Everything should be set as-is, however consider replacing the `JSON_RPC_URL` and adjusting the `PRIORITY_FEES`

```.env
# RPC URL for the cluster
JSON_RPC_URL=https://api.mainnet-beta.solana.com

# Cluster to specify (mainnet, testnet, devnet)
CLUSTER=mainnet

# Log levels
RUST_LOG="info,solana_gossip=error,solana_metrics=info"

# Path to keypair used to pay for account creation and execute transactions
KEYPAIR=./credentials/keypair.json

# Validator history program ID (Pubkey as base58 string)
VALIDATOR_HISTORY_PROGRAM_ID=HistoryJTGbKQD2mRgLZ3XhqHnN811Qpez8X9kCcGHoa

# Tip distribution program ID (Pubkey as base58 string)
TIP_DISTRIBUTION_PROGRAM_ID=4R3gSG8BpU4t19KYj8CfnbtRpnT8gtk4dvTHxVRwc2r7

# Steward program ID
STEWARD_PROGRAM_ID=Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8

# Steward config account
STEWARD_CONFIG=jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv

# Priority Fees in microlamports
PRIORITY_FEES=20000

# Run flags (true/false)
RUN_CLUSTER_HISTORY=true
RUN_COPY_VOTE_ACCOUNTS=true
RUN_MEV_COMMISSION=true
RUN_MEV_EARNED=true
RUN_STEWARD=true
RUN_EMIT_METRICS=true

################# DEBUGGING AND JITO USE ONLY #################

# Interval to update Validator History Accounts (in seconds)
# VALIDATOR_HISTORY_INTERVAL=300

# Interval to run steward (in seconds)
# STEWARD_INTERVAL=301

# Interval to emit metrics (in seconds)
# METRICS_INTERVAL=60

# For Jito Use Only ( For now )
# RUN_STAKE_UPLOAD=false
# RUN_GOSSIP_UPLOAD=false

# Run with the startup flag set to true
# FULL_STARTUP=true

# Running with no_pack set to true skips packing the instructions and will cost more
# NO_PACK=false

# Pay for new accounts when necessary
# PAY_FOR_NEW_ACCOUNTS=false

# Max time in minutes to wait after any fire cycle
# COOL_DOWN_RANGE=20

# Gossip entrypoint in the form of URL:PORT
# GOSSIP_ENTRYPOINT=

# Metrics upload config
# For Jito Use Only ( For now )
# SOLANA_METRICS_CONFIG=

# Path to keypair used specifically for submitting permissioned transactions
# For Jito Use Only ( For now )
# ORACLE_AUTHORITY_KEYPAIR=
```

## Running Docker

Once the setup is complete use the following commands to run/manage the docker container:

> Note: We are running `Docker version 24.0.5, build ced0996`

### Start Docker

```bash
docker compose --env-file config/.env up -d --build  validator-keeper --remove-orphans
```

### Stop Docker**

```bash
docker stop validator-keeper; docker rm validator-keeper;
```

### View Logs

```bash
docker logs validator-keeper -f
```

## Running Raw

To run the keeper in terminal, build for release and run the program.

### Build for Release

```bash
cargo build --release --bin validator-keeper
```

### Run Keeper

```bash
RUST_LOG=info cargo run --bin validator-keeper --
```

To see all available parameters run:

```bash
RUST_LOG=info cargo run --bin validator-keeper -- -h
```
