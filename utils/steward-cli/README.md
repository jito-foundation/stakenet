# Steward CLI

## Accounts

| Account        | Address                                      |
| -------------- | -------------------------------------------- |
| Program        | Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8  |
| Steward Config | jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv  |
| Steward State  | 9BAmGVLGxzqct6bkgjWmKSv3BFB6iKYXNBQp8GWG1LDY |
| Authority      | 9eZbWiHsPRsxLSiHxzg2pkXsAuQMwAjQrda7C7e21Fw6 |

## CLI Commands

### Build

```bash
make build-release
```

## Permissionless Commands

### View Config

```bash
cargo run -p steward-cli -- \
    --json-rpc-url $(solana config get | grep "RPC URL" | awk '{print $3}') \
    --steward-program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 \
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
    --steward-program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 \
    --json-rpc-url $(solana config get | grep "RPC URL" | awk '{print $3}') \
    view-state \
    --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv \
    --verbose
```

### View Next Index To Remove

```bash
cargo run -p steward-cli -- \
    --steward-program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 \
    --json-rpc-url $(solana config get | grep "RPC URL" | awk '{print $3}') \
    view-next-index-to-remove \
    --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv
```

### View Blacklist

```bash
./target/release/steward-cli \
    --steward-program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 \
    --validator-history-program-id HistoryJTGbKQD2mRgLZ3XhqHnN811Qpez8X9kCcGHoa \
    --json-rpc-url "" \
    view-blacklist
```

### Auto Remove Validator

```bash
cargo run -- --steward-program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 auto-remove-validator-from-pool --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --payer-keypair-path ../../credentials/stakenet_test.json --validator-index-to-remove 1397
```

### Auto Add Validator

```bash
cargo run -- --steward-program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 auto-add-validator-from-pool --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --payer-keypair-path ../../credentials/stakenet_test.json --vote-account 4m64H5TbwAGtZVnxaGAVoTSwjZGV8BCLKRPr8agKQv4Z
```

### Manually Update All Vote Accounts

```bash
cargo run -- --steward-program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 manually-copy-all-vote-accounts --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --payer-keypair-path ../../credentials/stakenet_test.json --priority-fee 300000
```

## Manually Update Vote Account

```bash
cargo run -- --steward-program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 manually-copy-vote-account --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --payer-keypair-path ../../credentials/stakenet_test.json --validator-index-to-update 1
```

### Manually Remove Validator

```bash
cargo run -- --steward-program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 manually-remove-validator  --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --authority-keypair-path ../../credentials/stakenet_test.json --validator-index-to-remove 0
```

## Remove Bad Validators

```bash
cargo run -- --steward-program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 remove-bad-validators --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --payer-keypair-path ../../credentials/stakenet_test.json
```

## Permissionless Cranks

## Crank Epoch Maintenance

```bash
cargo run -- --steward-program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 crank-epoch-maintenance --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --payer-keypair-path ../../credentials/stakenet_test.json
```

## Crank Compute Score

```bash
cargo run -- --steward-program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 crank-compute-score --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --payer-keypair-path ../../credentials/stakenet_test.json
```

## Crank Compute Delegations

```bash
cargo run -- --steward-program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 crank-compute-delegations --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --payer-keypair-path ../../credentials/stakenet_test.json
```

## Crank Idle

```bash
cargo run -- --steward-program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 crank-idle --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --payer-keypair-path ../../credentials/stakenet_test.json
```

## Crank Compute Instant Unstake

```bash
cargo run -- --steward-program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 crank-compute-instant-unstake --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --payer-keypair-path ../../credentials/stakenet_test.json
```

## Crank Rebalance

```bash
cargo run -- --steward-program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 crank-rebalance --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --payer-keypair-path ../../credentials/stakenet_test.json
```

## Crank Steward

```bash
cargo run -- --json-rpc-url $(solana config get | grep "RPC URL" | awk '{print $3}') crank-steward --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --payer-keypair-path ../../credentials/stakenet_test.json --priority-fee 200000
```

## Privileged Commands

