use std::{collections::HashMap, str::FromStr, sync::Arc};

use anchor_lang::{AccountDeserialize, Discriminator};
use anyhow::Result;
use jito_steward::{
    utils::{StakePool, ValidatorList},
    Config, StewardState, StewardStateAccount, COMPUTE_DELEGATIONS, COMPUTE_INSTANT_UNSTAKES,
    COMPUTE_SCORE, EPOCH_MAINTENANCE, POST_LOOP_IDLE, PRE_LOOP_IDLE, REBALANCE,
};
use keeper_core::get_multiple_accounts_batched;
use solana_client::{
    nonblocking::rpc_client::RpcClient,
    rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig},
    rpc_filter::{Memcmp, RpcFilterType},
    rpc_response::RpcVoteAccountInfo,
};
use solana_sdk::{
    account::Account, account_utils::State, borsh0_10::try_from_slice_unchecked, pubkey::Pubkey,
    stake::state::StakeStateV2,
};
use spl_stake_pool::{
    find_stake_program_address, find_transient_stake_program_address,
    find_withdraw_authority_program_address,
};
use validator_history::{ClusterHistory, ValidatorHistory};

pub struct AllStewardAccounts {
    pub config_account: Box<Config>,
    pub config_address: Pubkey,
    pub state_account: Box<StewardStateAccount>,
    pub state_address: Pubkey,
    pub stake_pool_account: Box<StakePool>,
    pub stake_pool_address: Pubkey,
    pub stake_pool_withdraw_authority: Pubkey,
    pub validator_list_account: Box<ValidatorList>,
    pub validator_list_address: Pubkey,
}

pub struct AllValidatorAccounts {
    pub all_history_vote_account_map: HashMap<Pubkey, Option<Account>>,
    pub all_stake_account_map: HashMap<Pubkey, Option<Account>>,
}

impl Default for AllValidatorAccounts {
    fn default() -> Self {
        AllValidatorAccounts {
            all_history_vote_account_map: HashMap::new(),
            all_stake_account_map: HashMap::new(),
        }
    }
}

pub async fn get_all_validator_accounts(
    client: &Arc<RpcClient>,
    all_vote_accounts: &Vec<RpcVoteAccountInfo>,
    validator_history_program_id: &Pubkey,
) -> Result<Box<AllValidatorAccounts>> {
    let accounts_to_fetch = all_vote_accounts.iter().map(|vote_account| {
        let vote_account =
            Pubkey::from_str(&vote_account.vote_pubkey).expect("Could not parse vote account");
        let stake_account = get_stake_address(&vote_account, &vote_account);
        let history_account =
            get_validator_history_address(&vote_account, &validator_history_program_id);

        (vote_account, stake_account, history_account)
    });

    let vote_addresses: Vec<Pubkey> = accounts_to_fetch
        .clone()
        .into_iter()
        .map(|(vote_account, _, _)| vote_account)
        .collect();

    let stake_accounts_to_fetch: Vec<Pubkey> = accounts_to_fetch
        .clone()
        .into_iter()
        .map(|(_, stake_account, _)| stake_account)
        .collect();

    let history_accounts_to_fetch: Vec<Pubkey> = accounts_to_fetch
        .clone()
        .into_iter()
        .map(|(_, _, history_account)| history_account)
        .collect();

    let stake_accounts =
        get_multiple_accounts_batched(stake_accounts_to_fetch.as_slice(), client).await?;

    let history_accounts =
        get_multiple_accounts_batched(history_accounts_to_fetch.as_slice(), client).await?;

    Ok(Box::new(AllValidatorAccounts {
        all_history_vote_account_map: vote_addresses
            .clone()
            .into_iter()
            .zip(history_accounts)
            .collect::<HashMap<Pubkey, Option<Account>>>(),

        all_stake_account_map: vote_addresses
            .clone()
            .into_iter()
            .zip(stake_accounts)
            .collect::<HashMap<Pubkey, Option<Account>>>(),
    }))
}

