use anchor_lang::prelude::*;

use crate::{
    constants::MAX_VALIDATORS, state::directed_stake::DirectedStakeMeta, utils::get_config_admin,
    Config,
};

#[derive(Accounts)]
pub struct MigrateDirectedToAlgorithmic<'info> {
    pub config: AccountLoader<'info, Config>,

    #[account(
        mut,
        seeds = [DirectedStakeMeta::SEED, config.key().as_ref()],
        bump
    )]
    pub directed_stake_meta: AccountLoader<'info, DirectedStakeMeta>,

    #[account(address = get_config_admin(&config)?)]
    pub authority: Signer<'info>,
}

/// Zeroes all directed stake targets and applied lamports so that all existing
/// on-chain stake is reclassified as algorithmic without requiring an unstake.
/// Both mirrors (targets[i].total_staked_lamports and directed_stake_lamports[j])
/// are cleared atomically to preserve the accounting invariant.
pub fn handler(ctx: Context<MigrateDirectedToAlgorithmic>) -> Result<()> {
    let clock = Clock::get()?;
    let directed_stake_meta = &mut ctx.accounts.directed_stake_meta.load_mut()?;

    for target in directed_stake_meta.targets.iter_mut() {
        if target.vote_pubkey.ne(&Pubkey::default()) {
            target.total_target_lamports = 0;
            target.total_staked_lamports = 0;
            target.target_last_updated_epoch = clock.epoch;
            target.staked_last_updated_epoch = clock.epoch;
        }
    }

    directed_stake_meta.directed_stake_lamports = [0; MAX_VALIDATORS];
    directed_stake_meta.directed_unstake_total = 0;

    Ok(())
}