### Create Steward

```bash
cargo run -- --steward-program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 init-steward \
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
cargo run -p steward-cli -- \
    --program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 \
    realloc-state \
    --authority-keypair-path ../../credentials/stakenet_test.json \
    --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv
```

### Migrate State To V2

```bash
cargo run -p steward-cli -- \
    --program-id 3YeBnUPN2ZW8MBVb8695Hdffu8jBpRjm6BUazRexHDTg \
    migrate-state-to-v2 \
    --authority-keypair-path ~/.config/solana/id.json \
    --steward-config F4bBBC1am1PTow5TJYy6cbbLbPoEEN7peAbxRWqHKaNP
```

### Update Config

```bash
cargo run -- --steward-program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 update-config \
  --authority-keypair-path ../../credentials/stakenet_test.json \
  --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv \
  --num-epochs-between-scoring 3
```

### Update Authority

**Direct execution:**

`blacklist` | `admin` | `parameters` | `priority-fee-parameters` | `directed-stake-meta-upload` | `directed-stake-whitelist`

```bash
./target/release/steward-cli \
  --steward-program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 \
  update-authority \
  --signer  ../../credentials/stakenet_test.json \
  blacklist \
  --authority-keypair-path ../../credentials/stakenet_test.json \
  --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv \
  --new-authority aaaDerwdMyzNkoX1aSoTi3UtFe2W45vh5wCgQNhsjF8
```

**Creating a Squads multisig proposal with Ledger:**

`blacklist` | `admin` | `directed-stake-whitelist`

```bash
./target/release/steward-cli \
  --steward-program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 \
  --json-rpc-url "https://api.testnet.solana.com" \
  update-authority \
  --signer ledger \
  --squads-proposal \
  --squads-multisig 87zx3xqcWzP9DpGgbrNGnVsU6Dzci3XvaQvuTkgfWF5c \
  blacklist \
  --steward-config 5pZmpk3ktweGZW9xFknpEHhQoWeAKTzSGwnCUyVdiye \
  --new-authority aaaDerwdMyzNkoX1aSoTi3UtFe2W45vh5wCgQNhsjF8 \
```

### Set Staker

```bash
cargo run -- --steward-program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 set-staker \
  --authority-keypair-path ../../credentials/stakenet_test.json \
  --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv
```

### Revert Staker

```bash
cargo run -- --steward-program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 revert-staker \
  --authority-keypair-path ../../credentials/stakenet_test.json \
  --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv
```

### Pause

```bash
cargo run -- --steward-program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 pause \
  --authority-keypair-path ../../credentials/stakenet_test.json \
  --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --print-tx
```

### Resume

```bash
cargo run -- --steward-program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 resume \
  --authority-keypair-path ../../credentials/stakenet_test.json \
  --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --print-tx
```

### Reset State

```bash
cargo run -- --steward-program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 reset-state --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv  --authority-keypair-path ../../credentials/stakenet_test.json
```

### Add To Blacklist

**Direct execution:**

```bash
cargo run -- --steward-program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 add-to-blacklist \
  --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv \
  --signer ../../credentials/stakenet_test.json \
  --validator-history-indices-to-blacklist 2168
```

**Creating a Squads multisig proposal with Ledger:**

```bash
cargo run -- --steward-program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 add-to-blacklist \
  --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv \
  --signer ledger \
  --validator-history-indices-to-blacklist 2168 \
  --squads-proposal \
  --squads-multisig 87zx3xqcWzP9DpGgbrNGnVsU6Dzci3XvaQvuTkgfWF5c \
  --squads-vault-index 0
```

Note: `--squads-multisig` defaults to the blacklist authority multisig and `--squads-vault-index` defaults to the main vault, so they can be omitted:

```bash
cargo run -- --steward-program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 add-to-blacklist \
  --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv \
  --signer ledger \
  --validator-history-indices-to-blacklist 2168 \
  --squads-proposal
```

### Remove From Blacklist

```bash
cargo run -- --steward-program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 remove-from-blacklist --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --authority-keypair-path ../../credentials/stakenet_test.json --validator-history-index-to-deblacklist 2168
```

