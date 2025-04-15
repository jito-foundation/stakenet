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
    total_priority_fees: u64,
    total_leader_slots: u32,
    blocks_produced: u32,
    current_slot: u64,
) -> Result<()> {
    let mut validator_history_account = ctx.accounts.validator_history_account.load_mut()?;
    let clock = Clock::get()?;

    // Cannot set stake for future epochs
    if epoch > clock.epoch {
        return Err(ValidatorHistoryError::EpochOutOfRange.into());
    }
    let epoch = cast_epoch(epoch)?;

    validator_history_account.set_total_priority_fees_and_block_metadata(
        epoch,
        total_priority_fees,
        total_leader_slots,
        blocks_produced,
        current_slot,
    )?;

    Ok(())
}
