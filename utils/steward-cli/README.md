# Accounts

| Account        | Address                                      |
| -------------- | -------------------------------------------- |
| Program        | Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8  |
| Steward Config | jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv  |
| Steward State  | 9BAmGVLGxzqct6bkgjWmKSv3BFB6iKYXNBQp8GWG1LDY |
| Authority      | 9eZbWiHsPRsxLSiHxzg2pkXsAuQMwAjQrda7C7e21Fw6 |

# CLI Commands

## Permissionless Commands

### View Config

```bash
cargo run -p steward-cli -- \
    --json-rpc-url $(solana config get | grep "RPC URL" | awk '{print $3}') \
    --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 \
    view-config \
    --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv
```

### View State

Displays high level Steward internal operations including current state, total number of validators in the pool, next cycle epoch, etc.

```bash
cargo run -p steward-cli -- \
    --json-rpc-url $(solana config get | grep "RPC URL" | awk '{print $3}') \
    view-state \
    --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv
```

### View State of Single Validator

Displays state of a single Validator.

```bash
cargo run -p steward-cli -- \
    --json-rpc-url $(solana config get | grep "RPC URL" | awk '{print $3}') \
    view-state \
    --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv \
    --vote-account J1to1yufRnoWn81KYg1XkTWzmKjnYSnmE2VY8DGUJ9Qv
```

Output:

```
Vote Account: J1to1yufRnoWn81KYg1XkTWzmKjnYSnmE2VY8DGUJ9Qv
Stake Account: 6PAY8LEswawgCGnzB3tKGJBtELUwDpeMfDCiNpCyNt8q
Transient Stake Account: C2AurJCKxp5Q8DbaZ84aiSUiKKazqgRVsUiTiihqNYui
Steward List Index: 3
Overall Rank: 404
Score: 6957991073806817273
Inflation Commission: 4
MEV commission BPS: 800
Validator Age: 544
Vote Credits: 9967609
Commission Score: 6957991073806817273
Passing Eligibility Criteria: Yes
Target Delegation Percent: 0.0%

Is Instant Unstake: false
Is blacklisted: false

Validator History Index: 321

Active Lamports: 3289511 (0.00 ◎)
Transient Lamports: 0 (0.00 ◎)
Steward Internal Lamports: 0
Status: 🟩 Active
Marked for removal: false
Marked for immediate removal: false
```

`Vote Account`: Validator's vote account address

`Stake Account`: Validator's stake account from this stake pool

`Transient Stake Account`: Validator's transient stake account used for activating/deactivating stake

`Steward List Index`: Position in the Steward list, 1-1 with spl-stake-pool `ValidatorList`

`Overall Rank`: Validator's rank among all validators, indicating priority for stake if Target is nonzero, and priority for unstaking if target is zero

`Passing Eligibility Criteria`: Indicates if validator meets binary eligibility requirements

`Score`: Validator's overall score

`Inflation Commission`: Validator's inflation commission

`MEV Commission BPS`: Validator's mev commission bps

`Validator Age`: Validator's age in epoch

`Vote Credits`: Validator's vote credits

`Target Delegation Percent`: Share of the stake pool TVL this validator is targeted to receive. Not a guaranteed amount - dependent on staking and unstaking priority.

`Is Instant Unstake`: Indicates if this validator should be immediately unstaked

`Is blacklisted`: Indicates if validator is blacklisted from the pool

`Validator History Index`: Position in the validator history

`Active Lamports`: Amount of actively staked lamports

`Transient Lamports`: Amount of lamports in transient state

`Steward Internal Lamports`: Steward's internal tracking of stake used to detect user deposits

`Status`: Validator's `StakeStatus` in the spl-stake-pool `ValidatorList` account

`Marked for removal`: Indicates if validator is flagged for removal next epoch

`Marked for immediate removal`: Indicates if validator is flagged for immediate removal

### View State of All Validators

```bash
cargo run -p steward-cli -- \
    --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 \
    --json-rpc-url $(solana config get | grep "RPC URL" | awk '{print $3}') \
    view-state \
    --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv \
    --verbose
```

### View Next Index To Remove

```bash
cargo run -p steward-cli -- \
    --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 \
    --json-rpc-url $(solana config get | grep "RPC URL" | awk '{print $3}') \
    view-next-index-to-remove \
    --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv
```

### Auto Remove Validator

```bash
cargo run -- --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 auto-remove-validator-from-pool --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --payer-keypair-path ../../credentials/stakenet_test.json --validator-index-to-remove 1397
```

### Auto Add Validator

```bash
cargo run -- --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 auto-add-validator-from-pool --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --payer-keypair-path ../../credentials/stakenet_test.json --vote-account 4m64H5TbwAGtZVnxaGAVoTSwjZGV8BCLKRPr8agKQv4Z
```

### Manually Update All Vote Accounts

```bash
cargo run -- --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 manually-copy-all-vote-accounts --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --payer-keypair-path ../../credentials/stakenet_test.json --priority-fee 300000
```

## Manually Update Vote Account

```bash
cargo run -- --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 manually-copy-vote-account --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --payer-keypair-path ../../credentials/stakenet_test.json --validator-index-to-update 1
```

### Manually Remove Validator

```bash
cargo run -- --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 manually-remove-validator  --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --authority-keypair-path ../../credentials/stakenet_test.json --validator-index-to-remove 0
```

## Remove Bad Validators

```bash
cargo run -- --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 remove-bad-validators --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --payer-keypair-path ../../credentials/stakenet_test.json
```

