use crate::{
    errors::ValidatorHistoryError,
    state::{Config, ValidatorHistory},
    utils::cast_epoch,
};
use anchor_lang::{prelude::*, solana_program::vote};

#[derive(Accounts)]
pub struct UpdatePriorityFeeHistory<'info> {
    #[account(
      mut,
      seeds = [ValidatorHistory::SEED, vote_account.key().as_ref()],
      bump
  )]
    pub validator_history_account: AccountLoader<'info, ValidatorHistory>,

    /// CHECK: fine since we are not deserializing account
    #[account(owner = vote::program::ID.key())]
    pub vote_account: AccountInfo<'info>,

    #[account(
      seeds = [Config::SEED],
      bump = config.bump,
      has_one = priority_fee_oracle_authority
  )]
    pub config: Account<'info, Config>,

    pub priority_fee_oracle_authority: Signer<'info>,
}

pub fn handle_update_priority_fee_history(
    ctx: Context<UpdatePriorityFeeHistory>,
    epoch: u64,
    lamports: u64,
) -> Result<()> {
    let mut validator_history_account: std::cell::RefMut<'_, ValidatorHistory> =
        ctx.accounts.validator_history_account.load_mut()?;

    // Cannot set stake for future epochs
    if epoch > Clock::get()?.epoch {
        return Err(ValidatorHistoryError::EpochOutOfRange.into());
    }
    let epoch = cast_epoch(epoch)?;

    validator_history_account.set_total_priority_fees(epoch, lamports)?;

    Ok(())
}
