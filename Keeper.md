# Running a Keeper Bot

The keeper bot keeps the

## Raw

## Docker

### Credentials

In the root directory create a new folder named `credentials` and then populate it with a keypair. This is keypair that signs and pays for all transactions.

```bash
mkdir credentials
solana-keygen new -o ./credentials/keypair.json
```

### ENV

```bash
mkdir config
touch ./config/.env
```

```.env
# RPC URL for the cluster
JSON_RPC_URL=https://api.mainnet-beta.solana.com

# Gossip entrypoint in the form of URL:PORT
GOSSIP_ENTRYPOINT=

# Metrics upload config
# For Jito Use Only ( For now )
SOLANA_METRICS_CONFIG=

# Log levels
RUST_LOG="info,solana_gossip=error,solana_metrics=info"

# Path to keypair used to pay for account creation and execute transactions
KEYPAIR=./credentials/keypair.json

# Path to keypair used specifically for submitting permissioned transactions
# For Jito Use Only ( For now )
ORACLE_AUTHORITY_KEYPAIR=

# Validator history program ID (Pubkey as base58 string)
VALIDATOR_HISTORY_PROGRAM_ID=HistoryJTGbKQD2mRgLZ3XhqHnN811Qpez8X9kCcGHoa

# Tip distribution program ID (Pubkey as base58 string)
TIP_DISTRIBUTION_PROGRAM_ID=4R3gSG8BpU4t19KYj8CfnbtRpnT8gtk4dvTHxVRwc2r7

# Steward program ID
STEWARD_PROGRAM_ID=sssh4zkKhX8jXTNQz1xDHyGpygzgu2UhcRcUvZihBjP

# Steward config account
STEWARD_CONFIG=35mMfFNiui7hcHy6xHTz11Q6YukbhH9qQgYR5dhWAQQH

# Interval to update Validator History Accounts (in seconds)
VALIDATOR_HISTORY_INTERVAL=300

# Interval to run steward (in seconds)
STEWARD_INTERVAL=301

# Interval to emit metrics (in seconds)
METRICS_INTERVAL=60

# Priority Fees in microlamports
PRIORITY_FEES=20000

# Cluster to specify (Mainnet, Testnet, Devnet)
CLUSTER=Mainnet

# Run flags (true/false)
RUN_CLUSTER_HISTORY=true
RUN_COPY_VOTE_ACCOUNTS=true
RUN_MEV_COMMISSION=true
RUN_MEV_EARNED=true
RUN_STEWARD=true

# For Jito Use Only ( For now )
RUN_STAKE_UPLOAD=false
RUN_GOSSIP_UPLOAD=false

# Run with the startup flag set to true
FULL_STARTUP=true
```
