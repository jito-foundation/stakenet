use std::{
    ops::{Mul, Sub},
    sync::Arc,
};

use anchor_lang::{AccountDeserialize, AnchorDeserialize, InstructionData, ToAccountMetas};
use jito_steward::{
    stake_pool_utils::{StakePool, ValidatorList},
    StewardStateEnum,
};
use log::{error, info};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::borsh1::try_from_slice_unchecked;
use solana_program::instruction::Instruction;
use solana_pubkey::Pubkey as SolanaPubkey;
#[allow(deprecated)]
use solana_sdk::{
    pubkey::Pubkey,
    signature::Keypair,
    signer::Signer,
    stake,
    stake::{
        instruction::deactivate_delinquent_stake,
        state::StakeStateV2,
        tools::{acceptable_reference_epoch_credits, eligible_for_deactivate_delinquent},
    },
    system_program,
};
use solana_vote_interface::state::VoteStateV4;
use spl_associated_token_account::{
    get_associated_token_address, instruction::create_associated_token_account_idempotent,
};
#[allow(deprecated)]
use spl_stake_pool::{
    find_transient_stake_program_address, find_withdraw_authority_program_address,
    instruction::deposit_sol,
    instruction::{
        cleanup_removed_validator_entries, update_stake_pool_balance, update_validator_list_balance,
    },
    minimum_delegation, minimum_reserve_lamports,
    state::{StakeStatus, ValidatorStakeInfo},
    MAX_VALIDATORS_TO_UPDATE,
};
use stakenet_sdk::{
    models::{
        aggregate_accounts::{AllStewardAccounts, AllValidatorAccounts},
        errors::{JitoSendTransactionError, JitoTransactionError},
        submit_stats::SubmitStats,
    },
    utils::{
        accounts::{
            get_cluster_history_address, get_directed_stake_meta, get_directed_stake_meta_address,
            get_stake_address, get_steward_state_account, get_transient_stake_address,
            get_validator_history_address,
        },
        helpers::{
            check_stake_accounts, get_unprogressed_validators, DirectedRebalanceProgressionInfo,
        },
        transactions::{configure_instruction, package_instructions, submit_packaged_transactions},
    },
};
use validator_history::ValidatorHistory;

/// Upper bound on how many validators the keeper will add to the pool in a single crank.
const MAX_VALIDATORS_TO_ADD_PER_CRANK: usize = 10;

/// Lamports the keeper always keeps in its own wallet for transaction fees. Never spent on
/// topping up the pool reserve.
const KEEPER_WALLET_RESERVE_LAMPORTS: u64 = 1_000_000_000;

pub fn _get_update_stake_pool_ixs(
    program_id: &Pubkey,
    stake_pool: &StakePool,
    validator_list: &ValidatorList,
    stake_pool_address: &Pubkey,
    all_validator_accounts: &AllValidatorAccounts,
    no_merge: bool,
    epoch: u64,
) -> (Vec<Instruction>, Vec<Instruction>, Vec<Instruction>) {
    let (withdraw_authority, _) =
        find_withdraw_authority_program_address(program_id, stake_pool_address);

    let mut update_list_instructions: Vec<Instruction> = vec![];
    let mut start_index = 0;
    for validator_info_chunk in validator_list.validators.chunks(MAX_VALIDATORS_TO_UPDATE) {
        let should_update = validator_info_chunk
            .iter()
            .any(|info: &ValidatorStakeInfo| {
                if u64::from(info.last_update_epoch) < epoch {
                    true
                } else {
                    match StakeStatus::try_from(info.status).unwrap() {
                        StakeStatus::DeactivatingValidator => true,
                        _ => false,
                        // StakeStatus::DeactivatingAll => false,
                        // StakeStatus::Active => false,
                        // StakeStatus::DeactivatingTransient => false,
                        // StakeStatus::ReadyForRemoval => false,
                    }
                }
            });

        if should_update {
            let validator_vote_accounts = validator_info_chunk
                .iter()
                .map(|v| v.vote_account_address)
                .collect::<Vec<Pubkey>>();

            #[allow(deprecated)]
            update_list_instructions.push(update_validator_list_balance(
                program_id,
                stake_pool_address,
                &withdraw_authority,
                &stake_pool.validator_list,
                &stake_pool.reserve_stake,
                validator_list,
                &validator_vote_accounts,
                start_index,
                no_merge,
            ));
        }
        // Advance no matter what
        start_index = start_index.saturating_add(MAX_VALIDATORS_TO_UPDATE as u32);
    }

    let mut deactivate_delinquent_instructions: Vec<Instruction> = vec![];
    let reference_vote_account = validator_list.validators.iter().find(|validator_info| {
        let raw_vote_account = all_validator_accounts
            .all_vote_account_map
            .get(&validator_info.vote_account_address)
            .expect("Vote account not found");

        let Some(raw_vote_account) = raw_vote_account else {
            return false;
        };

        let vote_pubkey =
            SolanaPubkey::new_from_array(validator_info.vote_account_address.to_bytes());
        let vote_account = match VoteStateV4::deserialize(&raw_vote_account.data, &vote_pubkey) {
            Ok(v) => v,
            Err(e) => {
                error!("Failed to deserialize vote account vote_account={vote_pubkey}: {e:?}");
                return false;
            }
        };

        acceptable_reference_epoch_credits(&vote_account.epoch_credits, epoch)
    });

    for validator_info in validator_list.validators.iter() {
        let raw_vote_account = all_validator_accounts
            .all_vote_account_map
            .get(&validator_info.vote_account_address)
            .expect("Vote account not found");

        let raw_stake_account = all_validator_accounts
            .all_stake_account_map
            .get(&validator_info.vote_account_address)
            .expect("Stake account not found");

        let raw_stake_account = raw_stake_account.as_ref().filter(|stake_account| {
            let is_stake_account = stake_account.owner == stake::program::id();
            if !is_stake_account {
                error!(
                    "Stake account is not owned by the stake program vote_account={} owner={}",
                    validator_info.vote_account_address, stake_account.owner
                );
            }
            is_stake_account
        });

        let should_deactivate = match (raw_vote_account, raw_stake_account) {
            (None, Some(_)) => true,
            (Some(raw_vote_account), Some(raw_stake_account)) => {
                let stake_account =
                    match StakeStateV2::deserialize(&mut raw_stake_account.data.as_slice()) {
                        Ok(s) => s,
                        Err(e) => {
                            error!(
                                "Failed to deserialize stake account vote_account={}: {e:?}",
                                validator_info.vote_account_address
                            );
                            continue;
                        }
                    };

                let vote_pubkey =
                    SolanaPubkey::new_from_array(validator_info.vote_account_address.to_bytes());
                let vote_account =
                    match VoteStateV4::deserialize(&raw_vote_account.data, &vote_pubkey) {
                        Ok(v) => v,
                        Err(e) => {
                            error!(
                            "Failed to deserialize vote account vote_account={vote_pubkey}: {e:?}"
                        );
                            continue;
                        }
                    };

                match stake_account {
                    StakeStateV2::Stake(_meta, stake, _stake_flags) => {
                        if stake.delegation.deactivation_epoch != u64::MAX {
                            false
                        } else {
                            eligible_for_deactivate_delinquent(&vote_account.epoch_credits, epoch)
                        }
                    }
                    _ => {
                        error!(
                            "Stake account is not in Stake state vote_account={}",
                            validator_info.vote_account_address
                        );
                        false
                    }
                }
            }
            (_, None) => false,
        };

        if should_deactivate && reference_vote_account.is_some() {
            let stake_account =
                get_stake_address(&validator_info.vote_account_address, stake_pool_address);

            let ix = deactivate_delinquent_stake(
                &stake_account,
                &validator_info.vote_account_address,
                &reference_vote_account.unwrap().vote_account_address,
            );

            deactivate_delinquent_instructions.push(ix);
        }
    }

    if reference_vote_account.is_none() {
        info!(
            "No vote account satisfies the stake program's reference requirements, skipping delinquent deactivation epoch={epoch}"
        );
    }

    let final_instructions = vec![
        update_stake_pool_balance(
            program_id,
            stake_pool_address,
            &withdraw_authority,
            &stake_pool.validator_list,
            &stake_pool.reserve_stake,
            &stake_pool.manager_fee_account,
            &stake_pool.pool_mint,
            &stake_pool.token_program_id,
        ),
        cleanup_removed_validator_entries(
            program_id,
            stake_pool_address,
            &stake_pool.validator_list,
        ),
    ];
    (
        update_list_instructions,
        deactivate_delinquent_instructions,
        final_instructions,
    )
}

