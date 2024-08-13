# StakeNet Keeper Bot

## Overview

This is a service that permissionlessly maintains the StakeNet Validator History and Steward programs. It ensures that Validator History data is kept up to date, and that the Steward program is properly moving through its regular operations to maintain the targeted stake pool. Operations, program addresses, and transaction submission details, can be configured via environment variables.

## Usage

Prerequisites:

- RPC endpoint that can handle getProgramAccounts calls
- Keypair loaded with some SOL at stakenet/credentials/keypair.json
- .env file with configuration below

### Build and run from source

In `stakenet/` directory:

```
docker compose --env-file .env up -d --build  validator-keeper
```

### Run from Dockerhub

This image is available on Dockerhub at: (TODO)

In `stakenet/` directory:

```
docker pull <repository>/<image-name>:<tag>
docker run -d \
  --name validator-keeper \
  --env-file .env \
  -v $(pwd)/credentials:/credentials \
  --restart on-failure:5 \
  <repository>/<image-name>:<tag>
```

### Env File for third party Keepers

In `stakenet/.env`:

```bash
# RPC URL for the cluster
JSON_RPC_URL="INCLUDE YOUR RPC URL HERE"

# Cluster to specify (mainnet, testnet, devnet)
CLUSTER=mainnet

# Log levels
RUST_LOG="info,solana_gossip=error,solana_metrics=info"

# Path to keypair used to execute tranasactions
KEYPAIR=./credentials/keypair.json

# Validator history program ID (Pubkey as base58 string)
VALIDATOR_HISTORY_PROGRAM_ID=HistoryJTGbKQD2mRgLZ3XhqHnN811Qpez8X9kCcGHoa

# Tip distribution program ID (Pubkey as base58 string)
TIP_DISTRIBUTION_PROGRAM_ID=4R3gSG8BpU4t19KYj8CfnbtRpnT8gtk4dvTHxVRwc2r7

# Steward program ID
STEWARD_PROGRAM_ID=Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8

# Steward config account for JitoSOL
STEWARD_CONFIG=jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv

# Priority Fees in microlamports
PRIORITY_FEES=20000

# Retry count
TX_RETRY_COUNT=100

# Confirmation time after submission
TX_CONFIRMATION_SECONDS=30

# Run flags (true/false)
RUN_CLUSTER_HISTORY=true
RUN_COPY_VOTE_ACCOUNTS=true
RUN_MEV_COMMISSION=true
RUN_MEV_EARNED=true
RUN_STEWARD=true
RUN_EMIT_METRICS=false

# Interval to update Validator History Accounts (in seconds)
VALIDATOR_HISTORY_INTERVAL=300

# Interval to run steward (in seconds)
STEWARD_INTERVAL=301

# Interval to emit metrics (in seconds)
METRICS_INTERVAL=60

# For Oracle Authority Only
RUN_STAKE_UPLOAD=false
RUN_GOSSIP_UPLOAD=false

# Run with the startup flag set to true
FULL_STARTUP=true

# Running with no_pack set to true skips packing the instructions and will cost more
NO_PACK=false

# Pay for new accounts when necessary
PAY_FOR_NEW_ACCOUNTS=false

# Max time in minutes to wait after any fire cycle
COOL_DOWN_RANGE=0

# Metrics upload influx server (optional)
SOLANA_METRICS_CONFIG=""
```

## Program Layout

### Keeper State

The `KeeperState` keeps track of:

- current epoch data
- running tally of all operations successes and failures for the given epoch
- all accounts fetched from the RPC that are needed downstream

Note: All maps are key'd by the `vote_account`

### Initialize

Gather all needed arguments, and initialize the global `KeeperState`.

### Loop

The forever loop consists of three parts: **Fetch**, **Fire** and **Emit**. There is only ever one **Fetch** and **Emit** section, and there can be several **Fire** sections.

The **Fire** sections can run on independent cadences - say we want the Validator History functions to run every 300sec and we want to emit metrics every 60sec.

The **Fetch** section is run _before_ and **Fire** section.
The **Emit** section is _one tick_ after any **Fire** section.

#### Fetch

The **Fetch** section is in charge of three operations:

- Keeping track of the current epoch and resetting the runs and error counts for each operation
- Creating any missing accounts needed for the downstream **Fires** to run
- Fetching and updating all of the needed accounts needed downstream

This is accomplished is by running three functions within the **Fetch** section

- `pre_create_update` - Updates epoch, and fetches all needed accounts that are not dependant on any missing accounts.
- `create_missing_accounts` - Creates the missing accounts, which can be determined by the accounts fetched in the previous step
- `post_create_update` - Fetches any last accounts that needed the missing accounts

Since this is run before every **FIRE** section, some accounts will be fetched that are not needed. This may seem wasteful but the simplicity of having a synchronized global is worth the cost.

Notes:

- The **Fetch** section is the only section that will mutate the `KeeperState`.
- If anything in the **Fetch** section fails, no **Fires** will run

#### Fire

There are several **Fire** sections running at their own cadence. Before any **Fire** section is run, the **Fetch** section will be called.

Each **Fire** is a call to `operations::operation_name::fire` which will fire off the operation and return the new count of runs and errors for that operation to be saved in the `KeeperState`

Notes:

- Each **Fire** is self contained, one should not be dependant on another.
- No \*_Fire_ will fetch any accounts, if there are needs for them, they should be added to the `KeeperState`

#### Emit

This section emits the state of the Keeper one tick after any operation has been called. This is because we want to emit a failure of any **Fetch** operation, which on failure advances the tick.