## Close Steward

```bash
cargo run -- --steward-program-id Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 close-steward --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --authority-keypair-path ../../credentials/stakenet_test.json
```

## Directed Stake

Directed stake allows validators, users, and protocols to express preferences for how stake should be distributed across the validator set.

### System Components

- **DirectedStakeWhitelist**: Controls who can submit stake tickets (validators, users, protocols)
- **DirectedStakeMeta**: Aggregates all tickets and token balances to compute final stake distribution
- **DirectedStakeTicket**: Individual stake preference tickets submitted by whitelisted entities

### Setup Workflow

The complete setup follows five logical stages and should be performed in order:

#### Stage 1: Authority Configuration

Configure the authorities that can manage directed stake metadata and the whitelist.

#### Stage 2: Whitelist Setup

Create and configure the whitelist that controls who can submit directed stake tickets.

#### Stage 3: Metadata Setup

Create the aggregation system that combines all stake tickets and token balances.

#### Stage 4: Ticket Management

Enable whitelisted entities to create and manage their stake preference tickets.

#### Stage 5: Computation

Aggregate all tickets and compute the final stake distribution.

### Setup Checklist

**Phase 1: Authorities**
- [ ] Update stake meta upload authority
- [ ] Update stake whitelist authority

**Phase 2: Whitelist**
- [ ] Initialize DirectedStakeWhitelist account
- [ ] Reallocate DirectedStakeWhitelist account (runs once, handles multiple transactions automatically)
- [ ] Verify whitelist with view command
- [ ] Add validators to whitelist (repeat as needed)
- [ ] Add users to whitelist (repeat as needed)
- [ ] Add protocols to whitelist (repeat as needed)

**Phase 3: Metadata**
- [ ] Initialize DirectedStakeMeta account
- [ ] Reallocate DirectedStakeMeta account
- [ ] Verify metadata with view command

**Phase 4: Tickets** (per whitelisted entity)
- [ ] Initialize DirectedStakeTicket for each entity
- [ ] Verify tickets with view commands
- [ ] Update tickets with preferences (can be done multiple times)

**Phase 5: Computation**
- [ ] Run compute-directed-stake-meta
- [ ] Set up periodic computation (e.g., cron job)

### Commands

#### Update Stake Meta Upload Authority

Sets the authority that can upload and update directed stake metadata.

```bash
./target/release/steward-cli \
    --json-rpc-url http://127.0.0.1:8899 \
    --program-id 3YeBnUPN2ZW8MBVb8695Hdffu8jBpRjm6BUazRexHDTg \
    update-authority \
    --signer ~/.config/solana/id.json \
    directed-stake-meta-upload \
    --steward-config F4bBBC1am1PTow5TJYy6cbbLbPoEEN7peAbxRWqHKaNP \
    --new-authority BBBATax9kikSHQp8UTcyQL3tfU3BmQD9yid5qhC7QEAA \
    --authority-keypair-path ~/.config/solana/id.json
```

#### Update Stake Whitelist Authority

Sets the authority that can add/remove entries from the directed stake whitelist.

```bash
./target/release/steward-cli \
    --json-rpc-url http://127.0.0.1:8899 \
    --program-id 3YeBnUPN2ZW8MBVb8695Hdffu8jBpRjm6BUazRexHDTg \
    update-authority \
    --signer ~/.config/solana/id.json \
    directed-stake-whitelist  \
    --steward-config F4bBBC1am1PTow5TJYy6cbbLbPoEEN7peAbxRWqHKaNP \
    --new-authority BBBATax9kikSHQp8UTcyQL3tfU3BmQD9yid5qhC7QEAA \
    --authority-keypair-path ~/.config/solana/id.json
```

#### Initialize DirectedStakeWhitelist

Creates the whitelist account that will store approved validators, users, and protocols.

