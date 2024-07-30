
# Accounts

**Program**
`sssh4zkKhX8jXTNQz1xDHyGpygzgu2UhcRcUvZihBjP`

**Authority**
`aaaDerwdMyzNkoX1aSoTi3UtFe2W45vh5wCgQNhsjF8`

**Steward Config**
`35mMfFNiui7hcHy6xHTz11Q6YukbhH9qQgYR5dhWAQQH`

**Stake Pool**
`3DuPtyTAKrxKfHkSPZ5fqCayMcGru1BarAKKTfGDeo2j`

**Staker**
`4m64H5TbwAGtZVnxaGAVoTSwjZGV8BCLKRPr8agKQv4Z`

**State**
`Hmctj1WrZnBx3cmJ8njeid6zKMGT8XHp8C6UkojSF72C`

# Initial Commands

## Create Steward

```bash
cargo run -- --program-id sssh4zkKhX8jXTNQz1xDHyGpygzgu2UhcRcUvZihBjP init-steward \
  --authority-keypair-path ../../credentials/stakenet_test.json \
  --steward-config-keypair-path ../../credentials/steward_config.json \
  --stake-pool 3DuPtyTAKrxKfHkSPZ5fqCayMcGru1BarAKKTfGDeo2j \
  --mev-commission-range 10 \
  --epoch-credits-range 30 \
  --commission-range 30 \
  --mev-commission-bps-threshold 1000 \
  --commission-threshold 5 \
  --historical-commission-threshold 50 \
  --scoring-delinquency-threshold-ratio 0.85 \
  --instant-unstake-delinquency-threshold-ratio 0.70 \
  --num-delegation-validators 200 \
  --scoring-unstake-cap-bps 750 \
  --instant-unstake-cap-bps 1000 \
  --stake-deposit-unstake-cap-bps 1000 \
  --compute-score-slot-range 50000 \
  --instant-unstake-epoch-progress 0.50 \
  --instant-unstake-inputs-epoch-progress 0.50 \
  --num-epochs-between-scoring 3 \
  --minimum-stake-lamports 100000000000 \
  --minimum-voting-epochs 5
```

## Create State

```bash
cargo run -- --program-id sssh4zkKhX8jXTNQz1xDHyGpygzgu2UhcRcUvZihBjP realloc-state --authority-keypair-path ../../credentials/stakenet_test.json --steward-config 35mMfFNiui7hcHy6xHTz11Q6YukbhH9qQgYR5dhWAQQH
```

## Update Config

```bash
cargo run -- --program-id sssh4zkKhX8jXTNQz1xDHyGpygzgu2UhcRcUvZihBjP update-config \
  --authority-keypair-path ../../credentials/stakenet_test.json \
  --steward-config 35mMfFNiui7hcHy6xHTz11Q6YukbhH9qQgYR5dhWAQQH \
  --num-epochs-between-scoring 3
```

## Update Authority

`blacklist` | `admin` | `parameters`

```bash
cargo run -- --program-id sssh4zkKhX8jXTNQz1xDHyGpygzgu2UhcRcUvZihBjP update-authority blacklist \
  --authority-keypair-path ../../credentials/stakenet_test.json \
  --steward-config 35mMfFNiui7hcHy6xHTz11Q6YukbhH9qQgYR5dhWAQQH \
  --new-authority aaaDerwdMyzNkoX1aSoTi3UtFe2W45vh5wCgQNhsjF8
```

## Set Staker

```bash
cargo run -- --program-id sssh4zkKhX8jXTNQz1xDHyGpygzgu2UhcRcUvZihBjP set-staker \
  --authority-keypair-path ../../credentials/stakenet_test.json \
  --steward-config 35mMfFNiui7hcHy6xHTz11Q6YukbhH9qQgYR5dhWAQQH
```

## Revert Staker