async fn _update_pool(
    payer: &Arc<Keypair>,
    client: &Arc<RpcClient>,
    epoch: u64,
    all_steward_accounts: &AllStewardAccounts,
    all_validator_accounts: &AllValidatorAccounts,
    priority_fee: Option<u64>,
) -> Result<SubmitStats, JitoTransactionError> {
    let mut stats = SubmitStats::default();

    let (update_ixs, deactivate_delinquent_ixs, cleanup_ixs) = _get_update_stake_pool_ixs(
        &spl_stake_pool::ID,
        &all_steward_accounts.stake_pool_account,
        &all_steward_accounts.validator_list_account,
        &all_steward_accounts.stake_pool_address,
        all_validator_accounts,
        false,
        epoch,
    );

    info!("Updating stake pool balances");
    let update_txs_to_run =
        package_instructions(&update_ixs, 1, priority_fee, Some(1_400_000), None);
    let update_stats =
        submit_packaged_transactions(client, update_txs_to_run, payer, Some(50), None).await?;

    stats.combine(&update_stats);

    // TODO fix
    info!("Deactivating delinquent validators");
    // for ix in deactivate_delinquent_ixs {
    //     let tx = Transaction::new_signed_with_payer(&[ix], Some(&payer.pubkey()), &[&payer]);
    //     let tx = client
    //         .send_and_confirm_transaction_with_spinner_and_config(&tx)
    //         .await?;
    //     stats.add_tx(&tx);
    // }
    let deactivate_txs_to_run = package_instructions(
        &deactivate_delinquent_ixs,
        1,
        priority_fee,
        Some(1_400_000),
        None,
    );
    let update_stats =
        submit_packaged_transactions(client, deactivate_txs_to_run, payer, Some(50), None).await?;

    stats.combine(&update_stats);

    info!("Cleaning up removed validator entries");
    let cleanup_txs_to_run =
        package_instructions(&cleanup_ixs, 1, priority_fee, Some(1_400_000), None);
    let cleanup_stats =
        submit_packaged_transactions(client, cleanup_txs_to_run, payer, Some(50), None).await?;

    stats.combine(&cleanup_stats);

    Ok(stats)
}

async fn _handle_instant_removal_validators(
    payer: &Arc<Keypair>,
    client: &Arc<RpcClient>,
    program_id: &Pubkey,
    all_steward_accounts: &AllStewardAccounts,
    priority_fee: Option<u64>,
) -> Result<SubmitStats, JitoTransactionError> {
    let mut num_validators = all_steward_accounts.state_account.state.num_pool_validators;
    let mut validators_to_remove = all_steward_accounts
        .state_account
        .state
        .validators_for_immediate_removal;

    let mut stats = SubmitStats::default();

    while validators_to_remove.count() != 0 {
        let mut validator_index_to_remove = None;
        for i in 0..all_steward_accounts.validator_list_account.validators.len() as u64 {
            if validators_to_remove.get(i as usize).map_err(|e| {
                JitoTransactionError::Custom(format!(
                    "Error fetching bitmask index for immediate removed validator: {i}/{num_validators} - {e}"
                ))
            })? {
                validator_index_to_remove = Some(i);
                break;
            }
        }

        info!("Submitting instant removal validator_list_index={validator_index_to_remove:?}");

        let directed_stake_meta =
            get_directed_stake_meta_address(&all_steward_accounts.config_address, program_id);

        let ix = Instruction {
            program_id: *program_id,
            accounts: jito_steward::accounts::InstantRemoveValidator {
                config: all_steward_accounts.config_address,
                state_account: all_steward_accounts.state_address,
                validator_list: all_steward_accounts.validator_list_address,
                stake_pool: all_steward_accounts.stake_pool_address,
                directed_stake_meta,
            }
            .to_account_metas(None),
            data: jito_steward::instruction::InstantRemoveValidator {
                validator_index_to_remove: validator_index_to_remove.unwrap(),
            }
            .data(),
        };

        let configured_ix = configure_instruction(&[ix], priority_fee, Some(1_400_000), None);

        let new_stats =
            submit_packaged_transactions(client, vec![configured_ix], payer, Some(50), None)
                .await?;

        stats.combine(&new_stats);

        if stats.errors > 0 {
            return Ok(stats);
        }

        // NOTE: This is the only time an account is fetched
        // in any of these cranking functions
        let updated_state_account =
            get_steward_state_account(client, program_id, &all_steward_accounts.config_address)
                .await
                .unwrap();

        num_validators = updated_state_account.state.num_pool_validators;
        validators_to_remove = updated_state_account.state.validators_for_immediate_removal;
    }

    Ok(stats)
}