```bash
./target/release/steward-cli \
    --json-rpc-url http://127.0.0.1:8899 \
    --program-id 3YeBnUPN2ZW8MBVb8695Hdffu8jBpRjm6BUazRexHDTg \
    init-directed-stake-whitelist \
    --steward-config F4bBBC1am1PTow5TJYy6cbbLbPoEEN7peAbxRWqHKaNP \
    --authority-keypair-path ~/.config/solana/id.json
```

Expected output:
```
Initializing DirectedStakeWhitelist...
  Authority: <AUTHORITY_PUBKEY>
  Steward Config: F4bBBC1am1PTow5TJYy6cbbLbPoEEN7peAbxRWqHKaNP
  DirectedStakeWhitelist PDA: 83U6qSYdAuEZJiZYzkg4Rb7XyRiM4rpa2fjTE2ieA2X
✅ DirectedStakeWhitelist initialized successfully!
  Transaction signature: 3FDMPL4kJJPneNgo2CHikLxsBrSGu9sSeuf2qin9CYwmsaJRAYepr5ftMt2KgAnBaUQ51r3X2iRoahNavzPXQbZE
  DirectedStakeWhitelist account: 83U6qSYdAuEZJiZYzkg4Rb7XyRiM4rpa2fjTE2ieA2X
```

#### Reallocate DirectedStakeWhitelist

Grows the whitelist account to its full size.

```bash
./target/release/steward-cli \
    --json-rpc-url http://127.0.0.1:8899 \
    --program-id 3YeBnUPN2ZW8MBVb8695Hdffu8jBpRjm6BUazRexHDTg \
    realloc-directed-stake-whitelist \
    --steward-config F4bBBC1am1PTow5TJYy6cbbLbPoEEN7peAbxRWqHKaNP \
    --authority-keypair-path ~/.config/solana/id.json
```

#### View DirectedStakeWhitelist

Displays the current state of the whitelist for verification.

```bash
./target/release/steward-cli \
    --json-rpc-url http://127.0.0.1:8899 \
    --program-id 3YeBnUPN2ZW8MBVb8695Hdffu8jBpRjm6BUazRexHDTg \
    view-directed-stake-whitelist \
    --steward-config F4bBBC1am1PTow5TJYy6cbbLbPoEEN7peAbxRWqHKaNP
```

#### Add to DirectedStakeWhitelist

Adds validators, users, or protocols to the whitelist, allowing them to submit stake tickets.

**Add a Validator:**
```bash
./target/release/steward-cli \
    --json-rpc-url http://127.0.0.1:8899 \
    --program-id 3YeBnUPN2ZW8MBVb8695Hdffu8jBpRjm6BUazRexHDTg \
    add-to-directed-stake-whitelist \
    --steward-config F4bBBC1am1PTow5TJYy6cbbLbPoEEN7peAbxRWqHKaNP \
    --authority-keypair-path ~/.config/solana/id.json \
    --record-type "validator" \
    --record BBBATax9kikSHQp8UTcyQL3tfU3BmQD9yid5qhC7QEAA
```

**Add a User:**
```bash
./target/release/steward-cli \
    --json-rpc-url http://127.0.0.1:8899 \
    --program-id 3YeBnUPN2ZW8MBVb8695Hdffu8jBpRjm6BUazRexHDTg \
    add-to-directed-stake-whitelist \
    --steward-config F4bBBC1am1PTow5TJYy6cbbLbPoEEN7peAbxRWqHKaNP \
    --authority-keypair-path ~/.config/solana/id.json \
    --record-type "user" \
    --record BBBATax9kikSHQp8UTcyQL3tfU3BmQD9yid5qhC7QEAA
```

**Add a Protocol:**
```bash
./target/release/steward-cli \
    --json-rpc-url http://127.0.0.1:8899 \
    --program-id 3YeBnUPN2ZW8MBVb8695Hdffu8jBpRjm6BUazRexHDTg \
    add-to-directed-stake-whitelist \
    --steward-config F4bBBC1am1PTow5TJYy6cbbLbPoEEN7peAbxRWqHKaNP \
    --authority-keypair-path ~/.config/solana/id.json \
    --record-type "protocol" \
    --record BBBATax9kikSHQp8UTcyQL3tfU3BmQD9yid5qhC7QEAA
```

