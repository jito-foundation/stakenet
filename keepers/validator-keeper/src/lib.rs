use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::{atomic::AtomicBool, Arc},
};

use anchor_lang::{AccountDeserialize, Discriminator, InstructionData, ToAccountMetas};
use keeper_core::{MultipleAccountsError, TransactionExecutionError};
use log::error;
use solana_account_decoder::UiDataSliceConfig;
use solana_client::{
    client_error::ClientError,
    nonblocking::rpc_client::RpcClient,
    rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig},
    rpc_filter::{Memcmp, RpcFilterType},
};
use solana_gossip::{
    cluster_info::ClusterInfo, gossip_service::GossipService,
    legacy_contact_info::LegacyContactInfo,
};
use solana_net_utils::bind_in_range;
use solana_sdk::{
    instruction::Instruction,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};
use solana_streamer::socket::SocketAddrSpace;

use jito_tip_distribution::state::TipDistributionAccount;
use thiserror::Error as ThisError;
use validator_history::{constants::MAX_ALLOC_BYTES, ClusterHistory, Config, ValidatorHistory};
pub mod entries;
pub mod operations;
pub mod state;

pub type Error = Box<dyn std::error::Error>;

pub const PRIORITY_FEE: u64 = 200_000;

#[derive(ThisError, Debug)]
pub enum KeeperError {
    #[error(transparent)]
    ClientError(#[from] ClientError),
    #[error(transparent)]
    TransactionExecutionError(#[from] TransactionExecutionError),
    #[error(transparent)]
    MultipleAccountsError(#[from] MultipleAccountsError),
    #[error("Custom: {0}")]
    Custom(String),
}

pub async fn get_tip_distribution_accounts(
    rpc_client: &RpcClient,
    tip_distribution_program: &Pubkey,
    epoch: u64,
) -> Result<Vec<Pubkey>, Error> {
    const EPOCH_OFFSET: usize = 8 + 32 + 32 + 1; // Discriminator + Pubkey + Pubkey + size of "None" Option<T>
    let config = RpcProgramAccountsConfig {
        filters: Some(vec![
            RpcFilterType::Memcmp(Memcmp::new_raw_bytes(
                0,
                TipDistributionAccount::discriminator().into(),
            )),
            RpcFilterType::Memcmp(Memcmp::new_raw_bytes(
                EPOCH_OFFSET,
                epoch.to_le_bytes().to_vec(),
            )),
        ]),
        account_config: RpcAccountInfoConfig {
            encoding: Some(solana_account_decoder::UiAccountEncoding::Base64),
            data_slice: Some(UiDataSliceConfig {
                offset: EPOCH_OFFSET,
                length: 8,
            }),
            ..RpcAccountInfoConfig::default()
        },
        ..RpcProgramAccountsConfig::default()
    };
    let res = rpc_client
        .get_program_accounts_with_config(tip_distribution_program, config)
        .await?;

    // we actually don't care about the data slice, we just want the pubkey
    Ok(res.into_iter().map(|x| x.0).collect::<Vec<Pubkey>>())
}

pub fn derive_cluster_history_address(program_id: &Pubkey) -> Pubkey {
    let (address, _) = Pubkey::find_program_address(&[ClusterHistory::SEED], program_id);
    address
}

pub fn derive_validator_history_address(vote_account: &Pubkey, program_id: &Pubkey) -> Pubkey {
    let (address, _) = Pubkey::find_program_address(
        &[ValidatorHistory::SEED, &vote_account.to_bytes()],
        program_id,
    );

    address
}

pub fn derive_validator_history_config_address(program_id: &Pubkey) -> Pubkey {
    let (address, _) = Pubkey::find_program_address(&[Config::SEED], program_id);

    address
}

pub fn get_create_validator_history_instructions(
    vote_account: &Pubkey,
    program_id: &Pubkey,
    signer: &Keypair,
) -> Vec<Instruction> {
    let validator_history_account = derive_validator_history_address(vote_account, program_id);
    let config_account = derive_validator_history_config_address(program_id);

    let mut ixs = vec![Instruction {
        program_id: *program_id,
        accounts: validator_history::accounts::InitializeValidatorHistoryAccount {
            validator_history_account,
            vote_account: *vote_account,
            system_program: solana_program::system_program::id(),
            signer: signer.pubkey(),
        }
        .to_account_metas(None),
        data: validator_history::instruction::InitializeValidatorHistoryAccount {}.data(),
    }];

    let num_reallocs = (ValidatorHistory::SIZE - MAX_ALLOC_BYTES) / MAX_ALLOC_BYTES + 1;
    ixs.extend(vec![
        Instruction {
            program_id: *program_id,
            accounts: validator_history::accounts::ReallocValidatorHistoryAccount {
                validator_history_account,
                vote_account: *vote_account,
                config: config_account,
                system_program: solana_program::system_program::id(),
                signer: signer.pubkey(),
            }
            .to_account_metas(None),
            data: validator_history::instruction::ReallocValidatorHistoryAccount {}.data(),
        };
        num_reallocs
    ]);

    ixs
}

pub async fn get_validator_history_accounts(
    client: &RpcClient,
    program_id: Pubkey,
) -> Result<Vec<ValidatorHistory>, ClientError> {
    let gpa_config = RpcProgramAccountsConfig {
        filters: Some(vec![RpcFilterType::Memcmp(Memcmp::new_raw_bytes(
            0,
            ValidatorHistory::discriminator().into(),
        ))]),
        account_config: RpcAccountInfoConfig {
            encoding: Some(solana_account_decoder::UiAccountEncoding::Base64),
            ..RpcAccountInfoConfig::default()
        },
        ..RpcProgramAccountsConfig::default()
    };
    let mut validator_history_accounts = client
        .get_program_accounts_with_config(&program_id, gpa_config)
        .await?;

    let validator_histories = validator_history_accounts
        .iter_mut()
        .filter_map(|(_, account)| {
            ValidatorHistory::try_deserialize(&mut account.data.as_slice()).ok()
        })
        .collect::<Vec<_>>();

    Ok(validator_histories)
}

pub async fn get_validator_history_accounts_with_retry(
    client: &RpcClient,
    program_id: Pubkey,
) -> Result<Vec<ValidatorHistory>, ClientError> {
    for _ in 0..4 {
        if let Ok(validator_histories) = get_validator_history_accounts(client, program_id).await {
            return Ok(validator_histories);
        }
    }
    get_validator_history_accounts(client, program_id).await
}

pub async fn get_balance_with_retry(
    client: &RpcClient,
    account: Pubkey,
) -> Result<u64, ClientError> {
    let mut retries = 5;
    loop {
        match client.get_balance(&account).await {
            Ok(balance) => return Ok(balance),
            Err(e) => {
                if retries == 0 {
                    return Err(e);
                }
                retries -= 1;
            }
        }
    }
}

pub fn start_spy_server(
    cluster_entrypoint: SocketAddr,
    gossip_port: u16,
    spy_socket_addr: SocketAddr,
    keypair: &Arc<Keypair>,
    exit: Arc<AtomicBool>,
) -> (GossipService, Arc<ClusterInfo>) {
    // bind socket to expected port
    let (_, gossip_socket) = bind_in_range(
        IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        (gossip_port, gossip_port + 1),
    )
    .map_err(|e| {
        error!("Failed to bind to expected port");
        e
    })
    .expect("Failed to bind to expected gossip port");

    // connect to entrypoint and start spying on gossip
    let node = ClusterInfo::gossip_contact_info(keypair.pubkey(), spy_socket_addr, 0);
    let cluster_info = Arc::new(ClusterInfo::new(
        node,
        keypair.clone(),
        SocketAddrSpace::Unspecified,
    ));

    cluster_info.set_entrypoint(LegacyContactInfo::new_gossip_entry_point(
        &cluster_entrypoint,
    ));
    let gossip_service =
        GossipService::new(&cluster_info, None, gossip_socket, None, true, None, exit);
    (gossip_service, cluster_info)
}