pub async fn get_all_steward_validator_accounts(
    client: &Arc<RpcClient>,
    all_steward_accounts: &AllStewardAccounts,
    validator_history_program_id: &Pubkey,
) -> Result<Box<AllValidatorAccounts>> {
    let accounts_to_fetch = all_steward_accounts
        .validator_list_account
        .validators
        .iter()
        .map(|validator| {
            let vote_account = validator.vote_account_address;
            let stake_account =
                get_stake_address(&vote_account, &all_steward_accounts.stake_pool_address);
            let history_account =
                get_validator_history_address(&vote_account, &validator_history_program_id);

            (vote_account, stake_account, history_account)
        });

    let vote_addresses: Vec<Pubkey> = accounts_to_fetch
        .clone()
        .into_iter()
        .map(|(vote_account, _, _)| vote_account)
        .collect();

    let stake_accounts_to_fetch: Vec<Pubkey> = accounts_to_fetch
        .clone()
        .into_iter()
        .map(|(_, stake_account, _)| stake_account)
        .collect();

    let history_accounts_to_fetch: Vec<Pubkey> = accounts_to_fetch
        .clone()
        .into_iter()
        .map(|(_, _, history_account)| history_account)
        .collect();

    let stake_accounts =
        get_multiple_accounts_batched(stake_accounts_to_fetch.as_slice(), client).await?;

    let history_accounts =
        get_multiple_accounts_batched(history_accounts_to_fetch.as_slice(), client).await?;

    Ok(Box::new(AllValidatorAccounts {
        all_history_vote_account_map: vote_addresses
            .clone()
            .into_iter()
            .zip(history_accounts)
            .collect::<HashMap<Pubkey, Option<Account>>>(),

        all_stake_account_map: vote_addresses
            .clone()
            .into_iter()
            .zip(stake_accounts)
            .collect::<HashMap<Pubkey, Option<Account>>>(),
    }))
}

pub async fn get_all_history_accounts(
    client: &Arc<RpcClient>,
    validator_list: &ValidatorList,
    validator_history_program_id: &Pubkey,
) -> Result<HashMap<Pubkey, Option<ValidatorHistory>>> {
    let all_vote_accounts = validator_list
        .validators
        .iter()
        .map(|validator| validator.vote_account_address)
        .collect::<Vec<Pubkey>>();

    let all_history_accounts = all_vote_accounts
        .clone()
        .iter()
        .map(|vote_account| {
            get_validator_history_address(vote_account, validator_history_program_id)
        })
        .collect::<Vec<Pubkey>>();

    let history_accounts_raw =
        get_multiple_accounts_batched(all_history_accounts.as_slice(), client).await?;

    let history_accounts = history_accounts_raw
        .iter()
        .map(|account| {
            if account.is_none() {
                None
            } else {
                Some(
                    ValidatorHistory::try_deserialize(
                        &mut account.as_ref().unwrap().data.as_slice(),
                    )
                    .unwrap(),
                )
            }
        })
        .collect::<Vec<Option<ValidatorHistory>>>();

    let map = all_vote_accounts
        .iter()
        .zip(history_accounts)
        .map(|(key, value)| (*key, value))
        .collect::<HashMap<Pubkey, Option<ValidatorHistory>>>();

    Ok(map)
}

pub async fn get_all_steward_accounts(
    client: &Arc<RpcClient>,
    program_id: &Pubkey,
    steward_config: &Pubkey,
) -> Result<Box<AllStewardAccounts>> {
    let config_account = get_steward_config_account(client, steward_config).await?;
    let stake_pool_address = config_account.stake_pool;

    let stake_pool_account = get_stake_pool_account(client, &stake_pool_address).await?;

    let validator_list_address = stake_pool_account.validator_list;
    let steward_state_address = get_steward_state_address(program_id, steward_config);

    let validator_list_account =
        get_validator_list_account(client, &validator_list_address).await?;

    // let history_accounts =
    //     get_all_history_accounts(client, &validator_list_account, &validator_history::id()).await?;

    Ok(Box::new(AllStewardAccounts {
        stake_pool_account,
        config_address: *steward_config,
        stake_pool_withdraw_authority: get_withdraw_authority_address(&stake_pool_address),
        validator_list_account,
        validator_list_address,
        stake_pool_address,
        config_account,
        // history_accounts,
        state_account: get_steward_state_account(client, program_id, steward_config).await?,
        state_address: steward_state_address,
    }))
}

