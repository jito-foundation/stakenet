use anyhow::{anyhow, Context};
use solana_remote_wallet::{
    ledger::{get_ledger_from_info, LedgerWallet},
    remote_keypair::RemoteKeypair,
    remote_wallet::{initialize_wallet_manager, RemoteWallet, RemoteWalletType},
};
use solana_sdk::{
    derivation_path::DerivationPath,
    pubkey::Pubkey,
    signature::{read_keypair_file, Keypair, Signature, Signer, SignerError},
};
use std::rc::Rc;

const DEFAULT_LEDGER_ACCOUNT: u32 = 0;
const LEDGER_SCAN_ACCOUNTS: u32 = 20;

pub struct CliSigner {
    pub keypair: Option<Keypair>,
    pub remote_keypair: Option<RemoteKeypair>,
}

impl CliSigner {
    pub fn new(keypair: Option<Keypair>, remote_keypair: Option<RemoteKeypair>) -> Self {
        if keypair.is_none() && remote_keypair.is_none() {
            panic!("No keypair or wallet manager provided");
        }

        Self {
            keypair,
            remote_keypair,
        }
    }

    pub fn new_keypair(keypair: Keypair) -> Self {
        Self::new(Some(keypair), None)
    }

    pub fn from_path(
        signer_path: &str,
        ledger_wallet: Option<Pubkey>,
        ledger_key: Option<&str>,
    ) -> anyhow::Result<Self> {
        if signer_path == "ledger" {
            return Self::new_ledger(ledger_wallet, ledger_key);
        }

        if ledger_wallet.is_some() || ledger_key.is_some() {
            return Err(anyhow!(
                "--ledger-wallet and --ledger-key can only be used with --signer ledger"
            ));
        }

        Self::new_keypair_from_path(signer_path)
    }

    /// Creates a signer from a path
    pub fn new_keypair_from_path(keypair_path: &str) -> anyhow::Result<Self> {
        match read_keypair_file(keypair_path) {
            Ok(keypair) => Ok(Self::new(Some(keypair), None)),
            Err(e) => Err(anyhow!("{e}")),
        }
    }

    /// Will only work with Ledger devices
    /// Defaults to the legacy `?key=0` derivation path unless a wallet pubkey or key is provided.
    pub fn new_ledger(
        ledger_wallet: Option<Pubkey>,
        ledger_key: Option<&str>,
    ) -> anyhow::Result<Self> {
        println!("\nConnecting to Ledger Device");
        println!("- This will only work with Ledger devices.");
        println!("- The Ledger must be unlocked and the Solana app open.");
        println!("- Verify the public key shown on your Ledger screen.\n");

        println!("Searching for wallets...");
        let wallet_manager =
            initialize_wallet_manager().context("Could not initialize wallet manager")?;
        let device_count = wallet_manager
            .update_devices()
            .context("Could not fetch devices")?;
        println!("Wallet found with {device_count} device(s) connected");

        let devices = wallet_manager.list_devices();
        let device = devices.first().context("No devices found")?;
        let ledger = get_ledger_from_info(device.clone(), "Signer", &wallet_manager)
            .context("This CLI only supports Ledger devices")?;
        let requested_derivation_path = ledger_key.map(parse_ledger_key).transpose()?;
        let derivation_path =
            resolve_ledger_derivation_path(&ledger, ledger_wallet, requested_derivation_path)?;

        let display_path = format!("usb://ledger{}", derivation_path.get_query());
        let confirm_key = true;
        let remote_keypair = RemoteKeypair::new(
            RemoteWalletType::Ledger(ledger),
            derivation_path,
            confirm_key,
            display_path.clone(),
        )
        .context("Could not create remote keypair")?;

        if let Some(expected_pubkey) = ledger_wallet {
            if remote_keypair.pubkey != expected_pubkey {
                return Err(anyhow!(
                    "Resolved Ledger pubkey {} did not match requested --ledger-wallet {}",
                    remote_keypair.pubkey,
                    expected_pubkey
                ));
            }
        }

        println!(
            "\n✓ Connected to Ledger:\n  Path: {}\n  Pubkey: {}\n",
            display_path, remote_keypair.pubkey
        );

        Ok(Self::new(None, Some(remote_keypair)))
    }
}

fn default_ledger_derivation_path() -> DerivationPath {
    DerivationPath::new_bip44(Some(DEFAULT_LEDGER_ACCOUNT), None)
}

fn parse_ledger_key(ledger_key: &str) -> anyhow::Result<DerivationPath> {
    DerivationPath::from_key_str(ledger_key)
        .map_err(|e| anyhow!("Invalid --ledger-key `{ledger_key}`: {e}"))
}

