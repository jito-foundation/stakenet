use anchor_lang::prelude::*;

use crate::state::directed_stake::{DirectedStakeMeta, DirectedStakeTarget};
use crate::{errors::StewardError, Config};
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

    if stake_meta.uploaded_stake_targets >= stake_meta.total_stake_targets {
        return Err(error!(StewardError::InvalidParameterValue));
    }

    if stake_meta.get_target_index(&vote_pubkey).is_some() {
        return Err(error!(StewardError::InvalidParameterValue));
    }

    let target_index = stake_meta.uploaded_stake_targets as usize;

    let new_target = DirectedStakeTarget {
        vote_pubkey,
        total_target_lamports: target_lamports,
        total_staked_lamports: 0,
        _padding0: [0; 64],
    };

    stake_meta.targets[target_index] = new_target;

    stake_meta.uploaded_stake_targets += 1;
    Ok(())
}
