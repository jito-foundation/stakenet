# Validator History CLI

This CLI tool provides various commands to interact with the validator history program on Solana.

### Commands

#### Crank Copy Is Jito BAM Client

Copies the Jito BAM client status for each validator into their validator history accounts on-chain.

```bash
# Build
make build-release

# Run
./target/release/validator-history-cli \
  --json-rpc-url <JSON_RPC_URL> \
  crank-copy-is-jito-bam-client \
  --keypair-path ~/.config/solana/id.json \
  --kobe-api-base-url "https://kobe.testnet.jito.network"
```

##### Description

This command fetches all validator history accounts, determines which ones have not yet had their `is_jito_bam_client` field set for the current epoch, queries the Kobe API to identify BAM-participating validators, and submits on-chain transactions to record each validator's BAM status.

Vote accounts that no longer exist on-chain are automatically filtered out to avoid `ConstraintOwner` errors.

##### Parameters

- `--keypair-path` (`-k`): Path to the keypair used for signing transactions (default: `~/.config/solana/id.json`)
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
