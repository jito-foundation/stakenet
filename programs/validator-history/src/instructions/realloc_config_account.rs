use crate::{errors::ValidatorHistoryError, Config};
use anchor_lang::prelude::*;

/// separate max config size since it's not zero copy
const MAX_CONFIG_SIZE: u64 = 1_000;

#[derive(Accounts)]
#[instruction(new_size: u64)]
pub struct ReallocConfigAccount<'info> {
    #[account(
        mut,
        realloc = new_size as usize,
        realloc::payer = signer,
        // any new memory allocated during reallocation is set to zero.
        realloc::zero = true,
        seeds = [Config::SEED],
        bump
    )]
    pub config_account: Account<'info, Config>,
    pub system_program: Program<'info, System>,
    #[account(mut)]
    pub signer: Signer<'info>,
}

pub fn handle_realloc_config_account(
    _ctx: Context<ReallocConfigAccount>,
    new_size: u64,
) -> Result<()> {
    require!(
        new_size <= MAX_CONFIG_SIZE,
        ValidatorHistoryError::AccountFullySized
    );
    require!(
        new_size as usize >= Config::SIZE,
        ValidatorHistoryError::DeallocNotAllowed
    );
    Ok(())
}