```bash
cargo run -- --program-id sssh4zkKhX8jXTNQz1xDHyGpygzgu2UhcRcUvZihBjP revert-staker \
  --authority-keypair-path ../../credentials/stakenet_test.json \
  --steward-config 35mMfFNiui7hcHy6xHTz11Q6YukbhH9qQgYR5dhWAQQH
```

## Pause

```bash
cargo run -- --program-id sssh4zkKhX8jXTNQz1xDHyGpygzgu2UhcRcUvZihBjP pause \
  --authority-keypair-path ../../credentials/stakenet_test.json \
  --steward-config 35mMfFNiui7hcHy6xHTz11Q6YukbhH9qQgYR5dhWAQQH --print-tx
```

## Resume

```bash
cargo run -- --program-id sssh4zkKhX8jXTNQz1xDHyGpygzgu2UhcRcUvZihBjP resume \
  --authority-keypair-path ../../credentials/stakenet_test.json \
  --steward-config 35mMfFNiui7hcHy6xHTz11Q6YukbhH9qQgYR5dhWAQQH --print-tx
```

## Reset State

```bash
cargo run -- --program-id sssh4zkKhX8jXTNQz1xDHyGpygzgu2UhcRcUvZihBjP reset-state --steward-config 35mMfFNiui7hcHy6xHTz11Q6YukbhH9qQgYR5dhWAQQH  --authority-keypair-path ../../credentials/stakenet_test.json
```

## View Config

```bash
cargo run -- --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 view-config --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv
```

## View State

```bash
cargo run -- --json-rpc-url $(solana config get | grep "RPC URL" | awk '{print $3}') view-state --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv
```

## View State Per Validator

```bash
cargo run -- --program-id sssh4zkKhX8jXTNQz1xDHyGpygzgu2UhcRcUvZihBjP --json-rpc-url $(solana config get | grep "RPC URL" | awk '{print $3}') view-state --verbose --steward-config 35mMfFNiui7hcHy6xHTz11Q6YukbhH9qQgYR5dhWAQQH
```

## View Next Index To Remove

```bash
cargo run -- --program-id sssh4zkKhX8jXTNQz1xDHyGpygzgu2UhcRcUvZihBjP view-next-index-to-remove --steward-config 35mMfFNiui7hcHy6xHTz11Q6YukbhH9qQgYR5dhWAQQH
```

## Add To Blacklist

```bash
cargo run -- --program-id sssh4zkKhX8jXTNQz1xDHyGpygzgu2UhcRcUvZihBjP add-to-blacklist --steward-config 35mMfFNiui7hcHy6xHTz11Q6YukbhH9qQgYR5dhWAQQH --authority-keypair-path ../../credentials/stakenet_test.json --validator-history-index-to-blacklist 2168
```

## Remove From Blacklist

```bash
cargo run -- --program-id sssh4zkKhX8jXTNQz1xDHyGpygzgu2UhcRcUvZihBjP remove-from-blacklist --steward-config 35mMfFNiui7hcHy6xHTz11Q6YukbhH9qQgYR5dhWAQQH --authority-keypair-path ../../credentials/stakenet_test.json --validator-history-index-to-deblacklist 2168
```

## Auto Remove Validator

```bash
cargo run -- --program-id sssh4zkKhX8jXTNQz1xDHyGpygzgu2UhcRcUvZihBjP auto-remove-validator-from-pool --steward-config 35mMfFNiui7hcHy6xHTz11Q6YukbhH9qQgYR5dhWAQQH --payer-keypair-path ../../credentials/stakenet_test.json --validator-index-to-remove 1397
```

## Auto Add Validator

```bash
cargo run -- --program-id sssh4zkKhX8jXTNQz1xDHyGpygzgu2UhcRcUvZihBjP auto-add-validator-from-pool --steward-config 35mMfFNiui7hcHy6xHTz11Q6YukbhH9qQgYR5dhWAQQH --payer-keypair-path ../../credentials/stakenet_test.json --vote-account 4m64H5TbwAGtZVnxaGAVoTSwjZGV8BCLKRPr8agKQv4Z 
```

