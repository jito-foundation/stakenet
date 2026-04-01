# Validator History CLI

This CLI tool provides various commands to interact with the validator history program on Solana.

### Commands

#### Crank Copy Is BAM Connected

Copies the Jito BAM client status for each validator into their validator history accounts on-chain.

```bash
# Build
make build-release

# Run
./target/release/validator-history-cli \
  --json-rpc-url <JSON_RPC_URL> \
  crank-copy-is-bam-connected \
  --keypair-path ~/.config/solana/id.json \
  --kobe-api-base-url "https://kobe.testnet.jito.network" \
  --epoch 931
```

##### Description

This command fetches all validator history accounts, determines which ones have not yet had their `is_bam_connected` field set for the current epoch, queries the Kobe API to identify BAM-participating validators, and submits on-chain transactions to record each validator's BAM status.

Vote accounts that are missing or no longer owned by the vote program are automatically filtered out to avoid `ConstraintOwner` errors.

##### Parameters

- `--keypair-path` (`-k`): Path to the oracle authority keypair used for signing transactions (default: `~/.config/solana/id.json`)
- `--kobe-api-base-url`: Base URL for the Kobe API (e.g., `https://kobe.testnet.jito.network` for testnet, `https://kobe.mainnet.jito.network` for mainnet)

#### History

Displays the full epoch-by-epoch history for a single validator.

```bash
./target/release/validator-history-cli \
  --json-rpc-url <JSON_RPC_URL> \
  history <VOTE_ACCOUNT_PUBKEY>
```

##### Example

```bash
./target/release/validator-history-cli \
  --json-rpc-url https://api.mainnet-beta.solana.com \
  history naV1peWmGQiJDJfckGdPKk7GUf698X1sWmSu1ihxKDf
```

##### Parameters

- `<VOTE_ACCOUNT_PUBKEY>`: The vote account address of the validator to inspect (required, positional)
- `--start-epoch`: Epoch to start displaying history from (optional, defaults to earliest available)
- `--end-epoch`: Epoch to stop displaying history at (optional, defaults to current epoch)
- `--json` (`-j`): Print output in JSON format (optional)

#### Set New Tip Distribution Program

Updates the tip distribution program address stored in the on-chain Config account. Must be signed by the Config admin.

```bash
./target/release/validator-history-cli \
  --json-rpc-url <JSON_RPC_URL> \
  set-new-tip-distribution-program \
  --keypair-path ~/.config/solana/id.json \
  --tip-distribution-program-id <NEW_PROGRAM_ID>
```

##### Parameters

- `--keypair-path` (`-k`): Path to the admin keypair used for signing (default: `~/.config/solana/id.json`)
- `--tip-distribution-program-id`: The new tip distribution program ID to set on the Config account (required)

#### StakeByCountry

Displays JitoSOL stake distribution by country.

```bash
cargo r -p validator-history-cli -- --json-rpc-url <JSON_RPC_URL> stake-by-country --stake-pool <STAKE_POOL> --country <COUNTRY> --ip-info-token <IP_INFO_TOKEN>
 ```

##### Description

This command analyzes the geographical distribution of JitoSOL stake across validators worldwide.
It fetches validator IPs from their history account and determines their countries using IP geolocation.

##### Parameters

- `--stake-pool`: The stake pool address to analyze (required)
- `--country`: Filter results to show only a specific country (optional)
- `--ip-info-token`: API token for IP geolocation service (required)

To obtain an IP info token, sign up at (https://ipinfo.io)[https://ipinfo.io]
