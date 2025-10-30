use std::{collections::HashMap, sync::Arc};

use jito_steward::{constants::BASIS_POINTS_MAX, DirectedStakeTicket};
use solana_client::{client_error::ClientError, nonblocking::rpc_client::RpcClient};
use solana_sdk::{pubkey::Pubkey, stake::state::StakeStateV2};
use spl_associated_token_account::get_associated_token_address;

use crate::models::{
    aggregate_accounts::{AllStewardAccounts, AllValidatorAccounts},
    errors::JitoInstructionError,
};
use solana_program::borsh1::try_from_slice_unchecked;

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

pub struct DirectedRebalanceProgressionInfo {
    /// Validator index
    pub validator_list_index: usize,

    /// Directed stake meta index
    pub directed_stake_meta_index: usize,

    /// Vote account pubkey
    pub vote_account: Pubkey,
}

impl DirectedRebalanceProgressionInfo {
    pub fn get_directed_staking_validators(
        all_steward_accounts: &AllStewardAccounts,
    ) -> Vec<DirectedRebalanceProgressionInfo> {
        let mut pregression_info = Vec::new();

        for validator_list_index in 0..all_steward_accounts.state_account.state.num_pool_validators
        {
            let vote_account = all_steward_accounts.validator_list_account.validators
                [validator_list_index as usize]
                .vote_account_address;
            if let Some(directed_stake_meta_index) = all_steward_accounts
                .directed_stake_meta_account
                .get_target_index(&vote_account)
            {
                pregression_info.push(DirectedRebalanceProgressionInfo {
                    validator_list_index: validator_list_index as usize,
                    directed_stake_meta_index,
                    vote_account,
                });
            }
        }

        pregression_info
    }
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

/// Fetches the specific token balance for a given token mint address and wallet address.
///
/// Returns the balance in lamports (as u64). If the account doesn't exist
/// or has an error, returns 0.
///
/// # Example
///
/// ```no_run
/// use std::{str::FromStr, sync::Arc};
///
/// use solana_client::nonblocking::rpc_client::RpcClient;
/// use solana_sdk::pubkey::Pubkey;
/// use stakenet_sdk::utils::helpers::get_token_balance;
///
/// # async fn example() {
/// let client = Arc::new(RpcClient::new("https://api.mainnet-beta.solana.com".to_string()));
/// let token_mint_address = Pubkey::from_str("J1toso1uCk3RLmjorhTtrVwY9HJ7X8V9yYac6Y7kGCPn").unwrap();
/// let wallet_address = Pubkey::from_str("AVpEyxKqctAXiSxzgR6Zbe4P5cuZkabWAEhGY2j7QEaD").unwrap();
///
/// let balance = get_token_balance(client, &token_mint_address, &wallet_address).await.unwrap();
/// # }
/// ```
pub async fn get_token_balance(
    client: Arc<RpcClient>,
    token_mint_address: &Pubkey,
    wallet_address: &Pubkey,
) -> Result<u64, JitoInstructionError> {
    let token_account = get_associated_token_address(wallet_address, token_mint_address);
    let (token_balance, _) = match client.get_token_account_balance(&token_account).await {
        Ok(balance) => (balance.amount, balance.ui_amount.unwrap_or(0.0)),
        Err(_) => ("0".to_string(), 0.0),
    };

    let total_lamports: u64 = token_balance
        .parse::<u64>()
        .map_err(|e| JitoInstructionError::ParseError(e.to_string()))?;

    Ok(total_lamports)
}

/// Calculates the conversion rate from lamports in basis points.
///
/// The conversion rate represents how many lamports are equivalent to 10,000 tokens.
/// Formula: `(total_lamports * 10,000) / pool_token_supply`
///
/// # Example
///
/// ```
/// use stakenet_sdk::utils::helpers::calculate_conversion_rate_bps;
///
/// let total_lamports = 1_000_000_000;
/// let pool_token_supply = 100_000_000;
/// let rate = calculate_conversion_rate_bps(1_000_000_000, 100_000_000)?;
/// assert_eq!(rate, 100_000);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn calculate_conversion_rate_bps(
    total_lamports: u64,
    pool_token_supply: u64,
) -> Result<u64, JitoInstructionError> {
    if pool_token_supply == 0 {
        return Err(JitoInstructionError::Custom(
            "Pool token supply cannot be zero".to_string(),
        ));
    }

    (total_lamports as u128)
        .checked_mul(BASIS_POINTS_MAX as u128)
        .and_then(|n| n.checked_div(pool_token_supply as u128))
        .map(|n| n as u64)
        .ok_or(JitoInstructionError::ArithmeticError)
}