## Manually Update Vote Account

```bash
cargo run -- --program-id sssh4zkKhX8jXTNQz1xDHyGpygzgu2UhcRcUvZihBjP manually-copy-vote-account --steward-config 35mMfFNiui7hcHy6xHTz11Q6YukbhH9qQgYR5dhWAQQH --payer-keypair-path ../../credentials/stakenet_test.json --validator-index-to-update 1
```

## Manually Update All Vote Accounts

```bash
cargo run -- --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 manually-copy-all-vote-accounts --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --payer-keypair-path ../../credentials/stakenet_test.json --priority-fee 300000
```

## Manually Remove Validator

```bash
cargo run -- --program-id sssh4zkKhX8jXTNQz1xDHyGpygzgu2UhcRcUvZihBjP manually-remove-validator  --steward-config 35mMfFNiui7hcHy6xHTz11Q6YukbhH9qQgYR5dhWAQQH --authority-keypair-path ../../credentials/stakenet_test.json --validator-index-to-remove 0
```

## Close Steward

```bash
cargo run -- --program-id sssh4zkKhX8jXTNQz1xDHyGpygzgu2UhcRcUvZihBjP close-steward --steward-config 35mMfFNiui7hcHy6xHTz11Q6YukbhH9qQgYR5dhWAQQH --authority-keypair-path ../../credentials/stakenet_test.json
```

## Remove Bad Validators

```bash
cargo run -- --program-id sssh4zkKhX8jXTNQz1xDHyGpygzgu2UhcRcUvZihBjP remove-bad-validators --steward-config 35mMfFNiui7hcHy6xHTz11Q6YukbhH9qQgYR5dhWAQQH --payer-keypair-path ../../credentials/stakenet_test.json
```

## Crank Epoch Maintenance

```bash
cargo run -- --program-id sssh4zkKhX8jXTNQz1xDHyGpygzgu2UhcRcUvZihBjP crank-epoch-maintenance --steward-config 35mMfFNiui7hcHy6xHTz11Q6YukbhH9qQgYR5dhWAQQH --payer-keypair-path ../../credentials/stakenet_test.json
```

## Crank Compute Score

```bash
cargo run -- --program-id sssh4zkKhX8jXTNQz1xDHyGpygzgu2UhcRcUvZihBjP crank-compute-score --steward-config 35mMfFNiui7hcHy6xHTz11Q6YukbhH9qQgYR5dhWAQQH --payer-keypair-path ../../credentials/stakenet_test.json
```

## Crank Compute Delegations

```bash
cargo run -- --program-id sssh4zkKhX8jXTNQz1xDHyGpygzgu2UhcRcUvZihBjP crank-compute-delegations --steward-config 35mMfFNiui7hcHy6xHTz11Q6YukbhH9qQgYR5dhWAQQH --payer-keypair-path ../../credentials/stakenet_test.json
```

## Crank Idle

```bash
cargo run -- --program-id sssh4zkKhX8jXTNQz1xDHyGpygzgu2UhcRcUvZihBjP crank-idle --steward-config 35mMfFNiui7hcHy6xHTz11Q6YukbhH9qQgYR5dhWAQQH --payer-keypair-path ../../credentials/stakenet_test.json
```

## Crank Compute Instant Unstake

```bash
cargo run -- --program-id sssh4zkKhX8jXTNQz1xDHyGpygzgu2UhcRcUvZihBjP crank-compute-instant-unstake --steward-config 35mMfFNiui7hcHy6xHTz11Q6YukbhH9qQgYR5dhWAQQH --payer-keypair-path ../../credentials/stakenet_test.json
```

## Crank Rebalance

```bash
cargo run -- --program-id sssh4zkKhX8jXTNQz1xDHyGpygzgu2UhcRcUvZihBjP crank-rebalance --steward-config 35mMfFNiui7hcHy6xHTz11Q6YukbhH9qQgYR5dhWAQQH --payer-keypair-path ../../credentials/stakenet_test.json
```

