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
JSON_RPC_URL=
PROGRAM_ID=HistoryJTGbKQD2mRgLZ3XhqHnN811Qpez8X9kCcGHoa
INTERVAL=300
TIP_DISTRIBUTION_PROGRAM_ID=4R3gSG8BpU4t19KYj8CfnbtRpnT8gtk4dvTHxVRwc2r7
SOLANA_METRICS_CONFIG=
KEYPAIR=/credentials/keypair.json
CLUSTER=mainnet
RUST_LOG="info,solana_gossip=error,solana_metrics=info"
PRIORITY_FEES=20000
```
