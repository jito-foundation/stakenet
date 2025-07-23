use anchor_lang::prelude::*;

use crate::{constants::MAX_ALLOC_BYTES, errors::ValidatorHistoryError, ValidatorStakeBuffer};

fn get_realloc_size(account_info: &AccountInfo) -> usize {
    let account_size = account_info.data_len();
    // If account is already over-allocated, don't try to shrink
    if account_size < ValidatorStakeBuffer::SIZE {
        ValidatorStakeBuffer::SIZE.min(account_size + MAX_ALLOC_BYTES)
    } else {
        account_size
    }
}

fn is_initialized(account_info: &AccountInfo) -> Result<bool> {
    let account_data = account_info.as_ref().try_borrow_data()?;
    // Parse last-observed-epoch bytes (first u64 field after discriminator)
    let epoch_bytes = account_data[8..16].to_vec();
    // Check for any non-zero bytes
    let non_zero = epoch_bytes.iter().any(|&x| x.ne(&0));
    Ok(non_zero)
}

#[derive(Accounts)]
pub struct ReallocValidatorStakeBufferAccount<'info> {
    #[account(
        mut,
        realloc = get_realloc_size(validator_stake_buffer_account.as_ref()),
        realloc::payer = signer,
        realloc::zero = false,
        seeds = [ValidatorStakeBuffer::SEED],
        bump
    )]
    pub validator_stake_buffer_account: AccountLoader<'info, ValidatorStakeBuffer>,
    #[account(mut)]
    pub signer: Signer<'info>,
    pub system_program: Program<'info, System>,
}

pub fn handle_realloc_validator_stake_buffer_account(
    ctx: Context<ReallocValidatorStakeBufferAccount>,
) -> Result<()> {
    let account_size = ctx
        .accounts
        .validator_stake_buffer_account
        .as_ref()
        .data_len();
    // Determine if account is sufficiently sized and/or initialized
    let big_enough = account_size >= ValidatorStakeBuffer::SIZE;
    let initialized = is_initialized(ctx.accounts.validator_stake_buffer_account.as_ref())?;
    match (big_enough, initialized) {
        // Not big enough
        (false, _) => {
            // Keep moving ...
        }
        // Big enough but not initialized yet
        (true, false) => {
            // Can actually initialze values now that the account is proper size
            let mut validator_stake_buffer_account =
                ctx.accounts.validator_stake_buffer_account.load_mut()?;
            // Indicate that the account has been initialized
            // by uniquely setting the last-observed-epoch value to 1
            validator_stake_buffer_account.reset(1u64);
        }
        // Already initialized
        (true, true) => {
            return Err(ValidatorHistoryError::NoReallocNeeded.into());
        }
    }
    Ok(())
}
