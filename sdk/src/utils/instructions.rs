use std::{collections::HashMap, sync::Arc};

use anchor_lang::{InstructionData, ToAccountMetas};
use jito_steward::DirectedStakePreference;
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
            get_directed_stake_meta_address, get_directed_stake_ticket_address,
            get_directed_stake_tickets, get_directed_stake_whitelist_address,
            get_stake_pool_account,
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
    let ticket_account = get_directed_stake_ticket_address(signer, program_id);

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

/// Computes directed stake metadata and generates instructions to copy stake targets to the chain.
///
/// This function performs a comprehensive calculation of stake delegation targets across all validators
/// based on directed stake tickets. It aggregates all ticket holders' preferences, converts JitoSOL
/// balances to lamports, and generates the necessary instructions to update on-chain metadata.
///
/// # Process Overview
///
/// 1. Fetches all directed stake tickets from the program
/// 2. For each ticket holder:
///    - Retrieves their JitoSOL token balance
///    - Converts JitoSOL to lamports using the stake pool's conversion rate
///    - Applies their allocation preferences across validators
/// 3. Aggregates total target delegations per validator
/// 4. Generates `CopyDirectedStakeTargets` instructions for each validator
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
    let tickets = get_directed_stake_tickets(client.clone(), program_id).await?;

    let stake_pool = get_stake_pool_account(&client.clone(), stake_pool_address).await?;
    let conversion_rate_bps =
        calculate_conversion_rate_bps(stake_pool.total_lamports, stake_pool.pool_token_supply)?;

    let mut jitosol_balances = HashMap::new();
    for ticket in &tickets {
        let balance = get_token_balance(
            client.clone(),
            token_mint_address,
            &ticket.ticket_update_authority,
        )
        .await?;
        jitosol_balances.insert(ticket.ticket_update_authority, balance);
    }

    let validator_targets =
        aggregate_validator_targets(&tickets, &jitosol_balances, conversion_rate_bps)?;

    let directed_stake_meta_pda = get_directed_stake_meta_address(steward_config, program_id);

    let instructions = validator_targets
        .iter()
        .map(|(vote_pubkey, total_target_lamports)| Instruction {
            program_id: *program_id,
            accounts: jito_steward::accounts::CopyDirectedStakeTargets {
                config: *steward_config,
                directed_stake_meta: directed_stake_meta_pda,
                authority: *authority_pubkey,
                clock: solana_sdk::sysvar::clock::id(),
            }
            .to_account_metas(None),
            data: jito_steward::instruction::CopyDirectedStakeTargets {
                vote_pubkey: *vote_pubkey,
                total_target_lamports: *total_target_lamports,
            }
            .data(),
        })
        .collect();
    Ok(instructions)
}