/// Per-validator costs that the stake pool program charges against the reserve stake account.
struct ReserveCosts {
    /// Rent-exempt minimum for a stake account.
    stake_rent: u64,

    /// `minimum_delegation(get_minimum_delegation())` as the stake pool program computes it.
    /// This is `max(cluster minimum delegation, MINIMUM_ACTIVE_STAKE)`, which is 1 SOL on
    /// clusters that have raised the stake program minimum, not the 1_000_000 lamport floor.
    minimum_delegation: u64,

    /// Lamports the stake pool program refuses to let the reserve drop below, i.e. the reserve's
    /// own rent-exempt reserve. See `minimum_reserve_lamports` in spl-stake-pool.
    reserve_floor: u64,

    /// Current reserve stake account balance.
    reserve_lamports: u64,
}

impl ReserveCosts {
    /// Cost of adding one validator to the pool, matching the check in
    /// `spl_stake_pool::processor::process_add_validator_to_pool`:
    /// `minimum_delegation(minimum) + rent.minimum_balance(stake_space)`.
    fn add_validator_cost(&self) -> u64 {
        self.minimum_delegation.saturating_add(self.stake_rent)
    }

    /// Total reserve balance required to add `count` validators, i.e. the balance at which the
    /// pool's `reserve_lamports - required_lamports < minimum_lamports` check stops failing.
    fn required_reserve_for(&self, count: u64) -> u64 {
        self.add_validator_cost()
            .saturating_mul(count)
            .saturating_add(self.reserve_floor)
    }

    /// Lamports that must be deposited for `count` adds to succeed, 0 if the reserve already
    /// covers them.
    ///
    /// This deliberately subtracts the reserve balance last. A reserve sitting *below* its own
    /// floor has a deficit that an intermediate saturating subtraction would clamp to zero,
    /// under-funding the deposit by exactly that deficit.
    #[cfg_attr(not(test), allow(dead_code))]
    fn shortfall_for(&self, count: u64) -> u64 {
        self.required_reserve_for(count)
            .saturating_sub(self.reserve_lamports)
    }

    /// How much the reserve currently sits *below* its own floor. Usually 0.
    fn deficit(&self) -> u64 {
        self.reserve_floor.saturating_sub(self.reserve_lamports)
    }

    /// How many validators the reserve can fund on its own, with no deposits at all.
    fn reserve_only_count(&self) -> u64 {
        self.reserve_lamports
            .saturating_sub(self.reserve_floor)
            .checked_div(self.add_validator_cost())
            .unwrap_or(0)
    }

    /// Per-transaction deposit that lets an add succeed **regardless of the order** in which the
    /// batch's transactions land.
    ///
    /// Transactions are submitted concurrently, so a transaction cannot assume any sibling's
    /// deposit has already settled. Depositing `cost + deficit` makes each one self-sufficient:
    /// the worst case is landing first, against the untouched reserve, where
    /// `reserve + deposit - cost == max(reserve, floor)` still clears the floor. Because the
    /// deposit is at least `cost`, every later landing only has more headroom.
    fn self_funded_deposit(&self) -> u64 {
        self.add_validator_cost().saturating_add(self.deficit())
    }
}

/// Reads the reserve stake account and the cluster's minimum delegation so that reserve funding
/// can be sized the same way the stake pool program sizes its own checks.
async fn _get_reserve_costs(
    client: &Arc<RpcClient>,
    all_steward_accounts: &AllStewardAccounts,
) -> Result<ReserveCosts, JitoTransactionError> {
    let reserve_stake_acc = client
        .get_account(&all_steward_accounts.stake_pool_account.reserve_stake)
        .await?;

    let stake_rent = client
        .get_minimum_balance_for_rent_exemption(StakeStateV2::size_of())
        .await?;

    let cluster_minimum_delegation = client.get_stake_minimum_delegation().await?;

    let reserve_floor = match try_from_slice_unchecked::<StakeStateV2>(
        reserve_stake_acc.data.as_slice(),
    ) {
        Ok(state) => match state.meta() {
            Some(meta) => minimum_reserve_lamports(&meta),
            None => {
                error!("Reserve stake account has no Meta, falling back to stake rent as floor");
                stake_rent
            }
        },
        Err(e) => {
            error!("Failed to deserialize reserve stake account: {e:?}, falling back to stake rent as floor");
            stake_rent
        }
    };

    Ok(ReserveCosts {
        stake_rent,
        minimum_delegation: minimum_delegation(cluster_minimum_delegation),
        reserve_floor,
        reserve_lamports: reserve_stake_acc.lamports,
    })
}

/// Builds the instructions needed to top the reserve up by `amount` lamports via `DepositSol`.
fn _build_reserve_deposit_ixs(
    payer: &Arc<Keypair>,
    all_steward_accounts: &AllStewardAccounts,
    amount: u64,
) -> Vec<Instruction> {
    if amount == 0 {
        return vec![];
    }

    let acc_token_address = get_associated_token_address(
        &payer.pubkey(),
        &all_steward_accounts.stake_pool_account.pool_mint,
    );

    vec![
        create_associated_token_account_idempotent(
            &payer.pubkey(),
            &payer.pubkey(),
            &all_steward_accounts.stake_pool_account.pool_mint,
            &spl_token::id(),
        ),
        deposit_sol(
            &spl_stake_pool::id(),
            &all_steward_accounts.stake_pool_address,
            &all_steward_accounts.stake_pool_withdraw_authority,
            &all_steward_accounts.stake_pool_account.reserve_stake,
            &payer.pubkey(),
            &acc_token_address,
            &all_steward_accounts.stake_pool_account.manager_fee_account,
            &acc_token_address,
            &all_steward_accounts.stake_pool_account.pool_mint,
            &spl_token::id(),
            amount,
        ),
    ]
}

