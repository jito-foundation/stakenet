use anchor_lang::{
    prelude::*,
    solana_program::{clock::Clock, slot_history::Check},
};

use crate::{errors::ValidatorHistoryError, utils::cast_epoch, ClusterHistory};

#[derive(Accounts)]
pub struct CopyClusterInfo<'info> {
    #[account(
        mut,
        seeds = [ClusterHistory::SEED],
        bump,
    )]
    pub cluster_history_account: AccountLoader<'info, ClusterHistory>,
    #[account(mut)]
    pub signer: Signer<'info>,
}

pub fn handler(ctx: Context<CopyClusterInfo>) -> Result<()> {
    let mut cluster_history_account = ctx.accounts.cluster_history_account.load_mut()?;
    let clock = Clock::get()?;

    let epoch_schedule = EpochSchedule::get()?;
    let slot_history = SlotHistory::get()?;

    let start_slot = epoch_schedule.get_first_slot_in_epoch(clock.epoch - 1);
    let end_slot = epoch_schedule.get_first_slot_in_epoch(clock.epoch);

    let mut blocks_in_epoch = 0;
    for i in start_slot..end_slot {
        match slot_history.check(i) {
            Check::Found => {
                blocks_in_epoch += 1;
            }
            Check::NotFound => {
                // do nothing
            }
            Check::TooOld => {
                return Err(ValidatorHistoryError::SlotHistoryOutOfDate.into());
            }
            Check::Future => {
                return Err(ValidatorHistoryError::SlotHistoryOutOfDate.into());
            }
        };
    }

    let epoch = cast_epoch(clock.epoch);

    // Sets the slot history for the previous epoch, since the current epoch is not yet complete.
    cluster_history_account.set_blocks(epoch - 1, blocks_in_epoch)?;

    Ok(())
}
