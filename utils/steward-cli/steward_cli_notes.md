
# Accounts

**Authority** 
`aaaDerwdMyzNkoX1aSoTi3UtFe2W45vh5wCgQNhsjF8`

**Steward Config**   
`AFohCpk3Mp3FEYhrZsAK4TUppWKCwNZMzLpnggYTJLdq`

**Stake Pool**
`3DuPtyTAKrxKfHkSPZ5fqCayMcGru1BarAKKTfGDeo2j`

**Staker**
`4m64H5TbwAGtZVnxaGAVoTSwjZGV8BCLKRPr8agKQv4Z`

**State**
`6SJrBTYSSu3jWmsPWWhMMHvrPxqKWXtLe9tRfYpU8EZa`

# Initial Commands

## Create Steward
```bash
cargo run init-steward \
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
cargo run realloc-state --authority-keypair-path ../../credentials/stakenet_test.json --steward-config AFohCpk3Mp3FEYhrZsAK4TUppWKCwNZMzLpnggYTJLdq
```


## Update Config
```bash
cargo run update-config \
  --authority-keypair-path ../../credentials/stakenet_test.json \
  --steward-config AFohCpk3Mp3FEYhrZsAK4TUppWKCwNZMzLpnggYTJLdq \
  --instant-unstake-inputs-epoch-progress 0.10 \
  --instant-unstake-epoch-progress 0.10
```


## Reset State
```bash
cargo run reset-state --steward-config AFohCpk3Mp3FEYhrZsAK4TUppWKCwNZMzLpnggYTJLdq  --authority-keypair-path ../../credentials/stakenet_test.json
```

## View Config
```bash
cargo run view-config --steward-config AFohCpk3Mp3FEYhrZsAK4TUppWKCwNZMzLpnggYTJLdq
```

## View State
```bash
cargo run view-state --steward-config AFohCpk3Mp3FEYhrZsAK4TUppWKCwNZMzLpnggYTJLdq
```

## View State Per Validator
```bash
cargo run view-state --verbose --steward-config AFohCpk3Mp3FEYhrZsAK4TUppWKCwNZMzLpnggYTJLdq
```

## View Next Index To Remove
```bash
cargo run view-next-index-to-remove --steward-config AFohCpk3Mp3FEYhrZsAK4TUppWKCwNZMzLpnggYTJLdq
```

## Auto Remove Validator
```bash
cargo run auto-remove-validator-from-pool --steward-config AFohCpk3Mp3FEYhrZsAK4TUppWKCwNZMzLpnggYTJLdq --payer-keypair-path ../../credentials/stakenet_test.json --validator-index-to-remove 1397
```

## Auto Add Validator
```bash
cargo run auto-add-validator-from-pool --steward-config AFohCpk3Mp3FEYhrZsAK4TUppWKCwNZMzLpnggYTJLdq --payer-keypair-path ../../credentials/stakenet_test.json --vote-account 4m64H5TbwAGtZVnxaGAVoTSwjZGV8BCLKRPr8agKQv4Z 
```

## Manually Update Vote Account
```bash
cargo run manually-copy-vote-account --steward-config AFohCpk3Mp3FEYhrZsAK4TUppWKCwNZMzLpnggYTJLdq --payer-keypair-path ../../credentials/stakenet_test.json --validator-index-to-update 1
```

## Manually Remove Validator
```bash
cargo run manually-remove-validator --steward-config AFohCpk3Mp3FEYhrZsAK4TUppWKCwNZMzLpnggYTJLdq --authority-keypair-path ../../credentials/stakenet_test.json --validator-index-to-remove 0
```

## Close Steward
```bash
cargo run close-steward --steward-config AFohCpk3Mp3FEYhrZsAK4TUppWKCwNZMzLpnggYTJLdq --authority-keypair-path ../../credentials/stakenet_test.json
```

## Remove Bad Validators
```bash
cargo run remove-bad-validators --steward-config AFohCpk3Mp3FEYhrZsAK4TUppWKCwNZMzLpnggYTJLdq --payer-keypair-path ../../credentials/stakenet_test.json
```

## Crank Epoch Maintenance
```bash
cargo run crank-epoch-maintenance --steward-config AFohCpk3Mp3FEYhrZsAK4TUppWKCwNZMzLpnggYTJLdq --payer-keypair-path ../../credentials/stakenet_test.json
```

## Crank Compute Score
```bash
cargo run crank-compute-score --steward-config AFohCpk3Mp3FEYhrZsAK4TUppWKCwNZMzLpnggYTJLdq --payer-keypair-path ../../credentials/stakenet_test.json
```

## Crank Compute Delegations
```bash
cargo run crank-compute-delegations --steward-config AFohCpk3Mp3FEYhrZsAK4TUppWKCwNZMzLpnggYTJLdq --payer-keypair-path ../../credentials/stakenet_test.json
```

## Crank Idle
```bash
cargo run crank-idle --steward-config AFohCpk3Mp3FEYhrZsAK4TUppWKCwNZMzLpnggYTJLdq --payer-keypair-path ../../credentials/stakenet_test.json
```

## Crank Compute Instant Unstake
```bash
cargo run crank-compute-instant-unstake --steward-config AFohCpk3Mp3FEYhrZsAK4TUppWKCwNZMzLpnggYTJLdq --payer-keypair-path ../../credentials/stakenet_test.json
```

## Crank Rebalance
```bash
cargo run crank-rebalance --steward-config AFohCpk3Mp3FEYhrZsAK4TUppWKCwNZMzLpnggYTJLdq --payer-keypair-path ../../credentials/stakenet_test.json
```

## Crank Monkey
```bash
cargo run -- --json-rpc-url $(solana config get | grep "RPC URL" | awk '{print $3}') crank-monkey --steward-config AFohCpk3Mp3FEYhrZsAK4TUppWKCwNZMzLpnggYTJLdq --payer-keypair-path ../../credentials/stakenet_test.json --priority-fee 200000
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


To Remove
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