#[allow(clippy::too_many_arguments)]
async fn _handle_adding_validators(
    payer: &Arc<Keypair>,
    client: &Arc<RpcClient>,
    program_id: &Pubkey,
    epoch: u64,
    all_steward_accounts: &AllStewardAccounts,
    all_steward_validator_accounts: &AllValidatorAccounts,
    all_validator_accounts: &AllValidatorAccounts,
    priority_fee: Option<u64>,
) -> Result<SubmitStats, JitoTransactionError> {
    let mut keys_to_add: Vec<&Pubkey> = vec![];
    all_validator_accounts
        .all_history_vote_account_map
        .keys()
        .for_each(|key| {
            if !all_steward_validator_accounts
                .all_history_vote_account_map
                .keys()
                .any(|k| k == key)
            {
                keys_to_add.push(key);
            }
        });

    let mut accounts_to_check: AllValidatorAccounts = AllValidatorAccounts::default();
    all_validator_accounts
        .all_history_vote_account_map
        .keys()
        .for_each(|key| {
            if keys_to_add.contains(&key) {
                accounts_to_check.all_history_vote_account_map.insert(
                    *key,
                    all_validator_accounts
                        .all_history_vote_account_map
                        .get(key)
                        .unwrap()
                        .clone(),
                );
                accounts_to_check.all_stake_account_map.insert(
                    *key,
                    all_validator_accounts
                        .all_stake_account_map
                        .get(key)
                        .unwrap()
                        .clone(),
                );
                accounts_to_check.all_vote_account_map.insert(
                    *key,
                    all_validator_accounts
                        .all_vote_account_map
                        .get(key)
                        .unwrap()
                        .clone(),
                );
            }
        });

    let checks = check_stake_accounts(&accounts_to_check, epoch);

    let mut good_vote_accounts = checks
        .iter()
        .filter_map(|(vote_address, check)| {
            if check.has_history && !check.has_stake_account {
                let raw_history_account = all_validator_accounts
                    .all_history_vote_account_map
                    .get(vote_address)
                    .unwrap();

                match raw_history_account {
                    Some(raw_history_account) => {
                        let history_account = ValidatorHistory::try_deserialize(
                            &mut raw_history_account.data.as_slice(),
                        )
                        .ok()
                        .unwrap();

                        let start_epoch = epoch.saturating_sub(
                            all_steward_accounts
                                .config_account
                                .parameters
                                .minimum_voting_epochs
                                .saturating_sub(1),
                        );
                        if let Some(entry) = history_account.history.last() {
                            // Steward requires that validators have been active for last minimum_voting_epochs epochs
                            if history_account
                                .history
                                .epoch_credits_range(start_epoch as u16, epoch as u16)
                                .iter()
                                .any(|entry| entry.is_none())
                            {
                                return None;
                            }
                            if entry.activated_stake_lamports
                                < all_steward_accounts
                                    .config_account
                                    .parameters
                                    .minimum_stake_lamports
                            {
                                return None;
                            }
                        } else {
                            info!(
                                "Skipping validator below liveness minimum vote_account={vote_address}"
                            );
                            return None;
                        }
                    }
                    _ => {
                        return None;
                    }
                }

                Some(*vote_address)
            } else {
                None
            }
        })
        .collect::<Vec<Pubkey>>();

    if good_vote_accounts.is_empty() {
        return Ok(SubmitStats::default());
    }

    let reserve_costs = _get_reserve_costs(client, all_steward_accounts).await?;

    if good_vote_accounts.len() > MAX_VALIDATORS_TO_ADD_PER_CRANK {
        info!(
            "Capping validators to add step=auto_add_validators eligible={} cap={MAX_VALIDATORS_TO_ADD_PER_CRANK}",
            good_vote_accounts.len()
        );
        good_vote_accounts.truncate(MAX_VALIDATORS_TO_ADD_PER_CRANK);
    }

    let payer_balance = client.get_balance(&payer.pubkey()).await?;
    let payer_spendable = payer_balance.saturating_sub(KEEPER_WALLET_RESERVE_LAMPORTS);

    let free_adds = reserve_costs.reserve_only_count() as usize;
    let self_funded_deposit = reserve_costs.self_funded_deposit();
    let payer_funded_adds = payer_spendable
        .checked_div(self_funded_deposit.max(1))
        .unwrap_or(0) as usize;

    let fundable = free_adds.saturating_add(payer_funded_adds);
    if fundable == 0 {
        error!(
            "Skipping all validator adds, reserve and payer cannot fund any step=auto_add_validators eligible={} reserve_lamports={} reserve_floor={} payer_spendable={payer_spendable}",
            good_vote_accounts.len(),
            reserve_costs.reserve_lamports,
            reserve_costs.reserve_floor
        );
        return Ok(SubmitStats::default());
    }

    if good_vote_accounts.len() > fundable {
        info!(
            "Limiting validator adds to fundable capacity step=auto_add_validators eligible={} fundable={fundable} free_adds={free_adds} reserve_lamports={} reserve_floor={} payer_spendable={payer_spendable}",
            good_vote_accounts.len(),
            reserve_costs.reserve_lamports,
            reserve_costs.reserve_floor
        );
        good_vote_accounts.truncate(fundable);
    }

    let txs_to_run = good_vote_accounts
        .iter()
        .enumerate()
        .map(|(idx, vote_account)| {
            let history_account =
                get_validator_history_address(vote_account, &validator_history::id());

            let stake_address =
                get_stake_address(vote_account, &all_steward_accounts.stake_pool_address);

            let deposit = if idx < free_adds {
                0
            } else {
                self_funded_deposit
            };
            let mut ixs = _build_reserve_deposit_ixs(payer, all_steward_accounts, deposit);

            ixs.push(Instruction {
                program_id: *program_id,
                accounts: jito_steward::accounts::AutoAddValidator {
                    config: all_steward_accounts.config_address,
                    steward_state: all_steward_accounts.state_address,
                    stake_pool_program: spl_stake_pool::id(),
                    stake_pool: all_steward_accounts.stake_pool_address,
                    validator_history_account: history_account,
                    withdraw_authority: all_steward_accounts.stake_pool_withdraw_authority,
                    validator_list: all_steward_accounts.validator_list_address,
                    reserve_stake: all_steward_accounts.stake_pool_account.reserve_stake,
                    stake_account: stake_address,
                    vote_account: *vote_account,
                    system_program: system_program::id(),
                    stake_program: stake::program::id(),
                    rent: solana_sdk::sysvar::rent::id(),
                    clock: solana_sdk::sysvar::clock::id(),
                    stake_history: solana_sdk::sysvar::stake_history::id(),
                    stake_config: stake::config::ID,
                }
                .to_account_metas(None),
                data: jito_steward::instruction::AutoAddValidatorToPool {}.data(),
            });

            configure_instruction(&ixs, priority_fee, Some(1_400_000), None)
        })
        .collect::<Vec<Vec<Instruction>>>();

    info!(
        "Submitting transactions step=auto_add_validators validators={} transactions={}",
        good_vote_accounts.len(),
        txs_to_run.len()
    );

    let stats = submit_packaged_transactions(client, txs_to_run, payer, Some(50), None).await?;

    Ok(stats)
}

