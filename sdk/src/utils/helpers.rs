use std::collections::HashMap;

use solana_client::{client_error::ClientError, nonblocking::rpc_client::RpcClient};
use solana_sdk::{pubkey::Pubkey, stake::state::StakeStateV2};

use crate::models::aggregate_accounts::{AllStewardAccounts, AllValidatorAccounts};
use spl_pod::solana_program::borsh1::try_from_slice_unchecked;

use super::accounts::get_validator_history_address;

// ------------------- BALANCE --------------------------
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

// ------------------- PROGRESS FETCH -------------------
pub struct ProgressionInfo {
    pub index: usize,
    pub vote_account: Pubkey,
    pub history_account: Pubkey,
}

/// Returns a list of validators that have not been progressed
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
                    get_validator_history_address(&vote_account, validator_history_program_id);

                Some(ProgressionInfo {
                    index: validator_index as usize,
                    vote_account,
                    history_account,
                })
            }
        })
        .collect::<Vec<ProgressionInfo>>()
}

// ------------------- VALIDATOR CHECKS -------------------
/// Return value of check_stake_accounts
pub struct StakeAccountChecks {
    pub is_deactivated: bool,
    pub has_history: bool,
    pub deactivation_epoch: Option<u64>,
    pub has_stake_account: bool,
    pub has_vote_account: bool,
}

/// Checks all of the Validator related accounts in AllValidatorAccounts
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
        .map(|vote_address| {
            let vote_account = all_validator_accounts
                .all_vote_account_map
                .get(&vote_address)
                .expect("Could not find vote account in map");

            let stake_account = all_validator_accounts
                .all_stake_account_map
                .get(&vote_address)
                .expect("Could not find stake account in map");
            let history_account = all_validator_accounts
                .all_history_vote_account_map
                .get(&vote_address)
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

            let has_vote_account = vote_account
                .as_ref()
                .map(|account| account.owner == solana_program::vote::program::id())
                .unwrap_or(false);

            let has_history = history_account.is_some();
            StakeAccountChecks {
                is_deactivated: deactivation_epoch.unwrap_or(0) < epoch,
                has_history,
                has_stake_account: stake_account.is_some(),
                deactivation_epoch,
                has_vote_account,
            }
        })
        .collect::<Vec<StakeAccountChecks>>();

    vote_accounts
        .into_iter()
        .zip(checks)
        .collect::<HashMap<Pubkey, StakeAccountChecks>>()
}