## Crank Steward

```bash
cargo run -- --json-rpc-url $(solana config get | grep "RPC URL" | awk '{print $3}') crank-steward --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --payer-keypair-path ../../credentials/stakenet_test.json --priority-fee 200000
```

# Deploy and Upgrade

- upgrade solana cli to 1.18.16
- make sure your configured keypair is `aaaDerwdMyzNkoX1aSoTi3UtFe2W45vh5wCgQNhsjF8`
- create a new keypair: `solana-keygen new -o credentials/temp-buffer.json`
- use anchor `0.30.0`: `avm install 0.30.0 && avm use 0.30.0`
- build .so file: `anchor build --no-idl`
- Write to buffer: `solana program write-buffer --use-rpc --buffer credentials/temp-buffer.json --url $(solana config get | grep "RPC URL" | awk '{print $3}') --with-compute-unit-price 10000 --max-sign-attempts 10000 target/deploy/jito_steward.so --keypair credentials/stakenet_test.json`
- Upgrade: `solana program upgrade $(solana address --keypair credentials/temp-buffer.json) sssh4zkKhX8jXTNQz1xDHyGpygzgu2UhcRcUvZihBjP --keypair credentials/stakenet_test.json --url $(solana config get | grep "RPC URL" | awk '{print $3}')`
- Close Buffers: `solana program close --buffers --keypair credentials/stakenet_test.json`
- Upgrade Program Size: `solana program extend sssh4zkKhX8jXTNQz1xDHyGpygzgu2UhcRcUvZihBjP 1000000 --keypair credentials/stakenet_test.json --url $(solana config get | grep "RPC URL" | awk '{print $3}')`

# Initial Parameters

```bash
# Note - Do not use this .env when updating the parameters - this will update them all
MEV_COMMISSION_RANGE=10
EPOCH_CREDITS_RANGE=30
COMMISSION_RANGE=30
MEV_COMMISSION_BPS_THRESHOLD=1000
COMMISSION_THRESHOLD=5
HISTORICAL_COMMISSION_THRESHOLD=50
SCORING_DELINQUENCY_THRESHOLD_RATIO=0.85
INSTANT_UNSTAKE_DELINQUENCY_THRESHOLD_RATIO=0.70
NUM_DELEGATION_VALIDATORS=200
SCORING_UNSTAKE_CAP_BPS=750
INSTANT_UNSTAKE_CAP_BPS=1000
STAKE_DEPOSIT_UNSTAKE_CAP_BPS=1000
COMPUTE_SCORE_SLOT_RANGE=1000
INSTANT_UNSTAKE_EPOCH_PROGRESS=0.50
INSTANT_UNSTAKE_INPUTS_EPOCH_PROGRESS=0.50
NUM_EPOCHS_BETWEEN_SCORING=3
MINIMUM_STAKE_LAMPORTS=100000000000
MINIMUM_VOTING_EPOCHS=5
```

# Testing

```rust
debug_send_single_transaction(client, &Arc::new(authority), &configured_ix, Some(true)).await?;
```

```bash
Vote Account: 6VSu1wCkeugWdSB3ZgCCFSAttu5XTuSWVRD1vJVPVQXq
Stake Account: 2kdoEDkHqtVVQXghLEpxRjBQrKE73roJP525EBXEBtWZ
Transient Stake Account: MPyP83sm5fNBXAYaAxqPsURsEoaDn7P5L7rs4dBKwTm
Validator Lamports: 3285712
Index: 1737
Is Blacklisted: Ok(false)
Is Instant Unstake: Ok(true)
Score: Some(0)
Yield Score: Some(872581150)
Score Index: Some(307)
Yield Score Index: Some(1142)
```

# Getting Ready to Merge

```bash
cargo +nightly-2024-02-04 clippy --all-features --all-targets --tests -- -D warnings
anchor build --idl idl
```
