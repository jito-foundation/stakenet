use crate::constants::PREFERRED_WITHDRAW_THRESHOLD_LAMPORTS;
use crate::errors::StewardError;
use crate::stake_pool_utils::deserialize_stake_pool;
use crate::state::Config;
use crate::utils::get_stake_pool_address;
use crate::{StewardStateAccount, StewardStateAccountV2};
use anchor_lang::prelude::*;
use anchor_lang::solana_program::{
    program::invoke_signed,
    stake::{state::StakeStateV2, tools::get_minimum_delegation},
};
use spl_stake_pool::instruction::PreferredValidatorType;
use spl_stake_pool::minimum_delegation;
use spl_stake_pool::state::{StakeStatus, ValidatorListHeader, ValidatorStakeInfo};

#[derive(Accounts)]
pub struct UpdatePreferredWithdrawValidator<'info> {
    pub config: AccountLoader<'info, Config>,

    #[account(
        seeds = [StewardStateAccount::SEED, config.key().as_ref()],
        bump
    )]
    pub state_account: AccountLoader<'info, StewardStateAccountV2>,

    /// CHECK: CPI program
    #[account(address = spl_stake_pool::ID)]
    pub stake_pool_program: AccountInfo<'info>,

    #[account(
        mut,
        address = get_stake_pool_address(&config)?
    )]
    /// CHECK: Validated by address check
    pub stake_pool: AccountInfo<'info>,

    /// CHECK: Validated in handler by deserialization
    #[account(address = deserialize_stake_pool(&stake_pool)?.validator_list)]
    pub validator_list: AccountInfo<'info>,

    /// CHECK: Stake program
    #[account(address = anchor_lang::solana_program::stake::program::ID)]
    pub stake_program: AccountInfo<'info>,

    /// Payer for the transaction
    #[account(mut)]
    pub signer: Signer<'info>,
}

/// Updates the preferred withdraw validator to the validator with the lowest score
/// that has sufficient available lamports for withdrawals
pub fn handler(ctx: Context<UpdatePreferredWithdrawValidator>) -> Result<()> {
    let state_account = ctx.accounts.state_account.load()?;
    let stake_pool = deserialize_stake_pool(&ctx.accounts.stake_pool)?;

    // Get stake pool parameters
    let minimum_delegation = minimum_delegation(get_minimum_delegation()?);
    let stake_rent = Rent::get()?.minimum_balance(StakeStateV2::size_of());

    let base_lamport_balance = minimum_delegation
        .checked_add(stake_rent)
        .ok_or(StewardError::ArithmeticError)?;

    // Read validator list
    let validator_list_data = &mut ctx.accounts.validator_list.try_borrow_mut_data()?;
    let (header, validator_list) = ValidatorListHeader::deserialize_vec(validator_list_data)?;

    if !header.is_valid() {
        return Err(StewardError::ValidatorListTypeMismatch.into());
    }

    // Find the optimal withdraw validator by iterating through sorted_raw_score_indices in reverse (lowest scores first)
    let mut optimal_validator: Option<Pubkey> = None;

    for idx in state_account.state.sorted_raw_score_indices
        [..state_account.state.num_pool_validators as usize]
        .iter()
        .rev()
    {
        let validator_index = *idx as usize;

        // Skip if index is out of bounds
        if validator_index >= header.max_validators as usize {
            continue;
        }

        // Get validator stake info
        let validator_info_slice =
            validator_list.deserialize_slice::<ValidatorStakeInfo>(validator_index, 1)?;

        if validator_info_slice.is_empty() {
            continue;
        }

        let validator_info = &validator_info_slice[0];

        // Skip if validator is not active
        let status = StakeStatus::try_from(validator_info.status)?;
        if status != StakeStatus::Active {
            continue;
        }

        // Calculate available lamports for withdrawal
        let active_stake = u64::from(validator_info.active_stake_lamports);
        let available_lamports = active_stake.saturating_sub(base_lamport_balance);

        // Check if this validator meets the threshold
        if available_lamports >= PREFERRED_WITHDRAW_THRESHOLD_LAMPORTS {
            optimal_validator = Some(validator_info.vote_account_address);
            msg!(
                "Found suitable validator {} with {} available lamports",
                validator_info.vote_account_address,
                available_lamports
            );
            break;
        }
    }

    // Get current preferred withdraw validator
    let current_preferred = stake_pool.preferred_withdraw_validator_vote_address;

    // Only update if the optimal validator is different from current
    if optimal_validator != current_preferred {
        msg!(
            "Updating preferred withdraw validator from {:?} to {:?}",
            current_preferred,
            optimal_validator
        );

        // Call set_preferred_validator on the stake pool
        invoke_signed(
            &spl_stake_pool::instruction::set_preferred_validator(
                ctx.accounts.stake_pool_program.key,
                &ctx.accounts.stake_pool.key(),
                &ctx.accounts.state_account.key(),
                &ctx.accounts.validator_list.key(),
                PreferredValidatorType::Withdraw,
                optimal_validator,
            ),
            &[
                ctx.accounts.stake_pool.to_account_info(),
                ctx.accounts.state_account.to_account_info(),
                ctx.accounts.validator_list.to_account_info(),
            ],
            &[&[
                StewardStateAccount::SEED,
                &ctx.accounts.config.key().to_bytes(),
                &[ctx.bumps.state_account],
            ]],
        )?;

        emit!(PreferredWithdrawValidatorUpdated {
            old_validator: current_preferred,
            new_validator: optimal_validator,
        });
    } else {
        msg!("Preferred withdraw validator already optimal, no update needed");
    }

    Ok(())
}

#[event]
pub struct PreferredWithdrawValidatorUpdated {
    pub old_validator: Option<Pubkey>,
    pub new_validator: Option<Pubkey>,
}
