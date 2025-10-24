use anchor_lang::prelude::*;

use crate::state::directed_stake::{DirectedStakeMeta, DirectedStakeTarget};
use crate::{errors::StewardError, maybe_transition, Config, StewardStateAccount};
use std::mem::size_of;

#[derive(Accounts)]
pub struct CopyDirectedStakeTargets<'info> {
    #[account()]
    pub config: AccountLoader<'info, Config>,

    #[account(
        mut,
        seeds = [DirectedStakeMeta::SEED, config.key().as_ref()],
        bump
    )]
    pub directed_stake_meta: AccountLoader<'info, DirectedStakeMeta>,

    pub clock: Sysvar<'info, Clock>,

    #[account(
        mut,
        address = config.load()?.directed_stake_whitelist_authority @ StewardError::Unauthorized
    )]
    pub authority: Signer<'info>,
}

impl CopyDirectedStakeTargets<'_> {
    pub const SIZE: usize = 8 + size_of::<Self>();
}

pub fn handler(
    ctx: Context<CopyDirectedStakeTargets>,
    vote_pubkey: Pubkey,
    target_lamports: u64,
) -> Result<()> {
    let mut stake_meta = ctx.accounts.directed_stake_meta.load_mut()?;
    let config = ctx.accounts.config.load()?;

    if ctx.accounts.authority.key() != config.directed_stake_meta_upload_authority {
        return Err(error!(StewardError::Unauthorized));
    }

    let maybe_target_index = stake_meta.get_target_index(&vote_pubkey);
    if maybe_target_index.is_none() {
        let new_target = DirectedStakeTarget {
            vote_pubkey,
            total_target_lamports: target_lamports,
            total_staked_lamports: 0,
            target_last_updated_epoch: 0,
            staked_last_updated_epoch: 0,
            _padding0: [0; 64],
        };
        let target_index = stake_meta.total_stake_targets as usize;
        stake_meta.targets[target_index] = new_target;
        stake_meta.total_stake_targets += 1;
    }

    if let Some(target_index) = maybe_target_index {
        let clock = Clock::get()?;
        stake_meta.targets[target_index].total_target_lamports = target_lamports;
        stake_meta.targets[target_index].target_last_updated_epoch = clock.epoch;
    }

    Ok(())
}
