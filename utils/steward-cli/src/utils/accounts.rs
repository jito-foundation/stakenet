use anchor_lang::AccountDeserialize;
use anyhow::Result;
use jito_steward::{
    utils::{StakePool, ValidatorList},
    Config, Staker, StewardStateAccount,
};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use spl_stake_pool::{
    find_stake_program_address, find_transient_stake_program_address,
    find_withdraw_authority_program_address,
};
use validator_history::{ClusterHistory, ValidatorHistory};

pub struct UsefulStewardAccounts {
    pub config_account: Box<Config>,
    pub config_address: Pubkey,
    pub staker_account: Box<Staker>,
    pub staker_address: Pubkey,
    pub state_account: Box<StewardStateAccount>,
    pub state_address: Pubkey,
    pub stake_pool_account: Box<StakePool>,
    pub stake_pool_address: Pubkey,
    pub stake_pool_withdraw_authority: Pubkey,
    pub validator_list_account: Box<ValidatorList>,
    pub validator_list_address: Pubkey,
}

pub async fn get_all_steward_accounts(
    client: &RpcClient,
    program_id: &Pubkey,
    steward_config: &Pubkey,
) -> Result<Box<UsefulStewardAccounts>> {
    let config_account = get_steward_config_account(client, steward_config).await?;
    let stake_pool_address = config_account.stake_pool;

    let stake_pool_account = get_stake_pool_account(client, &stake_pool_address).await?;
    let validator_list_address = stake_pool_account.validator_list;

    Ok(Box::new(UsefulStewardAccounts {
        config_account,
        config_address: *steward_config,
        state_account: get_steward_state_account(client, program_id, steward_config).await?,
        state_address: get_steward_state_address(program_id, steward_config),
        staker_account: get_steward_staker_account(client, program_id, steward_config).await?,
        staker_address: get_steward_staker_address(program_id, steward_config),
        stake_pool_address,
        stake_pool_account,
        stake_pool_withdraw_authority: get_withdraw_authority_address(&stake_pool_address),
        validator_list_account: get_validator_list_account(client, &validator_list_address).await?,
        validator_list_address,
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

pub fn get_steward_staker_address(program_id: &Pubkey, steward_config: &Pubkey) -> Pubkey {
    let (steward_staker, _) =
        Pubkey::find_program_address(&[Staker::SEED, steward_config.as_ref()], program_id);

    steward_staker
}

pub async fn get_steward_staker_account_and_address(
    client: &RpcClient,
    program_id: &Pubkey,
    steward_config: &Pubkey,
) -> Result<(Box<Staker>, Pubkey)> {
    let steward_staker = get_steward_staker_address(program_id, steward_config);

    let staker_raw_account = client.get_account(&steward_staker).await?;

    Ok((
        Box::new(Staker::try_deserialize(
            &mut staker_raw_account.data.as_slice(),
        )?),
        steward_staker,
    ))
}

pub async fn get_steward_staker_account(
    client: &RpcClient,
    program_id: &Pubkey,
    steward_config: &Pubkey,
) -> Result<Box<Staker>> {
    let steward_staker = get_steward_staker_address(program_id, steward_config);

    let staker_raw_account = client.get_account(&steward_staker).await?;

    Ok(Box::new(Staker::try_deserialize(
        &mut staker_raw_account.data.as_slice(),
    )?))
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

pub struct StakeAccountChecks {
    pub is_deactivated: bool,
    pub has_history: bool,
    pub deactivation_epoch: u64,
}

// pub async fn check_stake_accounts(
//     client: &Arc<RpcClient>,
//     stake_account_infos: &[StakeAccountInfo],
//     epoch: u64,
// ) -> Vec<StakeAccountChecks> {
//     let stake_accounts_to_fetch = stake_account_infos.iter().map(|info| info.stake_account);
//     let history_accounts_to_fetch = stake_account_infos
//         .iter()
//         .map(|info| info.validator_history);

//     println!(
//         "\nFetching {} stake accounts...\n",
//         stake_accounts_to_fetch.len()
//     );

//     let stake_accounts = get_multiple_accounts_batched(
//         stake_accounts_to_fetch.collect::<Vec<_>>().as_slice(),
//         &Arc::clone(client),
//     )
//     .await
//     .expect("Could not fetch stake accounts");

//     println!(
//         "Fetching {} history accounts...",
//         history_accounts_to_fetch.len()
//     );

//     let history_accounts = get_multiple_accounts_batched(
//         history_accounts_to_fetch.collect::<Vec<_>>().as_slice(),
//         &Arc::clone(client),
//     )
//     .await
//     .expect("Could not fetch history accounts");

//     assert!(stake_accounts.len() == stake_account_infos.len());

//     let mut stake_stats = Vec::new();

//     for index in 0..stake_accounts.len() {
//         let stake_account = &stake_accounts[index];
//         let history_account = &history_accounts[index];

//         let deactivation_epoch = stake_account
//             .as_ref()
//             .map(|stake_account| {
//                 // This code will only run if stake_account is Some
//                 let stake_state =
//                     try_from_slice_unchecked::<StakeStateV2>(stake_account.data.as_slice())
//                         .expect("Could not parse stake state");
//                 match stake_state {
//                     StakeStateV2::Stake(_, stake, _) => stake.delegation.deactivation_epoch,
//                     _ => 0,
//                 }
//             })
//             .unwrap_or(0);

//         let has_history = history_account.is_some();

//         let stats = StakeAccountChecks {
//             is_deactivated: deactivation_epoch < epoch,
//             has_history,
//             deactivation_epoch,
//         };

//         stake_stats.push(stats);
//     }

//     stake_stats
// }
