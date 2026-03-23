# Validator History CLI

This CLI tool provides various commands to interact with the validator history program on Solana.

### Commands


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
