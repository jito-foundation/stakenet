use {
    crate::{
        errors::ValidatorHistoryError,
        utils::{cast_epoch, cast_epoch_start_timestamp},
        ClusterHistory,
    },
    anchor_lang::{
        prelude::*,
        solana_program::{
            clock::Clock,
            log::sol_log_compute_units,
            slot_history::{Check, MAX_ENTRIES},
        },
    },
};

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

pub fn handle_copy_cluster_info(ctx: Context<CopyClusterInfo>) -> Result<()> {
    let mut cluster_history_account = ctx.accounts.cluster_history_account.load_mut()?;
    let slot_history: Box<SlotHistory> =
        Box::new(bincode::deserialize(&ctx.accounts.slot_history.try_borrow_data()?).unwrap());

    sol_log_compute_units();
    let clock = Clock::get()?;

    let epoch = cast_epoch(clock.epoch)?;

    let epoch_start_timestamp = cast_epoch_start_timestamp(clock.epoch_start_timestamp);

    let epoch_schedule = EpochSchedule::get()?;

    let slot_history = if epoch > 0 {
        let slot_history_next_slot = slot_history.next_slot;
        let start_slot = epoch_schedule.get_first_slot_in_epoch((epoch - 1).into());
        let end_slot = epoch_schedule.get_last_slot_in_epoch((epoch - 1).into());
        let (num_blocks, bitvec_inner) =
            confirmed_blocks_in_epoch(start_slot, end_slot, slot_history)?;
        // Sets the slot history for the previous epoch, since the total number of blocks in the epoch is now final
        cluster_history_account.set_blocks(epoch - 1, num_blocks)?;
        // recreates SlotHistory with same heap memory chunk
        Box::new(SlotHistory {
            bits: bitvec_inner.into(),
            next_slot: slot_history_next_slot,
        })
    } else {
        slot_history
    };

    let start_slot = epoch_schedule.get_first_slot_in_epoch(epoch.into());
    let end_slot = epoch_schedule.get_last_slot_in_epoch(epoch.into());
    let (num_blocks, _) = confirmed_blocks_in_epoch(start_slot, end_slot, slot_history)?;
    cluster_history_account.set_blocks(epoch, num_blocks)?;
    cluster_history_account.set_epoch_start_timestamp(epoch, epoch_start_timestamp)?;

    cluster_history_account.cluster_history_last_update_slot = clock.slot;

    Ok(())
}

const BITVEC_BLOCK_SIZE: u64 = 64;

pub fn confirmed_blocks_in_epoch(
    start_slot: u64,
    end_slot: u64,
    slot_history: Box<SlotHistory>,
) -> Result<(u32, Box<[u64]>)> {
    // The SlotHistory BitVec wraps a slice of "Blocks", usizes representing 64 slots each (different than solana blocks).
    // Iterating through each slot uses too much compute, but we can count the bits of each u64 altogether efficiently
    // with `.count_ones()`.
    // The epoch is not guaranteed to align perfectly with Blocks so we need to count the first and last partial Blocks separately.
    // The bitvec inner data is taken ownership of, then returned to be reused.

    let mut blocks_in_epoch = 0;

    let first_full_block_slot = if (start_slot % MAX_ENTRIES) % BITVEC_BLOCK_SIZE == 0 {
        start_slot
    } else {
        start_slot + (64 - (start_slot % MAX_ENTRIES) % BITVEC_BLOCK_SIZE)
    };

    let last_full_block_slot = if (end_slot % MAX_ENTRIES) % BITVEC_BLOCK_SIZE == 0 {
        end_slot
    } else {
        end_slot - (end_slot % MAX_ENTRIES) % BITVEC_BLOCK_SIZE
    };

    // First and last slots, in partial blocks
    for i in (start_slot..first_full_block_slot).chain(last_full_block_slot..=end_slot) {
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

    let inner_bitvec = slot_history.bits.into_boxed_slice();

    for i in (first_full_block_slot..last_full_block_slot).step_by(BITVEC_BLOCK_SIZE as usize) {
        let block_index = (i % MAX_ENTRIES) / BITVEC_BLOCK_SIZE;
        blocks_in_epoch += inner_bitvec[block_index as usize].count_ones();
    }

    Ok((blocks_in_epoch, inner_bitvec))
}