async fn _handle_delinquent_validators(
    payer: &Arc<Keypair>,
    client: &Arc<RpcClient>,
    program_id: &Pubkey,
    epoch: u64,
    all_steward_accounts: &AllStewardAccounts,
    all_steward_validator_accounts: &AllValidatorAccounts,
    priority_fee: Option<u64>,
) -> Result<SubmitStats, JitoTransactionError> {
    let checks = check_stake_accounts(all_steward_validator_accounts, epoch);

    let bad_vote_accounts = checks
        .iter()
        .filter_map(|(vote_account, check)| {
            if !check.has_history || check.is_deactivated || !check.has_vote_account {
                Some(*vote_account)
            } else {
                None
            }
        })
        .collect::<Vec<Pubkey>>();

    let ixs_to_run = bad_vote_accounts
        .iter()
        .filter_map(|vote_account| {
            let validator_index = all_steward_accounts
                .validator_list_account
                .validators
                .iter()
                .position(|v| v.vote_account_address == *vote_account)
                .expect("Cannot find vote account in Validator List");

            let history_account =
                get_validator_history_address(vote_account, &validator_history::id());

            let stake_address =
                get_stake_address(vote_account, &all_steward_accounts.stake_pool_address);

            let raw_stake_account = all_steward_validator_accounts
                .all_stake_account_map
                .get(vote_account)
                .and_then(|stake_account| stake_account.as_ref());
            let has_valid_stake_account = match raw_stake_account {
                Some(stake_account) if stake_account.owner == stake::program::id() => matches!(
                    StakeStateV2::deserialize(&mut stake_account.data.as_slice()),
                    Ok(StakeStateV2::Stake(..))
                ),
                _ => false,
            };
            if !has_valid_stake_account {
                error!(
                    "Cannot auto remove validator without a delegated stake account vote_account={vote_account} stake_account={stake_address}"
                );
                return None;
            }

            let transient_stake_address = get_transient_stake_address(
                vote_account,
                &all_steward_accounts.stake_pool_address,
                &all_steward_accounts.validator_list_account,
                validator_index,
            )?;

            if all_steward_accounts
                .state_account
                .state
                .validators_to_remove
                .get(validator_index)
                .expect("Could not find validator index in validators_to_remove")
            {
                return None;
            }

            Some(Instruction {
                program_id: *program_id,
                accounts: jito_steward::accounts::AutoRemoveValidator {
                    config: all_steward_accounts.config_address,
                    state_account: all_steward_accounts.state_address,
                    stake_pool_program: spl_stake_pool::id(),
                    stake_pool: all_steward_accounts.stake_pool_address,
                    validator_history_account: history_account,
                    withdraw_authority: all_steward_accounts.stake_pool_withdraw_authority,
                    validator_list: all_steward_accounts.validator_list_address,
                    reserve_stake: all_steward_accounts.stake_pool_account.reserve_stake,
                    stake_account: stake_address,
                    transient_stake_account: transient_stake_address,
                    vote_account: *vote_account,
                    system_program: system_program::id(),
                    stake_program: stake::program::id(),
                    rent: solana_sdk::sysvar::rent::id(),
                    clock: solana_sdk::sysvar::clock::id(),
                    stake_history: solana_sdk::sysvar::stake_history::id(),
                    stake_config: stake::config::ID,
                }
                .to_account_metas(None),
                data: jito_steward::instruction::AutoRemoveValidatorFromPool {
                    validator_list_index: validator_index as u64,
                }
                .data(),
            })
        })
        .collect::<Vec<Instruction>>();

    let txs_to_run = package_instructions(&ixs_to_run, 1, priority_fee, Some(1_400_000), None);

    info!(
        "Submitting transactions step=auto_remove_validators instructions={} transactions={}",
        ixs_to_run.len(),
        txs_to_run.len()
    );

    let stats = submit_packaged_transactions(client, txs_to_run, payer, Some(50), None).await?;
    // let stats = submit_packaged_transactions(client, txs_to_run, payer, Some(1), None).await?;

    Ok(stats)
}

async fn _handle_epoch_maintenance(
    payer: &Arc<Keypair>,
    client: &Arc<RpcClient>,
    program_id: &Pubkey,
    epoch: u64,
    all_steward_accounts: &AllStewardAccounts,
    priority_fee: Option<u64>,
) -> Result<SubmitStats, JitoTransactionError> {
    let mut current_epoch = epoch;
    let mut state_epoch = all_steward_accounts.state_account.state.current_epoch;
    let mut num_validators = all_steward_accounts.state_account.state.num_pool_validators;
    let mut validators_to_remove = all_steward_accounts
        .state_account
        .state
        .validators_to_remove;

    let mut stats = SubmitStats::default();

    while state_epoch != current_epoch {
        let mut validator_index_to_remove = None;
        for i in 0..num_validators {
            if validators_to_remove.get(i as usize).map_err(|e| {
                JitoTransactionError::Custom(format!(
                    "Error fetching bitmask index for removed validator: {i}/{num_validators} - {e}"
                ))
            })? {
                validator_index_to_remove = Some(i);
                break;
            }
        }

        info!(
            "Running epoch maintenance state_epoch={state_epoch} current_epoch={current_epoch} validator_index_to_remove={validator_index_to_remove:?}"
        );

        let directed_stake_meta =
            get_directed_stake_meta_address(&all_steward_accounts.config_address, program_id);

        let ix = Instruction {
            program_id: *program_id,
            accounts: jito_steward::accounts::EpochMaintenance {
                config: all_steward_accounts.config_address,
                state_account: all_steward_accounts.state_address,
                validator_list: all_steward_accounts.validator_list_address,
                stake_pool: all_steward_accounts.stake_pool_address,
                directed_stake_meta,
            }
            .to_account_metas(None),
            data: jito_steward::instruction::EpochMaintenance {
                validator_index_to_remove,
            }
            .data(),
        };

        let cu = validator_index_to_remove.map(|_| 1_400_000);
        let configured_ix = configure_instruction(&[ix], priority_fee, cu, None);

        let new_stats =
            submit_packaged_transactions(client, vec![configured_ix], payer, Some(50), None)
                .await?;

        stats.combine(&new_stats);

        if stats.errors > 0 {
            return Ok(stats);
        }

        // NOTE: This is the only time an account is fetched
        // in any of these cranking functions
        let updated_state_account =
            get_steward_state_account(client, program_id, &all_steward_accounts.config_address)
                .await
                .unwrap();

        num_validators = updated_state_account.state.num_pool_validators;
        validators_to_remove = updated_state_account.state.validators_to_remove;
        state_epoch = updated_state_account.state.current_epoch;
        current_epoch = client.get_epoch_info().await?.epoch;

        info!(
            "Epoch maintenance progressed state_epoch={state_epoch} current_epoch={current_epoch}"
        );
    }

    Ok(stats)
}

