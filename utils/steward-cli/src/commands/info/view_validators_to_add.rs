use std::sync::Arc;

use anchor_lang::AccountDeserialize;
use anyhow::Result;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::borsh1::try_from_slice_unchecked;
use solana_sdk::{pubkey::Pubkey, stake::state::StakeStateV2};
use spl_stake_pool::{minimum_delegation, minimum_reserve_lamports};
use stakenet_sdk::utils::{
    accounts::{
        get_all_steward_accounts, get_all_steward_validator_accounts, get_all_validator_accounts,
    },
    helpers::check_stake_accounts,
    transactions::get_vote_accounts_with_retry,
};
use validator_history::{constants::MIN_VOTE_EPOCHS, ValidatorHistory};

use crate::commands::command_args::ViewValidatorsToAdd;

/// Why a candidate vote account was rejected, mirroring the keeper's checks in
/// `_handle_adding_validators`.
enum Ineligible {
    MissingHistoryAccount,
    NoHistoryEntries,
    MissingVotingEpochs,
    BelowMinimumStake { activated_stake_lamports: u64 },
}

impl std::fmt::Display for Ineligible {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingHistoryAccount => write!(f, "validator history account not found"),
            Self::NoHistoryEntries => write!(f, "validator history has no entries"),
            Self::MissingVotingEpochs => {
                write!(f, "missing epoch credits within minimum_voting_epochs")
            }
            Self::BelowMinimumStake {
                activated_stake_lamports,
            } => write!(
                f,
                "activated stake {activated_stake_lamports} below minimum"
            ),
        }
    }
}

