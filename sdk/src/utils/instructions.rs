use std::{collections::HashMap, sync::Arc};

use anchor_lang::{InstructionData, ToAccountMetas};
use jito_steward::{DirectedStakePreference, DirectedStakeTicket};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    instruction::Instruction,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};
use validator_history::{constants::MAX_ALLOC_BYTES, ValidatorHistory};

use crate::{
    models::errors::JitoInstructionError,
    utils::{
        accounts::{
            get_directed_stake_meta, get_directed_stake_meta_address,
            get_directed_stake_ticket_address, get_directed_stake_tickets,
            get_directed_stake_whitelist_address, get_stake_pool_account,
            get_steward_config_account, get_steward_state_account, get_validator_list_account,
        },
        helpers::{aggregate_validator_targets, calculate_conversion_rate_bps, get_token_balance},
    },
};

use super::accounts::{get_validator_history_address, get_validator_history_config_address};

pub fn get_create_validator_history_instructions(
    vote_account: &Pubkey,
    program_id: &Pubkey,
    signer: &Keypair,
) -> Vec<Instruction> {
    let validator_history_account = get_validator_history_address(vote_account, program_id);
    let config_account = get_validator_history_config_address(program_id);

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

/// Creates an instruction to update a directed stake ticket.
///
/// This instruction allows a signer to update their stake delegation preferences by specifying
/// which validators they want to direct their stake to and in what proportions.
pub fn update_directed_stake_ticket(
    program_id: &Pubkey,
    steward_config: &Pubkey,
    signer: &Pubkey,
    preferences: Vec<DirectedStakePreference>,
) -> Instruction {
    let whitelist_account = get_directed_stake_whitelist_address(steward_config, program_id);
    let ticket_account = get_directed_stake_ticket_address(steward_config, signer, program_id);

    Instruction {
        program_id: *program_id,
        accounts: jito_steward::accounts::UpdateDirectedStakeTicket {
            config: *steward_config,
            whitelist_account,
            ticket_account,
            signer: *signer,
        }
        .to_account_metas(None),
        data: jito_steward::instruction::UpdateDirectedStakeTicket { preferences }.data(),
    }
}

/// Computes directed stake metadata and generates instructions to zero out stake targets for
/// validators with a steward score of `0`.
///
/// This function aggregates directed stake ticket holders' preferences, converts JitoSOL balances
/// to lamports, and emits `CopyDirectedStakeTargets` instructions **only** for validators whose
/// steward score is `0`, forcing their `total_target_lamports` to `0`. Validators with a score
/// greater than `0` are skipped.
///
/// # Process Overview
///
/// 1. Fetches all directed stake tickets from the program
/// 2. For each ticket holder:
///    - Retrieves their JitoSOL token balance
///    - Converts JitoSOL to lamports using the stake pool's conversion rate
///    - Applies their allocation preferences across validators
/// 3. Aggregates total target delegations per validator
/// 4. For each validator, checks its steward score:
///    - Score `== 0`: emits a `CopyDirectedStakeTargets` instruction with `total_target_lamports = 0`
///    - Score `> 0`: skipped — no instruction is generated
///
/// # Conversion Details
///
/// The function converts JitoSOL holdings to lamports using:
///
/// ```text
/// conversion_rate_bps = (stake_pool.total_lamports * 10,000) / pool_token_supply
/// allocation_lamports = (allocation_jitosol * conversion_rate_bps) / 10,000
/// ```
pub async fn compute_directed_stake_meta(
    client: Arc<RpcClient>,
    token_mint_address: &Pubkey,
    stake_pool_address: &Pubkey,
    steward_config: &Pubkey,
    authority_pubkey: &Pubkey,
    program_id: &Pubkey,
) -> Result<Vec<Instruction>, JitoInstructionError> {
    let ticket_map = get_directed_stake_tickets(client.clone(), program_id).await?;

    let stake_pool = get_stake_pool_account(&client.clone(), stake_pool_address).await?;
    let conversion_rate_bps =
        calculate_conversion_rate_bps(stake_pool.total_lamports, stake_pool.pool_token_supply)?;

    let mut jitosol_balances = HashMap::new();
    for ticket in ticket_map.values().copied() {
        let balance = get_token_balance(
            client.clone(),
            token_mint_address,
            &ticket.ticket_update_authority,
        )
        .await?;
        jitosol_balances.insert(ticket.ticket_update_authority, balance);
    }

    let existing_directed_stake_meta =
        get_directed_stake_meta(client.clone(), steward_config, program_id).await?;
    let tickets: Vec<DirectedStakeTicket> = ticket_map.values().copied().collect();
    let validator_targets = aggregate_validator_targets(
        &existing_directed_stake_meta,
        &tickets,
        &jitosol_balances,
        conversion_rate_bps,
    )?;

    // Get validator list to find indices
    let config_account = get_steward_config_account(&client, steward_config).await?;
    let steward_account = get_steward_state_account(&client, program_id, steward_config).await?;
    let stake_pool_account = get_stake_pool_account(&client, &config_account.stake_pool).await?;
    let validator_list_address = stake_pool_account.validator_list;
    let validator_list_account =
        get_validator_list_account(&client, &validator_list_address).await?;

    let directed_stake_meta_pda = get_directed_stake_meta_address(steward_config, program_id);

    let instructions = validator_targets
        .iter()
        .filter_map(|(vote_pubkey, _total_target_lamports)| {
            // Find the index of this vote_pubkey in the validator list
            let validator_list_index = validator_list_account
                .validators
                .iter()
                .position(|v| v.vote_account_address == *vote_pubkey)?;

            if steward_account
                .state
                .scores
                .get(validator_list_index)
                .is_some_and(|&s| s == 0)
            {
                Some(Instruction {
                    program_id: *program_id,
                    accounts: jito_steward::accounts::CopyDirectedStakeTargets {
                        config: *steward_config,
                        directed_stake_meta: directed_stake_meta_pda,
                        authority: *authority_pubkey,
                        clock: solana_sdk::sysvar::clock::id(),
                        validator_list: validator_list_address,
                    }
                    .to_account_metas(None),
                    data: jito_steward::instruction::CopyDirectedStakeTargets {
                        vote_pubkey: *vote_pubkey,
                        total_target_lamports: 0,
                        validator_list_index: validator_list_index as u32,
                    }
                    .data(),
                })
            } else {
                None
            }
        })
        .collect();

    Ok(instructions)
}

/// Computes the directed stake target for the Coinbase validator.
///
/// Fetches the Coinbase balance for the previous epoch from the Kobe API and builds a
/// `CopyDirectedStakeTargets` instruction to set the stake target for `coinbase_vote_pubkey`.
///
/// # Process Overview
///
/// 1. Fetches the Coinbase balance for the previous epoch via the Kobe API.
/// 2. Loads the on-chain `DirectedStakeMeta` account and collects any existing targets that
///    were last updated in the current epoch.
/// 3. Locates `coinbase_vote_pubkey` in the validator list.
/// 4. Computes `total_target_lamports` as the Coinbase balance plus any existing target for
///    this validator in the current epoch (saturating addition).
/// 5. Emits one `CopyDirectedStakeTargets` instruction for the validator.
///
/// # Return Value
///
/// Returns an empty vector if:
/// - The Kobe API returns no Coinbase balance (`coinbase_balance` is `None`), or
/// - `coinbase_vote_pubkey` is not present in the on-chain validator list.
pub async fn compute_coinbase_targets(
    client: Arc<RpcClient>,
    kobe_client: &KobeClient,
    steward_config: &Pubkey,
    authority_pubkey: &Pubkey,
    program_id: &Pubkey,
    coinbase_vote_pubkey: &Pubkey,
) -> Result<Vec<Instruction>, JitoInstructionError> {
    let epoch_info = client.get_epoch_info().await?;
    let current_epoch = epoch_info.epoch;
    let last_epoch = epoch_info.epoch - 1;

    let coinbase_balance = kobe_client.get_coinbase_balance(last_epoch).await?;

    let directed_stake_meta_pda = get_directed_stake_meta_address(steward_config, program_id);
    let directed_stake_meta =
        get_directed_stake_meta(client.clone(), steward_config, program_id).await?;
    let targets: HashMap<Pubkey, u64> = directed_stake_meta
        .targets
        .iter()
        .filter(|target| target.target_last_updated_epoch == current_epoch)
        .map(|target| (target.vote_pubkey, target.total_target_lamports))
        .collect();
    let config_account = get_steward_config_account(&client, steward_config).await?;
    let stake_pool_account = get_stake_pool_account(&client, &config_account.stake_pool).await?;
    let validator_list_address = stake_pool_account.validator_list;
    let validator_list_account =
        get_validator_list_account(&client, &validator_list_address).await?;

    let mut instructions = Vec::new();

    if let Some(cb_balance) = coinbase_balance.coinbase_balance {
        let mut cb_balance_lamports = cb_balance.cb_balance_lamports;
        if let Some(validator_list_index) = validator_list_account
            .validators
            .iter()
            .position(|v| v.vote_account_address.eq(coinbase_vote_pubkey))
        {
            if let Some(meta_target_lamports) = targets.get(coinbase_vote_pubkey) {
                cb_balance_lamports = cb_balance_lamports.saturating_add(*meta_target_lamports);
            }

            let ix = Instruction {
                program_id: *program_id,
                accounts: jito_steward::accounts::CopyDirectedStakeTargets {
                    config: *steward_config,
                    directed_stake_meta: directed_stake_meta_pda,
                    authority: *authority_pubkey,
                    clock: solana_sdk::sysvar::clock::id(),
                    validator_list: validator_list_address,
                }
                .to_account_metas(None),
                data: jito_steward::instruction::CopyDirectedStakeTargets {
                    vote_pubkey: *coinbase_vote_pubkey,
                    total_target_lamports: cb_balance_lamports,
                    validator_list_index: validator_list_index as u32,
                }
                .data(),
            };

            instructions.push(ix);
        }
    }

    Ok(instructions)
}