pub async fn get_steward_config_account(
    client: &RpcClient,
    steward_config: &Pubkey,
) -> Result<Box<Config>> {
    let config_raw_account = client.get_account(steward_config).await?;

    Ok(Box::new(Config::try_deserialize(
        &mut config_raw_account.data.as_slice(),
    )?))
}

pub fn get_steward_state_address(program_id: &Pubkey, steward_config: &Pubkey) -> Pubkey {
    let (steward_state, _) = Pubkey::find_program_address(
        &[StewardStateAccount::SEED, steward_config.as_ref()],
        program_id,
    );

    steward_state
}

pub async fn get_steward_state_account_and_address(
    client: &RpcClient,
    program_id: &Pubkey,
    steward_config: &Pubkey,
) -> Result<(Box<StewardStateAccount>, Pubkey)> {
    let steward_state = get_steward_state_address(program_id, steward_config);

    let state_raw_account = client.get_account(&steward_state).await?;
    Ok((
        Box::new(StewardStateAccount::try_deserialize(
            &mut state_raw_account.data.as_slice(),
        )?),
        steward_state,
    ))
}

pub async fn get_steward_state_account(
    client: &RpcClient,
    program_id: &Pubkey,
    steward_config: &Pubkey,
) -> Result<Box<StewardStateAccount>> {
    let steward_state = get_steward_state_address(program_id, steward_config);

    let state_raw_account = client.get_account(&steward_state).await?;
    Ok(Box::new(StewardStateAccount::try_deserialize(
        &mut state_raw_account.data.as_slice(),
    )?))
}

pub async fn get_stake_pool_account(
    client: &RpcClient,
    stake_pool: &Pubkey,
) -> Result<Box<StakePool>> {
    let stake_pool_account_raw = client.get_account(stake_pool).await?;

    Ok(Box::new(StakePool::try_deserialize(
        &mut stake_pool_account_raw.data.as_slice(),
    )?))
}

pub fn get_withdraw_authority_address(stake_pool_address: &Pubkey) -> Pubkey {
    let (withdraw_authority, _) =
        find_withdraw_authority_program_address(&spl_stake_pool::id(), stake_pool_address);

    withdraw_authority
}

pub async fn get_validator_history_accounts_with_retry(
    client: &RpcClient,
    program_id: Pubkey,
) -> Result<Vec<ValidatorHistory>> {
    for _ in 0..4 {
        if let Ok(validator_histories) = get_validator_history_accounts(client, program_id).await {
            return Ok(validator_histories);
        }
    }
    get_validator_history_accounts(client, program_id).await
}