fn resolve_ledger_derivation_path(
    ledger: &Rc<LedgerWallet>,
    ledger_wallet: Option<Pubkey>,
    requested_derivation_path: Option<DerivationPath>,
) -> anyhow::Result<DerivationPath> {
    match (ledger_wallet, requested_derivation_path) {
        (Some(expected_pubkey), Some(derivation_path)) => {
            let derived_pubkey = ledger
                .get_pubkey(&derivation_path, false)
                .with_context(|| {
                    format!(
                        "Could not read Ledger pubkey at path usb://ledger{}",
                        derivation_path.get_query()
                    )
                })?;

            if derived_pubkey != expected_pubkey {
                return Err(anyhow!(
                    "Requested --ledger-key usb://ledger{} resolves to {}, not {}",
                    derivation_path.get_query(),
                    derived_pubkey,
                    expected_pubkey
                ));
            }

            Ok(derivation_path)
        }
        (Some(expected_pubkey), None) => find_ledger_signer_path(ledger, expected_pubkey),
        (None, Some(derivation_path)) => Ok(derivation_path),
        (None, None) => Ok(default_ledger_derivation_path()),
    }
}

fn find_ledger_signer_path(
    ledger: &Rc<LedgerWallet>,
    target_pubkey: Pubkey,
) -> anyhow::Result<DerivationPath> {
    println!("Searching Ledger derivation paths for signer {target_pubkey}...");

    for derivation_path in ledger_candidate_paths() {
        let candidate_pubkey = ledger
            .get_pubkey(&derivation_path, false)
            .with_context(|| {
                format!(
                    "Could not read Ledger pubkey at path usb://ledger{}",
                    derivation_path.get_query()
                )
            })?;

        if candidate_pubkey == target_pubkey {
            println!(
                "Matched Ledger signer {target_pubkey} at path usb://ledger{}",
                derivation_path.get_query()
            );
            return Ok(derivation_path);
        }
    }

    Err(anyhow!(
        "Could not find Ledger signer {target_pubkey} in the scanned paths. Try --ledger-key <account[/change]> if you know the derivation path."
    ))
}

fn ledger_candidate_paths() -> Vec<DerivationPath> {
    let mut derivation_paths = Vec::with_capacity((LEDGER_SCAN_ACCOUNTS * 2 + 1) as usize);
    derivation_paths.push(DerivationPath::default());

    for account in 0..LEDGER_SCAN_ACCOUNTS {
        derivation_paths.push(DerivationPath::new_bip44(Some(account), None));
        derivation_paths.push(DerivationPath::new_bip44(Some(account), Some(0)));
    }

    derivation_paths
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ledger_candidate_paths_include_default_and_common_accounts() {
        let paths = ledger_candidate_paths();

        assert_eq!(paths[0].get_query(), "");
        assert!(paths.iter().any(|path| path.get_query() == "?key=0'"));
        assert!(paths.iter().any(|path| path.get_query() == "?key=0'/0'"));
        assert!(paths.iter().any(|path| path.get_query() == "?key=19'"));
        assert!(paths.iter().any(|path| path.get_query() == "?key=19'/0'"));
    }

    #[test]
    fn parse_ledger_key_supports_account_and_change_paths() {
        assert_eq!(parse_ledger_key("0").unwrap().get_query(), "?key=0'");
        assert_eq!(parse_ledger_key("2/1").unwrap().get_query(), "?key=2'/1'");
    }
}

impl Signer for CliSigner {
    fn try_pubkey(&self) -> Result<Pubkey, SignerError> {
        self.keypair.as_ref().map_or_else(
            || {
                self.remote_keypair
                    .as_ref()
                    .map_or(Err(SignerError::NoDeviceFound), |remote_keypair| {
                        Ok(remote_keypair.pubkey)
                    })
            },
            |keypair| Ok(keypair.pubkey()),
        )
    }

    fn try_sign_message(&self, message: &[u8]) -> Result<Signature, SignerError> {
        self.keypair.as_ref().map_or_else(
            || {
                self.remote_keypair
                    .as_ref()
                    .map_or(Err(SignerError::NoDeviceFound), |remote_keypair| {
                        remote_keypair.try_sign_message(message)
                    })
            },
            |keypair| keypair.try_sign_message(message),
        )
    }

    fn is_interactive(&self) -> bool {
        // Remote wallets are typically interactive, local keypairs are not
        self.remote_keypair.is_some()
    }
}