## Permissionless Cranks

## Crank Epoch Maintenance

```bash
cargo run -- --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 crank-epoch-maintenance --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --payer-keypair-path ../../credentials/stakenet_test.json
```

## Crank Compute Score

```bash
cargo run -- --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 crank-compute-score --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --payer-keypair-path ../../credentials/stakenet_test.json
```

## Crank Compute Delegations

```bash
cargo run -- --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 crank-compute-delegations --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --payer-keypair-path ../../credentials/stakenet_test.json
```

## Crank Idle

```bash
cargo run -- --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 crank-idle --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --payer-keypair-path ../../credentials/stakenet_test.json
```

## Crank Compute Instant Unstake

```bash
cargo run -- --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 crank-compute-instant-unstake --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --payer-keypair-path ../../credentials/stakenet_test.json
```

## Crank Rebalance

```bash
cargo run -- --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 crank-rebalance --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --payer-keypair-path ../../credentials/stakenet_test.json
```

## Crank Steward

```bash
cargo run -- --json-rpc-url $(solana config get | grep "RPC URL" | awk '{print $3}') crank-steward --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --payer-keypair-path ../../credentials/stakenet_test.json --priority-fee 200000
```

## Privileged Commands

### Create Steward

```bash
cargo run -- --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 init-steward \
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

### Realloc State

```bash
cargo run -- --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 realloc-state --authority-keypair-path ../../credentials/stakenet_test.json --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv
```

### Update Config

```bash
cargo run -- --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 update-config \
  --authority-keypair-path ../../credentials/stakenet_test.json \
  --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv \
  --num-epochs-between-scoring 3
```

### Update Authority

`blacklist` | `admin` | `parameters`

```bash
cargo run -- --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 update-authority blacklist \
  --authority-keypair-path ../../credentials/stakenet_test.json \
  --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv \
  --new-authority aaaDerwdMyzNkoX1aSoTi3UtFe2W45vh5wCgQNhsjF8
```

### Set Staker

```bash
cargo run -- --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 set-staker \
  --authority-keypair-path ../../credentials/stakenet_test.json \
  --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv
```

### Revert Staker

```bash
cargo run -- --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 revert-staker \
  --authority-keypair-path ../../credentials/stakenet_test.json \
  --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv
```

### Pause

```bash
cargo run -- --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 pause \
  --authority-keypair-path ../../credentials/stakenet_test.json \
  --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --print-tx
```

### Resume

```bash
cargo run -- --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 resume \
  --authority-keypair-path ../../credentials/stakenet_test.json \
  --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --print-tx
```

### Reset State

```bash
cargo run -- --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 reset-state --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv  --authority-keypair-path ../../credentials/stakenet_test.json
```

### Add To Blacklist

**Direct execution:**

```bash
cargo run -- --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 add-to-blacklist \
  --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv \
  --signer ../../credentials/stakenet_test.json \
  --validator-history-indices-to-blacklist 2168
```

**Creating a Squads multisig proposal with Ledger:**

```bash
cargo run -- --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 add-to-blacklist \
  --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv \
  --signer ledger \
  --validator-history-indices-to-blacklist 2168 \
  --squads-proposal \
  --squads-multisig 87zx3xqcWzP9DpGgbrNGnVsU6Dzci3XvaQvuTkgfWF5c \
  --squads-vault-index 0
```

Note: `--squads-multisig` defaults to the blacklist authority multisig and `--squads-vault-index` defaults to the main vault, so they can be omitted:

```bash
cargo run -- --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 add-to-blacklist \
  --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv \
  --signer ledger \
  --validator-history-indices-to-blacklist 2168 \
  --squads-proposal
```

### Remove From Blacklist

```bash
cargo run -- --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 remove-from-blacklist --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --authority-keypair-path ../../credentials/stakenet_test.json --validator-history-index-to-deblacklist 2168
```

## Close Steward

```bash
cargo run -- --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 close-steward --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --authority-keypair-path ../../credentials/stakenet_test.json
```

# Deploy and Upgrade

- upgrade solana cli to 1.18.16
- make sure your configured keypair is `aaaDerwdMyzNkoX1aSoTi3UtFe2W45vh5wCgQNhsjF8`
- create a new keypair: `solana-keygen new -o credentials/temp-buffer.json`
- use anchor `0.30.0`: `avm install 0.30.0 && avm use 0.30.0`
- make sure your configured keypair is program authority
- build .so file: `anchor build --no-idl`
- Write to buffer: `solana program write-buffer --use-rpc --buffer credentials/temp-buffer.json --url $(solana config get | grep "RPC URL" | awk '{print $3}') --with-compute-unit-price 10000 --max-sign-attempts 10000 target/deploy/jito_steward.so --keypair credentials/stakenet_test.json`
- Upgrade: `solana program upgrade $(solana address --keypair credentials/temp-buffer.json) Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 --keypair credentials/stakenet_test.json --url $(solana config get | grep "RPC URL" | awk '{print $3}')`
- Close Buffers: `solana program close --buffers --keypair credentials/stakenet_test.json`
- Upgrade Program Size: `solana program extend Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 1000000 --keypair credentials/stakenet_test.json --url $(solana config get | grep "RPC URL" | awk '{print $3}')`

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

# Getting Ready to Merge

```bash
cargo +nightly-2024-02-04 clippy --all-features --all-targets --tests -- -D warnings
anchor build --idl idl
```