pub async fn command_view_validators_to_add(
    args: ViewValidatorsToAdd,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
    validator_history_program_id: Pubkey,
) -> Result<()> {
    let steward_config = args.view_parameters.steward_config;
    let epoch = client.get_epoch_info().await?.epoch;

    let all_steward_accounts =
        get_all_steward_accounts(client, &program_id, &steward_config).await?;
    let all_steward_validator_accounts = get_all_steward_validator_accounts(
        client,
        &all_steward_accounts,
        &validator_history_program_id,
    )
    .await?;

    let active_vote_accounts = get_vote_accounts_with_retry(client, MIN_VOTE_EPOCHS, None).await?;
    let all_validator_accounts =
        get_all_validator_accounts(client, &active_vote_accounts, &validator_history_program_id)
            .await?;

    // Candidates are validators with a history account that are not already in the pool.
    let checks = check_stake_accounts(&all_validator_accounts, epoch);
    let in_pool = &all_steward_validator_accounts.all_history_vote_account_map;

    let minimum_voting_epochs = all_steward_accounts
        .config_account
        .parameters
        .minimum_voting_epochs;
    let minimum_stake_lamports = all_steward_accounts
        .config_account
        .parameters
        .minimum_stake_lamports;
    let start_epoch = epoch.saturating_sub(minimum_voting_epochs.saturating_sub(1));

    let mut eligible: Vec<Pubkey> = Vec::new();
    let mut rejected: Vec<(Pubkey, Ineligible)> = Vec::new();

    for (vote_account, check) in checks.iter() {
        if in_pool.contains_key(vote_account) || !check.has_history || check.has_stake_account {
            continue;
        }

        let raw_history_account = all_validator_accounts
            .all_history_vote_account_map
            .get(vote_account)
            .and_then(|account| account.as_ref());

        let Some(raw_history_account) = raw_history_account else {
            rejected.push((*vote_account, Ineligible::MissingHistoryAccount));
            continue;
        };

        let Ok(history_account) =
            ValidatorHistory::try_deserialize(&mut raw_history_account.data.as_slice())
        else {
            rejected.push((*vote_account, Ineligible::MissingHistoryAccount));
            continue;
        };

        let Some(entry) = history_account.history.last() else {
            rejected.push((*vote_account, Ineligible::NoHistoryEntries));
            continue;
        };

        if history_account
            .history
            .epoch_credits_range(start_epoch as u16, epoch as u16)
            .iter()
            .any(|entry| entry.is_none())
        {
            rejected.push((*vote_account, Ineligible::MissingVotingEpochs));
            continue;
        }

        if entry.activated_stake_lamports < minimum_stake_lamports {
            rejected.push((
                *vote_account,
                Ineligible::BelowMinimumStake {
                    activated_stake_lamports: entry.activated_stake_lamports,
                },
            ));
            continue;
        }

        eligible.push(*vote_account);
    }

    eligible.sort();
    rejected.sort_by_key(|(vote_account, _)| *vote_account);

    // Reserve math, matching `process_add_validator_to_pool` in spl-stake-pool.
    let reserve_stake_acc = &all_steward_accounts.reserve_stake_account;
    let stake_rent = client
        .get_minimum_balance_for_rent_exemption(StakeStateV2::size_of())
        .await?;
    let cluster_minimum_delegation = client.get_stake_minimum_delegation().await?;
    let reserve_floor =
        match try_from_slice_unchecked::<StakeStateV2>(reserve_stake_acc.data.as_slice()) {
            Ok(state) => state.meta().map(|meta| minimum_reserve_lamports(&meta)),
            Err(_) => None,
        }
        .unwrap_or(stake_rent);

    let cost_per_validator =
        minimum_delegation(cluster_minimum_delegation).saturating_add(stake_rent);
    let spendable_reserve = reserve_stake_acc.lamports.saturating_sub(reserve_floor);
    let affordable = spendable_reserve / cost_per_validator.max(1);
    let total_cost = cost_per_validator.saturating_mul(eligible.len() as u64);

    // Shortfall is measured against the absolute reserve balance the program demands. A reserve
    // below its own floor has a deficit that must be made up on top of the per-validator cost.
    let required_reserve = total_cost.saturating_add(reserve_floor);
    let shortfall = required_reserve.saturating_sub(reserve_stake_acc.lamports);

    println!("Epoch: {epoch}");
    println!("Steward config: {steward_config}");
    println!("Stake pool: {}", all_steward_accounts.stake_pool_address);
    println!(
        "Reserve stake: {}",
        all_steward_accounts.stake_pool_account.reserve_stake
    );
    println!();
    println!("minimum_voting_epochs: {minimum_voting_epochs}");
    println!("minimum_stake_lamports: {minimum_stake_lamports}");
    println!();
    println!("Reserve lamports:      {}", reserve_stake_acc.lamports);
    println!("Reserve floor:         {reserve_floor}");
    println!("Spendable reserve:     {spendable_reserve}");
    println!("Cost per validator:    {cost_per_validator}");
    println!("Validators affordable: {affordable}");
    if reserve_stake_acc.lamports < reserve_floor {
        println!(
            "NOTE: reserve is {} lamports BELOW its own rent-exempt floor; that deficit must be \
             funded on top of any per-validator cost.",
            reserve_floor.saturating_sub(reserve_stake_acc.lamports)
        );
    }
    println!();

    if args.verbose && !rejected.is_empty() {
        println!("Ineligible candidates ({}):", rejected.len());
        for (vote_account, reason) in rejected.iter() {
            println!("  {vote_account}  {reason}");
        }
        println!();
    }

    if eligible.is_empty() {
        println!("No validators are eligible to be added.");
        return Ok(());
    }

    println!("Eligible to add ({}):", eligible.len());
    for vote_account in eligible.iter() {
        println!("  {vote_account}");
    }
    println!();

    println!("Total reserve cost to add all: {total_cost}");
    if shortfall > 0 {
        println!(
            "Reserve SHORTFALL: {shortfall} lamports ({:.4} SOL) -- deposit this much into the reserve, or only the first {affordable} add(s) will succeed.",
            shortfall as f64 / 1e9
        );
    } else {
        println!("Reserve can fund all {} add(s).", eligible.len());
    }
    println!();

    println!("Run manually with:");
    for vote_account in eligible.iter().take(affordable.max(1) as usize) {
        println!(
            "  steward-cli --json-rpc-url $JSON_RPC_URL auto-add-validator-from-pool --steward-config {steward_config} --vote-account {vote_account}"
        );
    }

    Ok(())
}