#### Initialize DirectedStakeMeta

Creates the metadata account that will store aggregated stake preferences.

```bash
./target/release/steward-cli \
    --json-rpc-url http://127.0.0.1:8899 \
    --program-id 3YeBnUPN2ZW8MBVb8695Hdffu8jBpRjm6BUazRexHDTg \
    init-directed-stake-meta \
    --steward-config F4bBBC1am1PTow5TJYy6cbbLbPoEEN7peAbxRWqHKaNP \
    --authority-keypair-path ~/.config/solana/id.json
```

Expected output:
```
Initializing DirectedStakeMeta...
  Authority: <AUTHORITY_PUBKEY>
  Steward Config: F4bBBC1am1PTow5TJYy6cbbLbPoEEN7peAbxRWqHKaNP
  DirectedStakeMeta PDA: HK1WwbCnpefRfiZMTacHNMhLyU621uonSPCyCpB6mdp
✅ DirectedStakeMeta initialized successfully!
  Transaction signature: 2LXz9D6B5o3rs4bkQxhUju4bQZLXrmBni2AkawJCoXKv8VDR7H6rYxwQYeAjCViw2NNcsY7wdU2s3p41LBjjsgyn
  DirectedStakeMeta account: HK1WwbCnpefRfiZMTacHNMhLyU621uonSPCyCpB6mdp
```

#### Reallocate DirectedStakeMeta

Grows the metadata account to accommodate all aggregated data.

```bash
./target/release/steward-cli \
    --json-rpc-url http://127.0.0.1:8899 \
    --program-id 3YeBnUPN2ZW8MBVb8695Hdffu8jBpRjm6BUazRexHDTg \
    realloc-directed-stake-meta \
    --steward-config F4bBBC1am1PTow5TJYy6cbbLbPoEEN7peAbxRWqHKaNP \
    --authority-keypair-path ~/.config/solana/id.json
```

#### View DirectedStakeMeta

Displays the current state of the metadata account.

```bash
./target/release/steward-cli \
    --json-rpc-url http://127.0.0.1:8899 \
    --program-id 3YeBnUPN2ZW8MBVb8695Hdffu8jBpRjm6BUazRexHDTg \
    view-directed-stake-meta \
    --steward-config F4bBBC1am1PTow5TJYy6cbbLbPoEEN7peAbxRWqHKaNP
```

Displays the current state of the metadata account by specific vote pubkey.

```bash
./target/release/steward-cli \
    --json-rpc-url  https://api.mainnet-beta.solana.com \
    view-directed-stake-meta \
    --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv \
    --vote-pubkey DHoZJqvvMGvAXw85Lmsob7YwQzFVisYg8HY4rt5BAj6M
```

#### Initialize DirectedStakeTicket

Creates a ticket for a whitelisted entity to express stake preferences. Run by or on behalf of each whitelisted entity.

```bash
./target/release/steward-cli \
    --json-rpc-url http://127.0.0.1:8899 \
    --program-id 3YeBnUPN2ZW8MBVb8695Hdffu8jBpRjm6BUazRexHDTg \
    init-directed-stake-ticket \
    --steward-config F4bBBC1am1PTow5TJYy6cbbLbPoEEN7peAbxRWqHKaNP \
    --ticket-update-authority BBBATax9kikSHQp8UTcyQL3tfU3BmQD9yid5qhC7QEAA \
    --authority-keypair-path ~/.config/solana/id.json
```

