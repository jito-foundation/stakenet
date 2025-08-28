use anchor_lang::prelude::*;

use crate::state::directed_stake::DirectedStakeMeta;
use crate::{constants::MAX_ALLOC_BYTES, errors::StewardError, Config};
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

    #[account(
        mut,
        address = config.load()?.directed_stake_whitelist_authority @ StewardError::Unauthorized
    )]
    pub authority: Signer<'info>,
}

impl InitializeDirectedStakeMeta<'_> {
    pub const SIZE: usize = 8 + size_of::<Self>();
}

pub fn handler(ctx: Context<InitializeDirectedStakeMeta>, total_stake_targets: u16) -> Result<()> {
    let epoch = ctx.accounts.clock.epoch;
    let total_stake_targets = total_stake_targets as u16;
    let epoch_bytes = epoch.to_le_bytes();
    let total_stake_targets_bytes = total_stake_targets.to_le_bytes();
    let mut stake_meta_data = ctx.accounts.directed_stake_meta.as_ref().try_borrow_mut_data()?;
    // Normal serialization will fail due to required reallocs, so we copy to offsets which will remain unchanged
    stake_meta_data[8..16].copy_from_slice(&epoch_bytes);
    stake_meta_data[16..18].copy_from_slice(&total_stake_targets_bytes);
    Ok(())
}
