use std::{collections::HashMap, sync::Arc};

use anchor_lang::{InstructionData, ToAccountMetas};
use borsh_1::BorshDeserialize;
use jito_steward::DirectedStakePreference;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    instruction::Instruction,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};
use spl_stake_pool::state::StakePool;
use validator_history::{constants::MAX_ALLOC_BYTES, ValidatorHistory};

use crate::{
    models::errors::JitoTransactionError,
    utils::accounts::{
        get_directed_stake_meta_address, get_directed_stake_ticket_address,
        get_directed_stake_tickets, get_directed_stake_whitelist_address,
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
///
/// # Example
///
/// ```no_run
/// use std::{str::FromStr, sync::Arc};
///
/// use jito_steward::DirectedStakePreference;
/// use solana_sdk::pubkey::Pubkey;
/// use stakenet_sdk::utils::instructions::update_directed_stake_ticket;
///
/// let program_id = Pubkey::from_str("Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8").unwrap();
/// let steward_config = Pubkey::from_str("jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv").unwrap();
/// let signer = Pubkey::new_unique();
/// let validator_a = Pubkey::new_unique();
/// let preferences = vec![
///     DirectedStakePreference::new(validator_a, 10000)
/// ];
///
/// let update_ix = update_directed_stake_ticket(
///     &program_id,
///     &steward_config,
///     &signer,
///     preferences
/// );
/// ```
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
/// ```text
/// conversion_rate_bps = (stake_pool.total_lamports * 10,000) / pool_token_supply
/// allocation_lamports = (allocation_jitosol * conversion_rate_bps) / 10,000
/// ```
///
/// # Example
///
/// ```no_run
/// use std::sync::Arc;
///
/// use solana_client::nonblocking::rpc_client::RpcClient;
/// use solana_sdk::pubkey::Pubkey;
/// use stakenet_sdk::utils::instructions::compute_directed_stake_meta;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let client = Arc::new(RpcClient::new("https://api.mainnet-beta.solana.com".to_string()));
/// let stake_pool = Pubkey::new_unique();
/// let steward_config = Pubkey::new_unique();
/// let authority = Pubkey::new_unique();
/// let program_id = Pubkey::new_unique();
///
/// let instructions = compute_directed_stake_meta(
///     client,
///     &stake_pool,
///     &steward_config,
///     &authority,
///     &program_id,
/// ).await?;
///
/// # Ok(())
/// # }
/// ```
pub async fn compute_directed_stake_meta(
    client: Arc<RpcClient>,
    stake_pool_address: &Pubkey,
    steward_config: &Pubkey,
    authority_pubkey: &Pubkey,
    program_id: &Pubkey,
) -> Result<Vec<Instruction>, JitoTransactionError> {
    let mut validator_target_delegations: HashMap<Pubkey, u64> = HashMap::new();
    let tickets = get_directed_stake_tickets(client.clone(), program_id).await?;

    for ticket in &tickets {
        let (jitosol_balance, _jitosol_ui_amount) = match client
            .get_token_account_balance(&ticket.ticket_update_authority)
            .await
        {
            Ok(balance) => (balance.amount.clone(), balance.ui_amount.unwrap_or(0.0)),
            Err(_) => ("0".to_string(), 0.0),
        };

        let stake_pool_account = client.get_account(&stake_pool_address).await?;
        let stake_pool = StakePool::deserialize(&mut stake_pool_account.data.as_slice()).unwrap();

        let total_lamports: u64 = stake_pool.total_lamports;
        let pool_token_supply: u64 = stake_pool.pool_token_supply;
        let conversion_rate_bps: u64 = (total_lamports as u128)
            .checked_mul(10_000)
            .unwrap()
            .checked_div(pool_token_supply as u128)
            .unwrap() as u64;

        for preference in ticket.staker_preferences {
            if preference.vote_pubkey != Pubkey::default() {
                let total_lamports: u64 = jitosol_balance.parse::<u64>().unwrap();
                let allocation_jito_sol = preference.get_allocation(total_lamports);
                let allocation_lamports = allocation_jito_sol
                    .checked_mul(conversion_rate_bps as u128)
                    .unwrap()
                    .checked_div(10_000)
                    .unwrap() as u64;
                let current_allocation = validator_target_delegations
                    .get(&preference.vote_pubkey)
                    .unwrap_or(&0);
                validator_target_delegations.insert(
                    preference.vote_pubkey,
                    current_allocation.saturating_add(allocation_lamports),
                );
            }
        }
    }

    let pending_keys: Vec<Pubkey> = validator_target_delegations.keys().cloned().collect();

    let mut instructions = Vec::new();
    for i in 0..pending_keys.len() {
        let directed_stake_meta_pda = get_directed_stake_meta_address(steward_config, program_id);

        let instruction = Instruction {
            program_id: *program_id,
            accounts: jito_steward::accounts::CopyDirectedStakeTargets {
                config: *steward_config,
                directed_stake_meta: directed_stake_meta_pda,
                authority: *authority_pubkey,
                clock: solana_sdk::sysvar::clock::id(),
            }
            .to_account_metas(None),
            data: jito_steward::instruction::CopyDirectedStakeTargets {
                vote_pubkey: pending_keys[i],
                total_target_lamports: validator_target_delegations[&pending_keys[i]],
            }
            .data(),
        };

        instructions.push(instruction);
    }

    Ok(instructions)
}
