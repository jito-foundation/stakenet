use anchor_lang::prelude::*;

use crate::{constants::MAX_ALLOC_BYTES, Config, DirectedStakeWhitelist};
use std::mem::size_of;

#[derive(Accounts)]
pub struct InitializeDirectedStakeWhitelist<'info> {
    #[account()]
    pub config: AccountLoader<'info, Config>,

    #[account(
        init,
        payer = authority,
        space = MAX_ALLOC_BYTES,
        seeds = [DirectedStakeWhitelist::SEED, config.key().as_ref()],
        bump
    )]
    pub directed_stake_whitelist: AccountLoader<'info, DirectedStakeWhitelist>,

    pub system_program: Program<'info, System>,

    #[account(mut)]
    pub authority: Signer<'info>,
}

impl InitializeDirectedStakeWhitelist<'_> {
    pub const SIZE: usize = 8 + size_of::<Self>();
}

pub fn handler(_ctx: Context<InitializeDirectedStakeWhitelist>) -> Result<()> {
    Ok(())
}
