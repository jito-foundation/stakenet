use anchor_lang::{
    prelude::*,
    solana_program::{clock::Clock, vote},
};
use validator_history_vote_state::VoteStateVersions;

use crate::{state::ValidatorHistory, utils::cast_epoch};

#[derive(Accounts)]
pub struct CopyVoteAccount<'info> {
    #[account(
        mut,
        seeds = [ValidatorHistory::SEED, vote_account.key().as_ref()],
        bump,
        has_one = vote_account
    )]
    pub validator_history_account: AccountLoader<'info, ValidatorHistory>,
    /// CHECK: Safe because we check the vote program is the owner before reading bytes.
    #[account(owner = vote::program::ID.key())]
    pub vote_account: AccountInfo<'info>,
    #[account(mut)]
    pub signer: Signer<'info>,
}

pub fn handle_copy_vote_account(ctx: Context<CopyVoteAccount>) -> Result<()> {
    let mut validator_history_account = ctx.accounts.validator_history_account.load_mut()?;
    let clock = Clock::get()?;
    let epoch = cast_epoch(clock.epoch)?;

    let commission = VoteStateVersions::deserialize_commission(&ctx.accounts.vote_account)?;
    validator_history_account.set_commission_and_slot(epoch, commission, clock.slot)?;

    let epoch_credits = VoteStateVersions::deserialize_epoch_credits(&ctx.accounts.vote_account)?;
    validator_history_account.insert_missing_entries(&epoch_credits)?;
    validator_history_account.set_epoch_credits(&epoch_credits)?;

    Ok(())
}
