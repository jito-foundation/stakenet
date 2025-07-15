use anchor_lang::prelude::*;
use anchor_lang::solana_program::epoch_stake::get_epoch_stake_for_vote_account;

use crate::{Config, ValidatorHistory, ValidatorStake, ValidatorStakeBuffer};

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

    #[account(
        seeds = [Config::SEED],
        bump = config.bump,
    )]
    pub config: Account<'info, Config>,
}
// TODO: write event when finalized?
pub fn handle_update_stake_buffer(ctx: Context<UpdateStakeBuffer>) -> Result<()> {
    // Get validator vote account and index for insertion
    let validator_history = ctx.accounts.validator_history_account.load()?;
    let validator_id = validator_history.index;
    let vote_account_pubkey = validator_history.vote_account;

    // Build insert context
    let config = &ctx.accounts.config;
    let mut validator_stake_buffer = ctx.accounts.validator_stake_buffer_account.load_mut()?;

    // Validate buffer against epoch
    let epoch = Clock::get()?.epoch;
    if validator_stake_buffer.needs_reset(epoch) {
        // Reset buffer
        validator_stake_buffer.reset(epoch);
    }

    // Observe vote account stake
    let stake_amount: u64 = get_epoch_stake_for_vote_account(&vote_account_pubkey);
    // Insert into buffer
    let entry = ValidatorStake::new(validator_id, stake_amount);
    let mut insert = validator_stake_buffer.insert_builder(config);
    insert(entry)?;

    Ok(())
}
