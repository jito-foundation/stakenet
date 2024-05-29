# Validator History Program

## About

The Validator History Program, a component of Jito StakeNet, is an on-chain record of verified Solana validator data, storing up to 512 epochs of history per validator. It takes fields accessible to the solana runtime like validator performance history, validator commission, MEV commission, as well as Gossip data like validator IP, version, and client type, and stores them all in a single account. It also contains some fields that currently require permissioned upload but are easily verifiable with a getVoteAccounts call, like total active stake per validator, stake rank, and superminority status. All these fields are stored in a single account per validator, the ValidatorHistory account. This enables all these disparate fields to be easily composed with in on chain programs, with a long lookback period and ease of access through the single account.

## Structure

The main Anchor program is in `programs/validator-history`.

### Important files

- `src/lib.rs` - entrypoint for instructions
- `src/state.rs` - containing the account definitions as well as logic for appending all the fields to the main circular buffer
- `src/instructions/*.rs` - individual instructions

### Accounts

`ValidatorHistory`: Tracks historical metadata on chain for a single validator. Contains a `CircBuf`, a data structure that acts as a wrap-around array. The CircBuf contains entries of `ValidatorHistoryEntry`, which stores validator metadata for an epoch. The default/null value for each field is the max value for the field's type.

Note that this is a `zero_copy` account, which allows us to initialize a lot of space without hitting runtime stack or heap size liimits. This has the constraint of requiring the struct to implement `bytemuck::{Pod, Zeroable}` and following C-style struct alignment.

`Config`: Tracks admin authorities as well as global program metadata.

## Test

Tests are in `tests/` written with solana-program-test.

All tests can be run by running:
```shell
./run_tests.sh
```

## Build

`anchor build --program-name validator_history` (regular anchor build)
`solana-verify build --library-name validator_history` (solana verified build)

## Verify

Verify with [solana-verifiable-build](https://github.com/Ellipsis-Labs/solana-verifiable-build):

`solana-verify verify-from-repo -um --program-id HistoryJTGbKQD2mRgLZ3XhqHnN811Qpez8X9kCcGHoa https://github.com/jito-foundation/stakenet`

## Running Keeper

Run as binary:

Build: `cargo b -r --package validator-keeper`

Run: `./target/release/validator-keeper --json-rpc-url <YOUR RPC> --cluster mainnet --tip-distribution-program-id F2Zu7QZiTYUhPd7u9ukRVwxh7B71oA3NMJcHuCHc29P2 --program-id HistoryJTGbKQD2mRgLZ3XhqHnN811Qpez8X9kCcGHoa --interval 600 --keypair <YOUR KEYPAIR>`

Run as docker container (need to set environment variables in config/.env file):

`docker compose --env-file config/.env up -d --build  validator-keeper`

Metrics for running can be sent to your influx server if you set the SOLANA_METRICS_CONFIG env var.

## CLI

The CLI can be used to see the status of on-chain validator history data.

Build: `cargo b -r --package validator-history-cli`

To see the current epoch state of all validator history accounts:

`./target/release/validator-history-cli --json-rpc-url <YOUR RPC URL> cranker-status`

To see the historical state of a single validator history account:

`./target/release/validator-history-cli --json-rpc-url <YOUR RPC URL> history <VOTE ACCOUNT>`
