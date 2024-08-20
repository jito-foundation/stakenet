---
layout: default
title: Parameters
---

# CLI

# Accounts

| Account        | Address                                      |
| -------------- | -------------------------------------------- |
| Program        | Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8  |
| Steward Config | jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv  |
| Steward State  | 9BAmGVLGxzqct6bkgjWmKSv3BFB6iKYXNBQp8GWG1LDY |
| Authority      | 9eZbWiHsPRsxLSiHxzg2pkXsAuQMwAjQrda7C7e21Fw6 |

# CLI Commands

Build CLI binary:

```bash
cargo build -p steward-cli --release
```

## Permissionless Commands

### View Config

```bash
./target/release/steward-cli --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 view-config --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv
```

### View State

Displays high level Steward internal operations including current state, total number of validators in the pool, next cycle epoch, etc.

```bash
./target/release/steward-cli --json-rpc-url $(solana config get | grep "RPC URL" | awk '{print $3}') view-state --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv
```

### View State of Single Validator

Displays state of a single Validator.

```bash
./target/release/steward-cli --json-rpc-url $(solana config get | grep "RPC URL" | awk '{print $3}') view-state --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --vote-account J1to1yufRnoWn81KYg1XkTWzmKjnYSnmE2VY8DGUJ9Qv
```

Output:

```
Vote Account: J1to1yufRnoWn81KYg1XkTWzmKjnYSnmE2VY8DGUJ9Qv
Stake Account: 6PAY8LEswawgCGnzB3tKGJBtELUwDpeMfDCiNpCyNt8q
Transient Stake Account: C2AurJCKxp5Q8DbaZ84aiSUiKKazqgRVsUiTiihqNYui
Steward List Index: 3
Overall Rank: 441
Score: 0
Yield Score: 912832510
Passing Eligibility Criteria: No
Target Delegation Percent: 0.0%

Is Instant Unstake: false
Is blacklisted: false

Validator History Index: 321

Active Lamports: 3398839 (0.00 ◎)
Transient Lamports: 0 (0.00 ◎)
Steward Internal Lamports: 114590
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

`Yield Score`: Validator's relative yield score

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
./target/release/steward-cli --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 --json-rpc-url $(solana config get | grep "RPC URL" | awk '{print $3}') view-state --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --verbose
```

### View Next Index To Remove

```bash
./target/release/steward-cli --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 view-next-index-to-remove --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv
```

### Auto Remove Validator

```bash
./target/release/steward-cli --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 auto-remove-validator-from-pool --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --payer-keypair-path ../../credentials/stakenet_test.json --validator-index-to-remove 1397
```

### Auto Add Validator

```bash
./target/release/steward-cli --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 auto-add-validator-from-pool --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --payer-keypair-path ../../credentials/stakenet_test.json --vote-account 4m64H5TbwAGtZVnxaGAVoTSwjZGV8BCLKRPr8agKQv4Z
```

### Manually Update All Vote Accounts

```bash
./target/release/steward-cli --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 manually-copy-all-vote-accounts --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --payer-keypair-path ../../credentials/stakenet_test.json --priority-fee 300000
```

## Manually Update Vote Account

```bash
./target/release/steward-cli --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 manually-copy-vote-account --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --payer-keypair-path ../../credentials/stakenet_test.json --validator-index-to-update 1
```

### Manually Remove Validator

```bash
./target/release/steward-cli --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 manually-remove-validator  --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --authority-keypair-path ../../credentials/stakenet_test.json --validator-index-to-remove 0
```

## Remove Bad Validators

```bash
./target/release/steward-cli --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 remove-bad-validators --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --payer-keypair-path ../../credentials/stakenet_test.json
```

## Permissionless Cranks

## Crank Epoch Maintenance

```bash
./target/release/steward-cli --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 crank-epoch-maintenance --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --payer-keypair-path ../../credentials/stakenet_test.json
```

## Crank Compute Score

```bash
./target/release/steward-cli --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 crank-compute-score --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --payer-keypair-path ../../credentials/stakenet_test.json
```

## Crank Compute Delegations

```bash
./target/release/steward-cli --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 crank-compute-delegations --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --payer-keypair-path ../../credentials/stakenet_test.json
```

## Crank Idle

```bash
./target/release/steward-cli --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 crank-idle --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --payer-keypair-path ../../credentials/stakenet_test.json
```

## Crank Compute Instant Unstake

```bash
./target/release/steward-cli --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 crank-compute-instant-unstake --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --payer-keypair-path ../../credentials/stakenet_test.json
```

## Crank Rebalance

```bash
./target/release/steward-cli --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 crank-rebalance --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --payer-keypair-path ../../credentials/stakenet_test.json
```

## Crank Steward

```bash
./target/release/steward-cli --json-rpc-url $(solana config get | grep "RPC URL" | awk '{print $3}') crank-steward --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --payer-keypair-path ../../credentials/stakenet_test.json --priority-fee 200000
```

## Privileged Commands

### Create Steward

```bash
./target/release/steward-cli --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 init-steward \
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
./target/release/steward-cli --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 realloc-state --authority-keypair-path ../../credentials/stakenet_test.json --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv
```

### Update Config

```bash
./target/release/steward-cli --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 update-config \
  --authority-keypair-path ../../credentials/stakenet_test.json \
  --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv \
  --num-epochs-between-scoring 3
```

### Update Authority

`blacklist` | `admin` | `parameters`

```bash
./target/release/steward-cli --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 update-authority blacklist \
  --authority-keypair-path ../../credentials/stakenet_test.json \
  --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv \
  --new-authority aaaDerwdMyzNkoX1aSoTi3UtFe2W45vh5wCgQNhsjF8
```

### Set Staker

```bash
./target/release/steward-cli --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 set-staker \
  --authority-keypair-path ../../credentials/stakenet_test.json \
  --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv
```

### Revert Staker

```bash
./target/release/steward-cli --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 revert-staker \
  --authority-keypair-path ../../credentials/stakenet_test.json \
  --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv
```

### Pause

```bash
./target/release/steward-cli --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 pause \
  --authority-keypair-path ../../credentials/stakenet_test.json \
  --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --print-tx
```

### Resume

```bash
./target/release/steward-cli --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 resume \
  --authority-keypair-path ../../credentials/stakenet_test.json \
  --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --print-tx
```

### Reset State

```bash
./target/release/steward-cli --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 reset-state --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv  --authority-keypair-path ../../credentials/stakenet_test.json
```

### Add To Blacklist

```bash
./target/release/steward-cli --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 add-to-blacklist --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --authority-keypair-path ../../credentials/stakenet_test.json --validator-history-index-to-blacklist 2168
```

### Remove From Blacklist

```bash
./target/release/steward-cli --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 remove-from-blacklist --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --authority-keypair-path ../../credentials/stakenet_test.json --validator-history-index-to-deblacklist 2168
```

## Close Steward

```bash
./target/release/steward-cli --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 close-steward --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --authority-keypair-path ../../credentials/stakenet_test.json
```