async fn _handle_compute_score(
    payer: &Arc<Keypair>,
    client: &Arc<RpcClient>,
    program_id: &Pubkey,
    all_steward_accounts: &AllStewardAccounts,
    priority_fee: Option<u64>,
) -> Result<SubmitStats, JitoTransactionError> {
    let validator_history_program_id = validator_history::id();
    let cluster_history: Pubkey = get_cluster_history_address(&validator_history_program_id);

    let validators_to_run =
        get_unprogressed_validators(all_steward_accounts, &validator_history_program_id);

    let ixs_to_run = validators_to_run
        .iter()
        .map(|validator_info| Instruction {
            program_id: *program_id,
            accounts: jito_steward::accounts::ComputeScore {
                config: all_steward_accounts.config_address,
                state_account: all_steward_accounts.state_address,
                validator_history: validator_info.history_account,
                validator_list: all_steward_accounts.validator_list_address,
                cluster_history,
            }
            .to_account_metas(None),
            data: jito_steward::instruction::ComputeScore {
                validator_list_index: validator_info.index as u64,
            }
            .data(),
        })
        .collect::<Vec<Instruction>>();

    let txs_to_run = package_instructions(&ixs_to_run, 5, priority_fee, Some(1_400_000), None);

    info!(
        "Submitting transactions step=compute_score instructions={} transactions={}",
        ixs_to_run.len(),
        txs_to_run.len()
    );

    let stats = submit_packaged_transactions(client, txs_to_run, payer, Some(50), None).await?;

    Ok(stats)
}

async fn _handle_compute_delegations(
    payer: &Arc<Keypair>,
    client: &Arc<RpcClient>,
    program_id: &Pubkey,
    all_steward_accounts: &AllStewardAccounts,
    priority_fee: Option<u64>,
) -> Result<SubmitStats, JitoTransactionError> {
    let ix = Instruction {
        program_id: *program_id,
        accounts: jito_steward::accounts::ComputeDelegations {
            config: all_steward_accounts.config_address,
            state_account: all_steward_accounts.state_address,
            validator_list: all_steward_accounts.validator_list_address,
        }
        .to_account_metas(None),
        data: jito_steward::instruction::ComputeDelegations {}.data(),
    };

    let configured_ix = configure_instruction(&[ix], priority_fee, None, None);

    let stats =
        submit_packaged_transactions(client, vec![configured_ix], payer, Some(50), None).await?;

    Ok(stats)
}

async fn _handle_idle(
    payer: &Arc<Keypair>,
    client: &Arc<RpcClient>,
    program_id: &Pubkey,
    all_steward_accounts: &AllStewardAccounts,
    priority_fee: Option<u64>,
) -> Result<SubmitStats, JitoTransactionError> {
    let ix = Instruction {
        program_id: *program_id,
        accounts: jito_steward::accounts::Idle {
            config: all_steward_accounts.config_address,
            state_account: all_steward_accounts.state_address,
            validator_list: all_steward_accounts.validator_list_address,
        }
        .to_account_metas(None),
        data: jito_steward::instruction::Idle {}.data(),
    };

    let configured_ix = configure_instruction(&[ix], priority_fee, None, None);

    let stats =
        submit_packaged_transactions(client, vec![configured_ix], payer, Some(50), None).await?;

    Ok(stats)
}

async fn _handle_compute_instant_unstake(
    payer: &Arc<Keypair>,
    client: &Arc<RpcClient>,
    program_id: &Pubkey,
    all_steward_accounts: &AllStewardAccounts,
    priority_fee: Option<u64>,
) -> Result<SubmitStats, JitoTransactionError> {
    let validator_history_program_id = validator_history::id();
    let cluster_history: Pubkey = get_cluster_history_address(&validator_history_program_id);

    let validators_to_run =
        get_unprogressed_validators(all_steward_accounts, &validator_history_program_id);

    let ixs_to_run = validators_to_run
        .iter()
        .map(|validator_info| Instruction {
            program_id: *program_id,
            accounts: jito_steward::accounts::ComputeInstantUnstake {
                config: all_steward_accounts.config_address,
                state_account: all_steward_accounts.state_address,
                validator_history: validator_info.history_account,
                validator_list: all_steward_accounts.validator_list_address,
                cluster_history,
            }
            .to_account_metas(None),
            data: jito_steward::instruction::ComputeInstantUnstake {
                validator_list_index: validator_info.index as u64,
            }
            .data(),
        })
        .collect::<Vec<Instruction>>();

    let txs_to_run = package_instructions(&ixs_to_run, 1, priority_fee, Some(1_400_000), None);

    info!(
        "Submitting transactions step=compute_instant_unstake instructions={} transactions={}",
        ixs_to_run.len(),
        txs_to_run.len()
    );

    let stats = submit_packaged_transactions(client, txs_to_run, payer, Some(50), None).await?;

    Ok(stats)
}

