use anyhow::anyhow;
use solana_remote_wallet::{
    ledger::get_ledger_from_info,
    remote_keypair::RemoteKeypair,
    remote_wallet::{initialize_wallet_manager, RemoteWalletType},
};
use solana_sdk::{
    derivation_path::DerivationPath,
    pubkey::Pubkey,
    signature::{read_keypair_file, Keypair, Signature, Signer, SignerError},
};

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

    /// Creates a signer from a path
    pub fn new_keypair_from_path(keypair_path: &str) -> anyhow::Result<Self> {
        match read_keypair_file(keypair_path) {
            Ok(keypair) => Ok(Self::new(Some(keypair), None)),
            Err(e) => Err(anyhow!("{}", e)),
        }
    }

    // Will only work with Ledger devices
    pub fn new_ledger(path: &str) -> Self {
        println!("\nConnecting to Ledger Device");
        println!("- This will only work with Ledger devices.");
        println!("- It will use the first account on the first connected Ledger.");
        println!("- The Ledger must be unlocked and the Solana app open.\n");

        println!("Searching for wallets...");
        let wallet_manager =
            initialize_wallet_manager().expect("Could not initialize wallet manager");
        let device_count = wallet_manager
            .update_devices()
            .expect("Could not fetch devices");
        println!("Wallet found with {} device(s) connected", device_count);

        let devices = wallet_manager.list_devices();
        let device = devices.first().expect("No devices found");
        let ledger = get_ledger_from_info(device.clone(), "Signer", &wallet_manager)
            .expect("This CLI only supports Ledger devices");
        let derivation_path = DerivationPath::from_uri_key_query(
            &uriparse::URIReference::try_from(path).expect("Could not create URIReference"),
        )
        .expect("Could not create derivation path from str")
        .expect("Could not create derivation path from str");

        let path = format!("{}{}", ledger.pretty_path, derivation_path.get_query());
        let confirm_key = true;
        let remote_keypair = RemoteKeypair::new(
            RemoteWalletType::Ledger(ledger),
            derivation_path,
            confirm_key,
            path.clone(),
        )
        .expect("Could not create remote keypair");
        println!(
            "\nConnected to Ledger wallet specified by path and pubkey\n- {}\n- {}\n",
            path, remote_keypair.pubkey
        );

        Self::new(None, Some(remote_keypair))
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
