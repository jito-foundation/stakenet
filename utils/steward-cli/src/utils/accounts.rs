use anchor_lang::AccountDeserialize;
use anyhow::Result;
use jito_steward::{
    utils::{StakePool, ValidatorList},
    Config, Staker, StewardStateAccount,
};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use spl_stake_pool::find_withdraw_authority_program_address;
use validator_history::{ClusterHistory, ValidatorHistory};

pub struct UsefulStewardAccounts {
    pub config_account: Config,
    pub staker_account: Staker,
    pub staker_address: Pubkey,
    pub state_account: StewardStateAccount,
    pub state_address: Pubkey,
    pub stake_pool_account: StakePool,
    pub stake_pool_address: Pubkey,
    pub stake_pool_withdraw_authority: Pubkey,
    pub validator_list_account: ValidatorList,
    pub validator_list_address: Pubkey,
}

pub async fn get_all_steward_accounts(
    client: &RpcClient,
    program_id: &Pubkey,
    steward_config: &Pubkey,
) -> Result<Box<UsefulStewardAccounts>> {
    let config_account = get_steward_config_account(client, steward_config).await?;
    let (state_account, state_address) =
        get_steward_state_account(client, program_id, steward_config).await?;
    let stake_pool_address = config_account.stake_pool;
    let stake_pool_account = get_stake_pool_account(client, &stake_pool_address).await?;
    let (staker_account, staker_address) =
        get_steward_staker_account(client, program_id, steward_config).await?;
    let stake_pool_withdraw_authority = get_withdraw_authority_address(&stake_pool_address);
    let validator_list_address = stake_pool_account.validator_list;
    let validator_list_account =
        get_validator_list_account(client, &validator_list_address).await?;

    Ok(Box::new(UsefulStewardAccounts {
        config_account,
        state_account,
        state_address,
        staker_account,
        staker_address,
        stake_pool_account,
        stake_pool_address,
        stake_pool_withdraw_authority,
        validator_list_account,
        validator_list_address,
    }))
}

pub async fn get_steward_config_account(
    client: &RpcClient,
    steward_config: &Pubkey,
) -> Result<Config> {
    let config_raw_account = client.get_account(steward_config).await?;

    Ok(Config::try_deserialize(
        &mut config_raw_account.data.as_slice(),
    )?)
}

pub fn get_steward_state_address(program_id: &Pubkey, steward_config: &Pubkey) -> Pubkey {
    let (steward_state, _) = Pubkey::find_program_address(
        &[StewardStateAccount::SEED, steward_config.as_ref()],
        program_id,
    );

    steward_state
}

pub async fn get_steward_state_account(
    client: &RpcClient,
    program_id: &Pubkey,
    steward_config: &Pubkey,
) -> Result<(StewardStateAccount, Pubkey)> {
    let steward_state = get_steward_state_address(program_id, steward_config);

    let state_raw_account = client.get_account(&steward_state).await?;
    Ok((
        StewardStateAccount::try_deserialize(&mut state_raw_account.data.as_slice())?,
        steward_state,
    ))
}

pub async fn get_stake_pool_account(client: &RpcClient, stake_pool: &Pubkey) -> Result<StakePool> {
    let stake_pool_account_raw = client.get_account(stake_pool).await?;

    Ok(StakePool::try_deserialize(
        &mut stake_pool_account_raw.data.as_slice(),
    )?)
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

pub async fn get_steward_staker_account(
    client: &RpcClient,
    program_id: &Pubkey,
    steward_config: &Pubkey,
) -> Result<(Staker, Pubkey)> {
    let steward_staker = get_steward_staker_address(program_id, steward_config);

    let staker_raw_account = client.get_account(&steward_staker).await?;

    Ok((
        Staker::try_deserialize(&mut staker_raw_account.data.as_slice())?,
        steward_staker,
    ))
}

pub async fn get_validator_list_account(
    client: &RpcClient,
    validator_list: &Pubkey,
) -> Result<ValidatorList> {
    let validator_list_account_raw = client.get_account(validator_list).await?;

    Ok(ValidatorList::try_deserialize(
        &mut validator_list_account_raw.data.as_slice(),
    )?)
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
