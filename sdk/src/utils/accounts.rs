use anchor_lang::{AccountDeserialize, Discriminator};
use jito_tip_distribution::state::TipDistributionAccount;
use solana_account_decoder::UiDataSliceConfig;
use solana_client::{
    nonblocking::rpc_client::RpcClient,
    rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig},
    rpc_filter::{Memcmp, RpcFilterType},
    rpc_response::RpcVoteAccountInfo,
};
use solana_sdk::pubkey::Pubkey;
use std::{collections::HashMap, str::FromStr, sync::Arc};

use validator_history::{ClusterHistory, ValidatorHistory};

pub type Error = Box<dyn std::error::Error>;
use jito_steward::{
    utils::{StakePool, ValidatorList},
    Config as StewardConfig, StewardStateAccount,
};

use solana_sdk::account::Account;

use spl_stake_pool::{
    find_stake_program_address, find_transient_stake_program_address,
    find_withdraw_authority_program_address,
};

use crate::models::{
    aggregate_accounts::{AllStewardAccounts, AllValidatorAccounts},
    errors::JitoTransactionError,
};

use super::transactions::get_multiple_accounts_batched;

// ---------------- GET ACCOUNTS ----------------

pub async fn get_all_validator_accounts(
    client: &Arc<RpcClient>,
    all_vote_accounts: &[RpcVoteAccountInfo],
    validator_history_program_id: &Pubkey,
) -> Result<Box<AllValidatorAccounts>, JitoTransactionError> {
    let accounts_to_fetch = all_vote_accounts.iter().map(|vote_account| {
        let vote_account =
            Pubkey::from_str(&vote_account.vote_pubkey).expect("Could not parse vote account");
        let stake_account = get_stake_address(&vote_account, &vote_account);
        let history_account =
            get_validator_history_address(&vote_account, validator_history_program_id);

        (vote_account, stake_account, history_account)
    });

    let vote_addresses: Vec<Pubkey> = accounts_to_fetch
        .clone()
        .map(|(vote_account, _, _)| vote_account)
        .collect();

    let stake_accounts_to_fetch: Vec<Pubkey> = accounts_to_fetch
        .clone()
        .map(|(_, stake_account, _)| stake_account)
        .collect();

    let history_accounts_to_fetch: Vec<Pubkey> = accounts_to_fetch
        .clone()
        .map(|(_, _, history_account)| history_account)
        .collect();

    let vote_accounts =
        get_multiple_accounts_batched(vote_addresses.clone().as_slice(), client).await?;

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

        all_vote_account_map: vote_addresses
            .into_iter()
            .zip(vote_accounts)
            .collect::<HashMap<Pubkey, Option<Account>>>(),
    }))
}

pub async fn get_all_steward_validator_accounts(
    client: &Arc<RpcClient>,
    all_steward_accounts: &AllStewardAccounts,
    validator_history_program_id: &Pubkey,
) -> Result<Box<AllValidatorAccounts>, JitoTransactionError> {
    let accounts_to_fetch = all_steward_accounts
        .validator_list_account
        .validators
        .iter()
        .map(|validator| {
            let vote_account = validator.vote_account_address;
            let stake_account =
                get_stake_address(&vote_account, &all_steward_accounts.stake_pool_address);
            let history_account =
                get_validator_history_address(&vote_account, validator_history_program_id);

            (vote_account, stake_account, history_account)
        });

    let vote_addresses: Vec<Pubkey> = accounts_to_fetch
        .clone()
        .map(|(vote_account, _, _)| vote_account)
        .collect();

    let stake_accounts_to_fetch: Vec<Pubkey> = accounts_to_fetch
        .clone()
        .map(|(_, stake_account, _)| stake_account)
        .collect();

    let history_accounts_to_fetch: Vec<Pubkey> = accounts_to_fetch
        .clone()
        .map(|(_, _, history_account)| history_account)
        .collect();

    let stake_accounts =
        get_multiple_accounts_batched(stake_accounts_to_fetch.as_slice(), client).await?;

    let history_accounts =
        get_multiple_accounts_batched(history_accounts_to_fetch.as_slice(), client).await?;

    let vote_accounts =
        get_multiple_accounts_batched(vote_addresses.clone().as_slice(), client).await?;

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

        all_vote_account_map: vote_addresses
            .into_iter()
            .zip(vote_accounts)
            .collect::<HashMap<Pubkey, Option<Account>>>(),
    }))
}