Expected output:
```
Initializing DirectedStakeTicket...
  Authority: BBBATax9kikSHQp8UTcyQL3tfU3BmQD9yid5qhC7QEAA
  Steward Config: F4bBBC1am1PTow5TJYy6cbbLbPoEEN7peAbxRWqHKaNP
  Ticket Update Authority: BBBATax9kikSHQp8UTcyQL3tfU3BmQD9yid5qhC7QEAA
  DirectedStakeTicket PDA: 4j6nu2W19qimz61VJUHGVQ31fa5skaT1bfSRVUWNVnLJ
✅ DirectedStakeTicket initialized successfully!
  Transaction signature: 39iHv6nWkmVremYN1s4EHYxREwattZMjQFSb19dZ5YrC8JN85Tr4e1A5TF5WDq5zVaEMwasmrNwqueLSDBEsUvCd
  DirectedStakeTicket account: 4j6nu2W19qimz61VJUHGVQ31fa5skaT1bfSRVUWNVnLJ
```

#### View DirectedStakeTicket (Single)

Displays a specific ticket's current preferences.

```bash
./target/release/steward-cli \
    --json-rpc-url http://127.0.0.1:8899 \
    --program-id 3YeBnUPN2ZW8MBVb8695Hdffu8jBpRjm6BUazRexHDTg \
    view-directed-stake-ticket \
    --ticket-signer BBBATax9kikSHQp8UTcyQL3tfU3BmQD9yid5qhC7QEAA
```

#### View DirectedStakeTickets (All)

Lists all tickets in the system.

```bash
./target/release/steward-cli \
    --json-rpc-url http://127.0.0.1:8899 \
    --program-id 3YeBnUPN2ZW8MBVb8695Hdffu8jBpRjm6BUazRexHDTg \
    view-directed-stake-tickets
```

#### Update DirectedStakeTicket

Updates a ticket with new validator preferences and stake allocations. Stake shares are specified in basis points (10000 bps = 100%).

```bash
./target/release/steward-cli \
    --json-rpc-url http://127.0.0.1:8899 \
    --program-id 3YeBnUPN2ZW8MBVb8695Hdffu8jBpRjm6BUazRexHDTg \
    update-directed-stake-ticket \
    --steward-config F4bBBC1am1PTow5TJYy6cbbLbPoEEN7peAbxRWqHKaNP \
    --authority-keypair-path ~/.config/solana/id.json \
    --vote-pubkey <VALIDATOR_1_VOTE_ACCOUNT> \
    --stake-share-bps 5000 \
    --vote-pubkey <VALIDATOR_2_VOTE_ACCOUNT> \
    --stake-share-bps 3000 \
    --vote-pubkey <VALIDATOR_3_VOTE_ACCOUNT> \
    --stake-share-bps 2000
```

Example distributing 50%, 30%, 20% across three validators:
- 5000 bps = 50%
- 3000 bps = 30%
- 2000 bps = 20%

#### Compute DirectedStakeMeta

Aggregates all tickets and computes the final stake distribution. Should be run:
- After validators/users update their tickets
- Refresh with current balances per epoch
- Before stake rebalancing operations

```bash
./target/release/steward-cli \
    --json-rpc-url http://127.0.0.1:8899 \
    --program-id 3YeBnUPN2ZW8MBVb8695Hdffu8jBpRjm6BUazRexHDTg \
    compute-directed-stake-meta \
    --steward-config F4bBBC1am1PTow5TJYy6cbbLbPoEEN7peAbxRWqHKaNP \
    --authority-keypair-path ~/.config/solana/id.json \
    --token-mint J1toso1uCk3RLmjorhTtrVwY9HJ7X8V9yYac6Y7kGCPn
```

The token mint (JitoSOL) is used to query balances and weight stake preferences accordingly.

### Ongoing Operations

**Regular Maintenance (Per Epoch):**

```bash
# Recompute metadata to reflect current balances and preferences
./target/release/steward-cli \
    --json-rpc-url <RPC_URL> \
    --program-id <PROGRAM_ID> \
    compute-directed-stake-meta \
    --steward-config <CONFIG> \
    --authority-keypair-path <PATH> \
    --token-mint J1toso1uCk3RLmjorhTtrVwY9HJ7X8V9yYac6Y7kGCPn
```

**Adding New Entities:**
1. Add to whitelist
2. Initialize ticket for entity
3. Entity updates preferences
4. Recompute metadata

**Updating Preferences:**
1. Entity updates ticket
2. Recompute metadata

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