/// Aggregates validator target delegations from all tickets.
///
/// For each ticket and each validator preference, calculates the lamports to allocate
/// and aggregates them per validator.
///
/// # Example
///
/// ```no_run
/// use std::collections::HashMap;
///
/// use jito_steward::{constants::BASIS_POINTS_MAX, DirectedStakeTicket};
/// use solana_sdk::pubkey::Pubkey;
/// use stakenet_sdk::utils::helpers::aggregate_validator_targets;
///
/// let tickets = vec![/* ... */];
/// let balances = HashMap::new();
/// let targets = aggregate_validator_targets(&tickets, &balances, 100_000)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn aggregate_validator_targets(
    tickets: &[DirectedStakeTicket],
    jitosol_balances: &HashMap<Pubkey, u64>,
    conversion_rate_bps: u64,
) -> Result<HashMap<Pubkey, u64>, JitoInstructionError> {
    let mut validator_target_delegations: HashMap<Pubkey, u64> = HashMap::new();

    for ticket in tickets {
        let jitosol_balance = jitosol_balances
            .get(&ticket.ticket_update_authority)
            .copied()
            .unwrap_or(0);

        if jitosol_balance == 0 {
            continue;
        }

        for preference in &ticket.staker_preferences {
            // Skip default/empty vote pubkeys
            if preference.vote_pubkey == Pubkey::default() {
                continue;
            }

            let allocated_tokens = preference.get_allocation(jitosol_balance);
            let allocation_lamports = allocated_tokens
                .checked_mul(conversion_rate_bps as u128)
                .and_then(|n| n.checked_div(BASIS_POINTS_MAX as u128))
                .map(|n| n as u64)
                .ok_or(JitoInstructionError::ArithmeticError)?;

            validator_target_delegations
                .entry(preference.vote_pubkey)
                .and_modify(|total| *total = total.saturating_add(allocation_lamports))
                .or_insert(allocation_lamports);
        }
    }

    Ok(validator_target_delegations)
}

#[cfg(test)]
mod tests {
    use jito_steward::{utils::U8Bool, DirectedStakePreference};

    use super::*;

    fn create_tickets(
        authority1: Pubkey,
        authority2: Pubkey,
        validator1: Pubkey,
        validator2: Pubkey,
    ) -> Vec<DirectedStakeTicket> {
        fn create_ticket(
            authority: Pubkey,
            preferences: Vec<(Pubkey, u16)>,
        ) -> DirectedStakeTicket {
            let mut staker_preferences = [DirectedStakePreference {
                vote_pubkey: Pubkey::default(),
                stake_share_bps: 0,
                _padding0: [0; 94],
            }; 8];

            for (i, (vote_pubkey, stake_share_bps)) in preferences.into_iter().enumerate() {
                if i < 8 {
                    staker_preferences[i] = DirectedStakePreference {
                        vote_pubkey,
                        stake_share_bps,
                        _padding0: [0; 94],
                    };
                }
            }

            DirectedStakeTicket {
                num_preferences: 0,
                ticket_holder_is_protocol: U8Bool::from(true),
                ticket_update_authority: authority,
                staker_preferences,
                _padding0: [0; 125],
            }
        }

        vec![
            create_ticket(authority1, vec![(validator1, 6000), (validator2, 4000)]),
            create_ticket(authority2, vec![(validator1, 6000), (validator2, 4000)]),
        ]
    }

    #[test]
    fn test_calculate_conversion_rate_bps() {
        // Test basic conversion
        let rate = calculate_conversion_rate_bps(1_000_000_000, 100_000_000).unwrap();
        assert_eq!(rate, 100_000);

        // Test 1:1 ratio
        let rate = calculate_conversion_rate_bps(10_000, 10_000).unwrap();
        assert_eq!(rate, 10_000);

        // Test zero pool supply
        assert!(calculate_conversion_rate_bps(1_000_000, 0).is_err());
    }

    #[test]
    fn test_aggregate_validator_targets() {
        let validator1 = Pubkey::new_unique();
        let validator2 = Pubkey::new_unique();
        let authority1 = Pubkey::new_unique();
        let authority2 = Pubkey::new_unique();

        let mut balances = HashMap::new();
        balances.insert(authority1, 100_000_000);

        let tickets = create_tickets(authority1, authority2, validator1, validator2);
        let targets = aggregate_validator_targets(&tickets, &balances, 10_000).unwrap();

        assert_eq!(targets.len(), 2);
        assert_eq!(*targets.get(&validator1).unwrap(), 60_000_000);
        assert_eq!(*targets.get(&validator2).unwrap(), 40_000_000);
    }

    #[test]
    fn test_aggregate_multiple_tickets_same_validator() {
        let validator1 = Pubkey::new_unique();
        let authority1 = Pubkey::new_unique();
        let authority2 = Pubkey::new_unique();

        let mut balances = HashMap::new();
        balances.insert(authority1, 100_000_000);
        balances.insert(authority2, 50_000_000);

        let tickets = create_tickets(authority1, authority2, validator1, validator1);
        let targets = aggregate_validator_targets(&tickets, &balances, 10_000).unwrap();

        assert_eq!(targets.len(), 1);
        assert_eq!(*targets.get(&validator1).unwrap(), 150_000_000); // Sum of both
    }

    #[test]
    fn test_aggregate_skips_default_pubkey() {
        let authority1 = Pubkey::new_unique();
        let authority2 = Pubkey::new_unique();
        let validator1 = Pubkey::default();
        let validator2 = Pubkey::default();

        let tickets = create_tickets(authority1, authority2, validator1, validator2);

        let mut balances = HashMap::new();
        balances.insert(authority1, 100_000_000);

        let targets = aggregate_validator_targets(&tickets, &balances, 10_000).unwrap();

        assert_eq!(targets.len(), 0);
    }
}