/// Handles the rebalancing of validator stakes in a stake pool.
///
/// This function is responsible for redistributing stake across validators in a stake pool
/// according to the steward's rebalancing algorithm. It identifies validators that need
/// rebalancing, constructs the necessary instructions, and submits them as transactions.
///
/// # Note
///
/// ## Reserve Stake Pre-funding
///
/// Before submitting rebalance instructions, this function checks if the reserve stake
/// account has sufficient lamports (N * transient stake account + ephemeral_stake account) to cover rent for all validators being
/// processed.
async fn _handle_rebalance(
    payer: &Arc<Keypair>,
    client: &Arc<RpcClient>,
    program_id: &Pubkey,
    all_steward_accounts: &AllStewardAccounts,
    priority_fee: Option<u64>,
) -> Result<SubmitStats, JitoTransactionError> {
    let validator_history_program_id = validator_history::id();

    let validators_to_run =
        get_unprogressed_validators(all_steward_accounts, &validator_history_program_id);

    let reserve_stake_acc = client
        .get_account(&all_steward_accounts.stake_pool_account.reserve_stake)
        .await?;

    let stake_rent = client
        .get_minimum_balance_for_rent_exemption(StakeStateV2::size_of())
        .await?;

    let directed_stake_meta =
        get_directed_stake_meta_address(&all_steward_accounts.config_address, program_id);

    let needs_deposit = reserve_stake_acc
        .lamports
        .lt(&stake_rent.mul(validators_to_run.len() as u64));

    let deposit_sol_ix = if needs_deposit {
        let acc_token_address = get_associated_token_address(
            &payer.pubkey(),
            &all_steward_accounts.stake_pool_account.pool_mint,
        );

        Some(deposit_sol(
            &spl_stake_pool::id(),
            &all_steward_accounts.stake_pool_address,
            &all_steward_accounts.stake_pool_withdraw_authority,
            &all_steward_accounts.stake_pool_account.reserve_stake,
            &payer.pubkey(),
            &acc_token_address,
            &all_steward_accounts.stake_pool_account.manager_fee_account,
            &acc_token_address,
            &all_steward_accounts.stake_pool_account.pool_mint,
            &spl_token::id(),
            stake_rent,
        ))
    } else {
        None
    };

    let ixs_to_run: Vec<Instruction> = validators_to_run
        .iter()
        .flat_map(|validator_info| {
            let validator_index = validator_info.index;
            let vote_account = &validator_info.vote_account;
            let history_account = validator_info.history_account;

            let stake_address =
                get_stake_address(vote_account, &all_steward_accounts.stake_pool_address);

            let (transient_stake_address, _) = find_transient_stake_program_address(
                &spl_stake_pool::id(),
                vote_account,
                &all_steward_accounts.stake_pool_address,
                all_steward_accounts.validator_list_account.validators[validator_index]
                    .transient_seed_suffix
                    .into(),
            );

            let rebalance_ix = Instruction {
                program_id: *program_id,
                accounts: jito_steward::accounts::Rebalance {
                    config: all_steward_accounts.config_address,
                    state_account: all_steward_accounts.state_address,
                    validator_history: history_account,
                    stake_pool_program: spl_stake_pool::id(),
                    stake_pool: all_steward_accounts.stake_pool_address,
                    withdraw_authority: all_steward_accounts.stake_pool_withdraw_authority,
                    validator_list: all_steward_accounts.validator_list_address,
                    reserve_stake: all_steward_accounts.stake_pool_account.reserve_stake,
                    stake_account: stake_address,
                    transient_stake_account: transient_stake_address,
                    vote_account: *vote_account,
                    system_program: system_program::id(),
                    stake_program: stake::program::id(),
                    rent: solana_sdk::sysvar::rent::id(),
                    clock: solana_sdk::sysvar::clock::id(),
                    stake_history: solana_sdk::sysvar::stake_history::id(),
                    stake_config: stake::config::ID,
                    directed_stake_meta,
                }
                .to_account_metas(None),
                data: jito_steward::instruction::Rebalance {
                    validator_list_index: validator_index as u64,
                }
                .data(),
            };

            deposit_sol_ix
                .iter()
                .cloned()
                .chain(std::iter::once(rebalance_ix))
                .collect::<Vec<Instruction>>()
        })
        .collect();

    let txs_to_run = package_instructions(&ixs_to_run, 2, priority_fee, Some(1_400_000), None);

    info!(
        "Submitting transactions step=rebalance instructions={} transactions={}",
        ixs_to_run.len(),
        txs_to_run.len()
    );

    let stats = submit_packaged_transactions(client, txs_to_run, payer, Some(30), None).await?;

    Ok(stats)
}

/// Handles the directed rebalancing of validator stakes in a stake pool.
///
/// This function is responsible for redistributing stake across validators in a stake pool
/// according to the steward's rebalancing algorithm. It identifies validators that need
/// rebalancing, constructs the necessary instructions, and submits them as transactions.
///
/// # Note
///
/// ## Reserve Stake Pre-funding
///
/// Before submitting rebalance instructions, this function checks if the reserve stake
/// account has sufficient lamports (N * transient stake account + ephemeral_stake account) to cover rent for all validators being
/// processed.
///
/// ## If directed stake target is removed from validator list
/// - Pass u64::MAX for `validator_list_index`
/// - Pass Pubkey::new_unique() for `transient_stake_address`
async fn _handle_directed_rebalance(
    payer: &Arc<Keypair>,
    client: &Arc<RpcClient>,
    program_id: &Pubkey,
    all_steward_accounts: &AllStewardAccounts,
    priority_fee: Option<u64>,
) -> Result<SubmitStats, JitoTransactionError> {
    let mut submit_stats = SubmitStats::default();

    let directed_stake_meta_address =
        get_directed_stake_meta_address(&all_steward_accounts.config_address, program_id);
    let directed_stake_meta_account = get_directed_stake_meta(
        client.clone(),
        &all_steward_accounts.config_address,
        program_id,
    )
    .await?;
    let validators_to_run = DirectedRebalanceProgressionInfo::get_directed_staking_validators(
        all_steward_accounts,
        &directed_stake_meta_account,
    );

    let reserve_stake_acc = client
        .get_account(&all_steward_accounts.stake_pool_account.reserve_stake)
        .await?;

    let stake_rent = client
        .get_minimum_balance_for_rent_exemption(StakeStateV2::size_of())
        .await?;

    if reserve_stake_acc
        .lamports
        .lt(&stake_rent.mul(validators_to_run.len() as u64))
    {
        let amount: u64 = stake_rent
            .mul(validators_to_run.len() as u64)
            .sub(reserve_stake_acc.lamports);

        let acc_token_address = get_associated_token_address(
            &payer.pubkey(),
            &all_steward_accounts.stake_pool_account.pool_mint,
        );

        let instruction = deposit_sol(
            &spl_stake_pool::id(),
            &all_steward_accounts.stake_pool_address,
            &all_steward_accounts.stake_pool_withdraw_authority,
            &all_steward_accounts.stake_pool_account.reserve_stake,
            &payer.pubkey(),
            &acc_token_address,
            &all_steward_accounts.stake_pool_account.manager_fee_account,
            &acc_token_address,
            &all_steward_accounts.stake_pool_account.pool_mint,
            &spl_token::id(),
            amount,
        );

        let txs_to_run =
            package_instructions(&[instruction], 1, priority_fee, Some(1_400_000), None);
        let stats = submit_packaged_transactions(client, txs_to_run, payer, Some(30), None).await?;
        submit_stats.combine(&stats);
    }

    let mut ixs_to_run = Vec::new();
    for validator_info in validators_to_run.iter() {
        let validator_index = validator_info.validator_list_index;
        let vote_account = &validator_info.vote_account;

        let stake_address =
            get_stake_address(vote_account, &all_steward_accounts.stake_pool_address);

        let transient_stake_address = get_transient_stake_address(
            vote_account,
            &all_steward_accounts.stake_pool_address,
            &all_steward_accounts.validator_list_account,
            validator_index,
        )
        .unwrap_or(Pubkey::new_unique());

        let ix = Instruction {
            program_id: *program_id,
            accounts: jito_steward::accounts::RebalanceDirected {
                config: all_steward_accounts.config_address,
                state_account: all_steward_accounts.state_address,
                stake_pool_program: spl_stake_pool::id(),
                stake_pool: all_steward_accounts.stake_pool_address,
                withdraw_authority: all_steward_accounts.stake_pool_withdraw_authority,
                validator_list: all_steward_accounts.validator_list_address,
                reserve_stake: all_steward_accounts.stake_pool_account.reserve_stake,
                stake_account: stake_address,
                transient_stake_account: transient_stake_address,
                vote_account: *vote_account,
                system_program: system_program::id(),
                stake_program: stake::program::id(),
                rent: solana_sdk::sysvar::rent::id(),
                clock: solana_sdk::sysvar::clock::id(),
                stake_history: solana_sdk::sysvar::stake_history::id(),
                stake_config: stake::config::ID,
                directed_stake_meta: directed_stake_meta_address,
            }
            .to_account_metas(None),
            data: jito_steward::instruction::RebalanceDirected {
                directed_stake_meta_index: validator_info.directed_stake_meta_index as u64,
            }
            .data(),
        };
        ixs_to_run.push(ix);
    }

    let txs_to_run = package_instructions(&ixs_to_run, 1, priority_fee, Some(1_400_000), None);

    info!(
        "Submitting transactions step=rebalance_directed instructions={} transactions={}",
        ixs_to_run.len(),
        txs_to_run.len()
    );

    let stats = submit_packaged_transactions(client, txs_to_run, payer, Some(30), None).await?;
    submit_stats.combine(&stats);

    Ok(submit_stats)
}

