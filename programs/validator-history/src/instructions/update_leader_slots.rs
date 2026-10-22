use anchor_lang::{prelude::*, solana_program::vote};

use crate::{errors::ValidatorHistoryError, state::ValidatorHistory, utils::cast_epoch, Config};

/// Records how many leader slots a validator is scheduled for in a given epoch.
/// Only callable by the oracle authority.
#[derive(Accounts)]
pub struct UpdateLeaderSlots<'info> {
    #[account(
        seeds = [Config::SEED],
        bump = config.bump,
        has_one = oracle_authority
    )]
    pub config: Account<'info, Config>,

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

    pub oracle_authority: Signer<'info>,
}

/// Sets the scheduled leader slot count for a validator at the specified epoch.
/// Rejects counts larger than the number of slots in the epoch.
pub fn handle_update_leader_slots(
    ctx: Context<UpdateLeaderSlots>,
    epoch: u64,
    total_leader_slots: u32,
) -> Result<()> {
    let clock = Clock::get()?;

    require!(epoch <= clock.epoch, ValidatorHistoryError::EpochOutOfRange);

    let slots_in_epoch = EpochSchedule::get()?.get_slots_in_epoch(epoch);
    require!(
        u64::from(total_leader_slots) <= slots_in_epoch,
        ValidatorHistoryError::InvalidLeaderSlots
    );

    let mut validator_history_account = ctx.accounts.validator_history_account.load_mut()?;

    let epoch = cast_epoch(epoch)?;

    validator_history_account.set_total_leader_slots(epoch, total_leader_slots)?;

    Ok(())
}
