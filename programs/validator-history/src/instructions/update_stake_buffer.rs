use anchor_lang::prelude::*;

use crate::{ValidatorHistory, ValidatorStakeBuffer};

#[derive(Accounts)]
pub struct UpdateStakeBuffer<'info> {
    #[account(
        mut,
        seeds = [ValidatorStakeBuffer::SEED],
        bump
    )]
    pub validator_stake_buffer_account: AccountLoader<'info, ValidatorStakeBuffer>,

    #[account(owner = crate::id())]
    pub validator_history_account: AccountLoader<'info, ValidatorHistory>,
}

pub fn handle_update_stake_buffer(ctx: Context<UpdateStakeBuffer>) -> Result<()> {
    Ok(())
}