pub async fn get_validator_history_accounts(
    client: &RpcClient,
    program_id: Pubkey,
) -> Result<Vec<ValidatorHistory>> {
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

pub async fn get_validator_list_account(
    client: &RpcClient,
    validator_list: &Pubkey,
) -> Result<Box<ValidatorList>> {
    let validator_list_account_raw = client.get_account(validator_list).await?;

    Ok(Box::new(ValidatorList::try_deserialize(
        &mut validator_list_account_raw.data.as_slice(),
    )?))
}

pub fn get_cluster_history_address(validator_history_program_id: &Pubkey) -> Pubkey {
    let (address, _) =
        Pubkey::find_program_address(&[ClusterHistory::SEED], validator_history_program_id);
    address
}

pub fn get_validator_history_address(
    vote_account: &Pubkey,
    validator_history_program_id: &Pubkey,
) -> Pubkey {
    let (address, _) = Pubkey::find_program_address(
        &[ValidatorHistory::SEED, &vote_account.to_bytes()],
        validator_history_program_id,
    );

    address
}

pub fn get_validator_history_config_address(validator_history_program_id: &Pubkey) -> Pubkey {
    let (address, _) = Pubkey::find_program_address(&[Config::SEED], validator_history_program_id);

    address
}

pub fn get_stake_address(vote_account_address: &Pubkey, stake_pool_address: &Pubkey) -> Pubkey {
    let (stake_address, _) = find_stake_program_address(
        &spl_stake_pool::id(),
        vote_account_address,
        stake_pool_address,
        None,
    );

    stake_address
}

pub fn get_transient_stake_address(
    vote_account_address: &Pubkey,
    stake_pool_address: &Pubkey,
    validator_list_account: &ValidatorList,
    validator_index: usize,
) -> Pubkey {
    let (transient_stake_address, _) = find_transient_stake_program_address(
        &spl_stake_pool::id(),
        vote_account_address,
        stake_pool_address,
        validator_list_account.validators[validator_index]
            .transient_seed_suffix
            .into(),
    );

    transient_stake_address
}

pub struct ProgressionInfo {
    pub index: usize,
    pub vote_account: Pubkey,
    pub history_account: Pubkey,
}

pub fn get_unprogressed_validators(
    all_steward_accounts: &AllStewardAccounts,
    validator_history_program_id: &Pubkey,
) -> Vec<ProgressionInfo> {
    (0..all_steward_accounts.state_account.state.num_pool_validators)
        .filter_map(|validator_index| {
            let has_progressed = all_steward_accounts
                .state_account
                .state
                .progress
                .get(validator_index as usize)
                .expect("Index is not in progress bitmask");
            if has_progressed {
                None
            } else {
                let vote_account = all_steward_accounts.validator_list_account.validators
                    [validator_index as usize]
                    .vote_account_address;
                let history_account =
                    get_validator_history_address(&vote_account, &validator_history_program_id);

                Some(ProgressionInfo {
                    index: validator_index as usize,
                    vote_account,
                    history_account,
                })
            }
        })
        .collect::<Vec<ProgressionInfo>>()
}

pub struct StakeAccountChecks {
    pub is_deactivated: bool,
    pub has_history: bool,
    pub deactivation_epoch: Option<u64>,
    pub has_stake_account: bool,
}

pub fn check_stake_accounts(
    all_validator_accounts: &AllValidatorAccounts,
    epoch: u64,
) -> HashMap<Pubkey, StakeAccountChecks> {
    let vote_accounts = all_validator_accounts
        .all_history_vote_account_map
        .keys()
        .cloned()
        .collect::<Vec<Pubkey>>();

    let checks = vote_accounts
        .clone()
        .into_iter()
        .map(|vote_account| {
            let stake_account = all_validator_accounts
                .all_stake_account_map
                .get(&vote_account)
                .expect("Could not find stake account in map");
            let history_account = all_validator_accounts
                .all_history_vote_account_map
                .get(&vote_account)
                .expect("Could not find history account in map");

            let deactivation_epoch = stake_account.as_ref().map(|stake_account| {
                // This code will only run if stake_account is Some
                let stake_state =
                    try_from_slice_unchecked::<StakeStateV2>(stake_account.data.as_slice())
                        .expect("Could not parse stake state");
                match stake_state {
                    StakeStateV2::Stake(_, stake, _) => stake.delegation.deactivation_epoch,
                    _ => 0,
                }
            });

            let has_history = history_account.is_some();
            StakeAccountChecks {
                is_deactivated: deactivation_epoch.unwrap_or(0) < epoch,
                has_history,
                has_stake_account: stake_account.is_some(),
                deactivation_epoch,
            }
        })
        .collect::<Vec<StakeAccountChecks>>();

    vote_accounts
        .into_iter()
        .zip(checks)
        .collect::<HashMap<Pubkey, StakeAccountChecks>>()
}

pub enum StateCode {
    NoState = 0x00,
    ComputeScore = 0x01 << 0,
    ComputeDelegations = 0x01 << 1,
    PreLoopIdle = 0x01 << 2,
    ComputeInstantUnstake = 0x01 << 3,
    Rebalance = 0x01 << 4,
    PostLoopIdle = 0x01 << 5,
}

pub fn state_to_state_code(steward_state: &StewardState) -> StateCode {
    if steward_state.has_flag(COMPUTE_SCORE) {
        StateCode::ComputeScore
    } else if steward_state.has_flag(COMPUTE_DELEGATIONS) {
        StateCode::ComputeDelegations
    } else if steward_state.has_flag(PRE_LOOP_IDLE) {
        StateCode::PreLoopIdle
    } else if steward_state.has_flag(COMPUTE_INSTANT_UNSTAKES) {
        StateCode::ComputeInstantUnstake
    } else if steward_state.has_flag(REBALANCE) {
        StateCode::Rebalance
    } else if steward_state.has_flag(POST_LOOP_IDLE) {
        StateCode::PostLoopIdle
    } else {
        StateCode::NoState
    }
}

pub fn format_state_string(steward_state: &StewardState) -> String {
    let mut state_string = String::new();

    // pub const COMPUTE_SCORE: u32 = 1 << 0;
    // pub const COMPUTE_DELEGATIONS: u32 = 1 << 1;
    // pub const EPOCH_MAINTENANCE: u32 = 1 << 2;
    // pub const PRE_LOOP_IDLE: u32 = 1 << 3;
    // pub const COMPUTE_INSTANT_UNSTAKES: u32 = 1 << 4;
    // pub const REBALANCE: u32 = 1 << 5;
    // pub const POST_LOOP_IDLE: u32 = 1 << 6;

    if steward_state.has_flag(EPOCH_MAINTENANCE) {
        state_string += "▣"
    } else {
        state_string += "□"
    }

    state_string += " ⇢ ";

    if steward_state.has_flag(COMPUTE_SCORE) {
        state_string += "▣"
    } else {
        state_string += "□"
    }

    if steward_state.has_flag(COMPUTE_DELEGATIONS) {
        state_string += "▣"
    } else {
        state_string += "□"
    }

    state_string += " ↺ ";

    if steward_state.has_flag(PRE_LOOP_IDLE) {
        state_string += "▣"
    } else {
        state_string += "□"
    }

    if steward_state.has_flag(COMPUTE_INSTANT_UNSTAKES) {
        state_string += "▣"
    } else {
        state_string += "□"
    }

    if steward_state.has_flag(REBALANCE) {
        state_string += "▣"
    } else {
        state_string += "□"
    }

    if steward_state.has_flag(POST_LOOP_IDLE) {
        state_string += "▣"
    } else {
        state_string += "□"
    }

    state_string
}

pub fn format_simple_state_string(steward_state: &StewardState) -> String {
    let mut state_string = String::new();

    // pub const COMPUTE_SCORE: u32 = 1 << 0;
    // pub const COMPUTE_DELEGATIONS: u32 = 1 << 1;
    // pub const EPOCH_MAINTENANCE: u32 = 1 << 2;
    // pub const PRE_LOOP_IDLE: u32 = 1 << 3;
    // pub const COMPUTE_INSTANT_UNSTAKES: u32 = 1 << 4;
    // pub const REBALANCE: u32 = 1 << 5;
    // pub const POST_LOOP_IDLE: u32 = 1 << 6;

    if steward_state.has_flag(EPOCH_MAINTENANCE) {
        state_string += "M"
    } else {
        state_string += "-"
    }

    if steward_state.has_flag(COMPUTE_SCORE) {
        state_string += "S"
    } else {
        state_string += "-"
    }

    if steward_state.has_flag(COMPUTE_DELEGATIONS) {
        state_string += "D"
    } else {
        state_string += "-"
    }

    if steward_state.has_flag(PRE_LOOP_IDLE) {
        state_string += "0"
    } else {
        state_string += "-"
    }

    if steward_state.has_flag(COMPUTE_INSTANT_UNSTAKES) {
        state_string += "U"
    } else {
        state_string += "-"
    }

    if steward_state.has_flag(REBALANCE) {
        state_string += "R"
    } else {
        state_string += "-"
    }

    if steward_state.has_flag(POST_LOOP_IDLE) {
        state_string += "1"
    } else {
        state_string += "-"
    }

    state_string
}
