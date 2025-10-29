use crate::state::directed_stake::DirectedStakeMeta;
use crate::{constants::MAX_ALLOC_BYTES, Config};
use anchor_lang::prelude::*;
use std::mem::size_of;

#[derive(Accounts)]
pub struct InitializeDirectedStakeMeta<'info> {
    #[account()]
    pub config: AccountLoader<'info, Config>,

    #[account(
        init,
        payer = authority,
        space = MAX_ALLOC_BYTES,
        seeds = [DirectedStakeMeta::SEED, config.key().as_ref()],
        bump
    )]
    pub directed_stake_meta: AccountLoader<'info, DirectedStakeMeta>,

    pub clock: Sysvar<'info, Clock>,

    pub system_program: Program<'info, System>,

    #[account(mut)]
    pub authority: Signer<'info>,
}

impl InitializeDirectedStakeMeta<'_> {
    pub const SIZE: usize = 8 + size_of::<Self>();
}

pub fn handler(_ctx: Context<InitializeDirectedStakeMeta>) -> Result<()> {
    Ok(())
}
