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
    /// CHECK: slot_history sysvar
    #[account(address = anchor_lang::solana_program::sysvar::slot_history::id())]
    pub slot_history: UncheckedAccount<'info>,
    #[account(mut)]
    pub signer: Signer<'info>,
}

pub fn handler(ctx: Context<CopyClusterInfo>) -> Result<()> {
    let mut cluster_history_account = ctx.accounts.cluster_history_account.load_mut()?;
    let slot_history: Box<SlotHistory> =
        Box::new(bincode::deserialize(&ctx.accounts.slot_history.try_borrow_data()?).unwrap());

    let clock = Clock::get()?;

    let epoch = cast_epoch(clock.epoch);

    // Sets the slot history for the previous epoch, since the current epoch is not yet complete.
    if epoch > 0 {
        cluster_history_account
            .set_blocks(epoch - 1, blocks_in_epoch(epoch - 1, &slot_history)?)?;
    }
    cluster_history_account.set_blocks(epoch, blocks_in_epoch(epoch, &slot_history)?)?;

    cluster_history_account.cluster_history_last_update_slot = clock.slot;

    Ok(())
}

fn blocks_in_epoch(epoch: u16, slot_history: &SlotHistory) -> Result<u32> {
    let epoch_schedule = EpochSchedule::get()?;
    let start_slot = epoch_schedule.get_first_slot_in_epoch(epoch as u64);
    let end_slot = epoch_schedule.get_last_slot_in_epoch(epoch as u64);

    let mut blocks_in_epoch = 0;
    for i in start_slot..=end_slot {
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
                // do nothing
            }
        };
    }

    Ok(blocks_in_epoch)
}
