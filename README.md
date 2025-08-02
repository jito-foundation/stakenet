# Stakenet

## About

Jito StakeNet is a decentralized Solana stake pool manager, blending Validator History and Steward Programs for secure, transparent validator management and autonomous stake operations.

## Validator History Program

The Validator History Program, a component of Jito StakeNet, is an on-chain record of verified Solana validator data, storing up to 512 epochs of history per validator. It takes fields accessible to the solana runtime like validator performance history, validator commission, MEV commission, as well as Gossip data like validator IP, version, and client type, and stores them all in a single account. It also contains some fields that currently require permissioned upload but are easily verifiable with a getVoteAccounts call, like total active stake per validator, stake rank, and superminority status. All these fields are stored in a single account per validator, the ValidatorHistory account. This enables all these disparate fields to be easily composed with in on chain programs, with a long lookback period and ease of access through the single account.

### Structure

The main Anchor program is in `programs/validator-history`.

### Important files

- `src/lib.rs` - entrypoint for instructions
- `src/state.rs` - containing the account definitions as well as logic for appending all the fields to the main circular buffer
- `src/instructions/*.rs` - individual instructions

### Accounts

`ValidatorHistory`: Tracks historical metadata on chain for a single validator. Contains a `CircBuf`, a data structure that acts as a wrap-around array. The CircBuf contains entries of `ValidatorHistoryEntry`, which stores validator metadata for an epoch. The default/null value for each field is the max value for the field's type.

Note that this is a `zero_copy` account, which allows us to initialize a lot of space without hitting runtime stack or heap size liimits. This has the constraint of requiring the struct to implement `bytemuck::{Pod, Zeroable}` and following C-style struct alignment.

`Config`: Tracks admin authorities as well as global program metadata.

## Steward Program

Harnessing on-chain validator metrics and network data, the Steward Program employs advanced algorithms to evaluate and rank validators. Automated keepers then execute a state machine to optimally allocate stake, maximizing network security and efficiency.

On-chain Steward accounts for JitoSOL:

| Account         | Address                                     |
|-----------------|---------------------------------------------|
| Program         | Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8 |
| Steward Config  | jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv |
| Steward State   | 9BAmGVLGxzqct6bkgjWmKSv3BFB6iKYXNBQp8GWG1LDY|
| Authority       | 9eZbWiHsPRsxLSiHxzg2pkXsAuQMwAjQrda7C7e21Fw6|


## Prerequisites

### Required Versions

- **Rust**: 1.88.0
- **Cargo**: 1.88.0
- **Anchor CLI**: 0.31.1
- **Solana CLI**: 2.3.6

## Build and Test

### Build

`anchor build --program-name validator_history` (regular anchor build)
`solana-verify build --library-name validator_history` (solana verified build)

### Verify

Verify with [solana-verifiable-build](https://github.com/Ellipsis-Labs/solana-verifiable-build):

`solana-verify verify-from-repo -um --program-id HistoryJTGbKQD2mRgLZ3XhqHnN811Qpez8X9kCcGHoa https://github.com/jito-foundation/stakenet`

### Test

Tests are in `tests/` written with solana-program-test.

All tests can be run by running ( root directory ):

```shell
./run_tests.sh
```

## Running Keeper

Check out the [Keeper Bot Quick Start](./keeper-bot-quick-start.md)


## CLIs

### Validator History

For detailed information on using the Validator History CLI, see the [CLI documentation](https://www.jito.network/docs/stakenet/validator-history/developers/cli/).

### Steward

For detailed information on using the Jito Steward CLI, see the [CLI documentation](https://www.jito.network/docs/stakenet/jito-steward/developers/cli/).


## Audits

| Program | Date | Commit |
|---------|------|--------|
| Steward | [2024-07-29](security-audits/jito_steward_audit.pdf) | [f4ea93a](https://github.com/jito-foundation/stakenet/commit/f4ea93a) |
| Validator History | [2024-01-12](security-audits/jito_validator_history_audit.pdf) | [fc34c25](https://github.com/jito-foundation/stakenet/commit/fc34c25) |