pub async fn get_all_validator_history_accounts(
    client: &RpcClient,
    program_id: Pubkey,
) -> Result<Vec<ValidatorHistory>, JitoTransactionError> {
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

pub async fn get_steward_history_accounts(
    client: &Arc<RpcClient>,
    validator_list: &ValidatorList,
    validator_history_program_id: &Pubkey,
) -> Result<HashMap<Pubkey, Option<ValidatorHistory>>, JitoTransactionError> {
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
) -> Result<Box<AllStewardAccounts>, JitoTransactionError> {
    let config_account = get_steward_config_account(client, steward_config).await?;
    let stake_pool_address = config_account.stake_pool;

    let stake_pool_account = get_stake_pool_account(client, &stake_pool_address).await?;

    let validator_list_address = stake_pool_account.validator_list;
    let steward_state_address = get_steward_state_address(program_id, steward_config);

    let validator_list_account =
        get_validator_list_account(client, &validator_list_address).await?;

    let reserve_stake_address = stake_pool_account.reserve_stake;
    let reserve_stake_account = client.get_account(&reserve_stake_address).await?;

    Ok(Box::new(AllStewardAccounts {
        stake_pool_account,
        config_address: *steward_config,
        stake_pool_withdraw_authority: get_withdraw_authority_address(&stake_pool_address),
        validator_list_account,
        validator_list_address,
        stake_pool_address,
        config_account,
        state_account: get_steward_state_account(client, program_id, steward_config).await?,
        state_address: steward_state_address,
        reserve_stake_address,
        reserve_stake_account,
    }))
}

// ---------------- GET ACCOUNTS ----------------

pub async fn get_steward_config_account(
    client: &RpcClient,
    steward_config: &Pubkey,
) -> Result<Box<StewardConfig>, JitoTransactionError> {
    let config_raw_account = client.get_account(steward_config).await?;

    StewardConfig::try_deserialize(&mut config_raw_account.data.as_slice())
        .map(Box::new)
        .map_err(|e| JitoTransactionError::Custom(format!("Failed to deserialize config: {}", e)))
}

pub async fn get_steward_state_account(
    client: &RpcClient,
    program_id: &Pubkey,
    steward_config: &Pubkey,
) -> Result<Box<StewardStateAccount>, JitoTransactionError> {
    let steward_state = get_steward_state_address(program_id, steward_config);

    let state_raw_account = client.get_account(&steward_state).await?;

    StewardStateAccount::try_deserialize(&mut state_raw_account.data.as_slice())
        .map_err(|e| {
            JitoTransactionError::Custom(format!(
                "Failed to deserialize steward state account: {}",
                e
            ))
        })
        .map(Box::new)
}

pub async fn get_stake_pool_account(
    client: &RpcClient,
    stake_pool: &Pubkey,
) -> Result<Box<StakePool>, JitoTransactionError> {
    let stake_pool_account_raw = client.get_account(stake_pool).await?;

    StakePool::try_deserialize(&mut stake_pool_account_raw.data.as_slice())
        .map_err(|e| {
            JitoTransactionError::Custom(format!("Failed to deserialize stake pool account: {}", e))
        })
        .map(Box::new)
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

pub async fn get_validator_list_account(
    client: &RpcClient,
    validator_list: &Pubkey,
) -> Result<Box<ValidatorList>, JitoTransactionError> {
    let validator_list_account_raw = client.get_account(validator_list).await?;

    ValidatorList::try_deserialize(&mut validator_list_account_raw.data.as_slice())
        .map_err(|e| {
            JitoTransactionError::Custom(format!(
                "Failed to deserialize validator list account: {}",
                e
            ))
        })
        .map(Box::new)
}

// ---------------- GET ADDRESSES ----------------

pub fn get_steward_state_address(steward_program_id: &Pubkey, steward_config: &Pubkey) -> Pubkey {
    let (steward_state, _) = Pubkey::find_program_address(
        &[StewardStateAccount::SEED, steward_config.as_ref()],
        steward_program_id,
    );

    steward_state
}

pub fn get_withdraw_authority_address(stake_pool_address: &Pubkey) -> Pubkey {
    let (withdraw_authority, _) =
        find_withdraw_authority_program_address(&spl_stake_pool::id(), stake_pool_address);

    withdraw_authority
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
    let (address, _) =
        Pubkey::find_program_address(&[StewardConfig::SEED], validator_history_program_id);

    address
}
