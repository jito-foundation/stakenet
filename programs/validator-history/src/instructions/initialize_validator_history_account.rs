use crate::{
    constants::{MAX_ALLOC_BYTES, MIN_VOTE_EPOCHS},
    errors::ValidatorHistoryError,
    state::ValidatorHistory,
};
use anchor_lang::{prelude::*, solana_program::vote};
use validator_history_vote_state::VoteStateVersions;

#[derive(Accounts)]
pub struct InitializeValidatorHistoryAccount<'info> {
    #[account(
        init,
        payer = signer,
        space = MAX_ALLOC_BYTES,
        seeds = [ValidatorHistory::SEED, vote_account.key().as_ref()],
        bump
    )]
    pub validator_history_account: AccountLoader<'info, ValidatorHistory>,
    /// CHECK: Safe because we check the vote program is the owner before deserialization.
    #[account(owner = vote::program::ID.key())]
    pub vote_account: AccountInfo<'info>,
    pub system_program: Program<'info, System>,
    #[account(mut)]
    pub signer: Signer<'info>,
}

pub fn handler(ctx: Context<InitializeValidatorHistoryAccount>) -> Result<()> {
    // Need minimum 5 epochs of vote credits to be valid
    let epoch_credits = VoteStateVersions::deserialize_epoch_credits(&ctx.accounts.vote_account)?;
    if epoch_credits.len() < MIN_VOTE_EPOCHS {
        return Err(ValidatorHistoryError::NotEnoughVotingHistory.into());
    }
    Ok(())
}