/// Main steward cranking function that orchestrates all steward operations.
///
/// Executes operations in a specific order:
/// 1. Update stake pool balances
/// 2. Handle epoch maintenance (if at epoch boundary)
/// 3. Process instant validator removals
/// 4. Remove delinquent validators
/// 5. Add new qualified validators
/// 6. Progress through steward state machine
/// 7. Log any errors encountered
#[allow(clippy::too_many_arguments)]
pub async fn crank_steward(
    client: &Arc<RpcClient>,
    payer: &Arc<Keypair>,
    program_id: &Pubkey,
    epoch: u64,
    all_steward_accounts: &AllStewardAccounts,
    all_steward_validator_accounts: &AllValidatorAccounts,
    all_active_validator_accounts: &AllValidatorAccounts,
    priority_fee: Option<u64>,
) -> Result<SubmitStats, JitoTransactionError> {
    let mut return_stats = SubmitStats::default();
    let should_run_epoch_maintenance =
        all_steward_accounts.state_account.state.current_epoch != epoch;
    let should_crank_state = !should_run_epoch_maintenance;

    {
        // --------- UPDATE STAKE POOL -----------
        let stats = _update_pool(
            payer,
            client,
            epoch,
            all_steward_accounts,
            all_steward_validator_accounts,
            priority_fee,
        )
        .await?;

        return_stats.combine(&stats);
    }

    {
        // --------- CHECK AND HANDLE EPOCH BOUNDARY -----------

        if should_run_epoch_maintenance {
            info!("Cranking steward step=epoch_maintenance");

            let stats = _handle_epoch_maintenance(
                payer,
                client,
                program_id,
                epoch,
                all_steward_accounts,
                priority_fee,
            )
            .await?;

            return_stats.combine(&stats);
        }
    }

    {
        // --------- CHECK AND HANDLE INSTANT REMOVAL -----------
        info!("Cranking steward step=instant_removal");

        let stats = _handle_instant_removal_validators(
            payer,
            client,
            program_id,
            all_steward_accounts,
            priority_fee,
        )
        .await?;

        return_stats.combine(&stats);
    }

    {
        // --------- CHECK VALIDATORS TO REMOVE -----------
        info!("Cranking steward step=remove_delinquent_validators");

        let stats = _handle_delinquent_validators(
            payer,
            client,
            program_id,
            epoch,
            all_steward_accounts,
            all_steward_validator_accounts,
            priority_fee,
        )
        .await?;

        return_stats.combine(&stats);

        if stats.successes > 0 {
            return Ok(return_stats);
        }
    }

    {
        // --------- CHECK VALIDATORS TO ADD -----------
        info!("Cranking steward step=add_validators");
        // Any validator that has new history account
        // Anything that would pass the benchmark
        // Find any validators that that are not in pool
        let stats = _handle_adding_validators(
            payer,
            client,
            program_id,
            epoch,
            all_steward_accounts,
            all_steward_validator_accounts,
            all_active_validator_accounts,
            priority_fee,
        )
        .await?;

        return_stats.combine(&stats);
    }

    {
        // --------- CHECK AND HANDLE STATE -----------
        if should_crank_state {
            let stats = match all_steward_accounts.state_account.state.state_tag {
                StewardStateEnum::ComputeScores => {
                    info!("Cranking steward state=compute_scores");

                    _handle_compute_score(
                        payer,
                        client,
                        program_id,
                        all_steward_accounts,
                        priority_fee,
                    )
                    .await?
                }
                StewardStateEnum::ComputeDelegations => {
                    info!("Cranking steward state=compute_delegations");

                    _handle_compute_delegations(
                        payer,
                        client,
                        program_id,
                        all_steward_accounts,
                        priority_fee,
                    )
                    .await?
                }
                StewardStateEnum::Idle => {
                    info!("Cranking steward state=idle");

                    _handle_idle(
                        payer,
                        client,
                        program_id,
                        all_steward_accounts,
                        priority_fee,
                    )
                    .await?
                }
                StewardStateEnum::ComputeInstantUnstake => {
                    info!("Cranking steward state=compute_instant_unstake");

                    _handle_compute_instant_unstake(
                        payer,
                        client,
                        program_id,
                        all_steward_accounts,
                        priority_fee,
                    )
                    .await?
                }
                StewardStateEnum::Rebalance => {
                    info!("Cranking steward state=rebalance");

                    _handle_rebalance(
                        payer,
                        client,
                        program_id,
                        all_steward_accounts,
                        priority_fee,
                    )
                    .await?
                }
                StewardStateEnum::RebalanceDirected => {
                    info!("Cranking steward state=rebalance_directed");

                    _handle_directed_rebalance(
                        payer,
                        client,
                        program_id,
                        all_steward_accounts,
                        priority_fee,
                    )
                    .await?
                }
            };
            return_stats.combine(&stats);
        }
    }

    {
        // --------- RECOVER FROM ERROR -----------
        return_stats.results.iter().for_each(|result| {
            if let Err(error) = result {
                // Access and print the error
                match error {
                    JitoSendTransactionError::ExceededRetries => {
                        // Continue
                        error!("Transaction exceeded retries: {error:?}");
                    }
                    JitoSendTransactionError::TransactionError(e) => {
                        // Flag
                        error!("Transaction failed: {e:?}");
                    }
                    JitoSendTransactionError::RpcSimulateTransactionResult(e) => {
                        // Recover
                        error!(
                            "Transaction simulation failed err={:?} logs={:?}",
                            e.err, e.logs
                        );
                    }
                }
            }
        });
    }

    Ok(return_stats)
}
