pub mod copy_vote_account_entry;
pub mod crank_copy_directed_stake_targets;
pub mod crank_steward;
pub mod gossip_entry;
pub mod is_bam_connected_entry;
pub mod mev_commission_entry;
pub mod priority_fee_and_block_metadata_entry;
pub mod priority_fee_commission_entry;
pub mod stake_history_entry;

/// `idle`: observed max 12,765 over 4,812 samples (top-1% spread 4.4%).
pub const CU_IDLE: u32 = 39_000;

/// `compute_instant_unstake`: observed max 27,772 over 12,403 samples (top-1% spread 0.3%).
pub const CU_COMPUTE_INSTANT_UNSTAKE: u32 = 84_000;

/// `epoch_maintenance`, no validator removal: observed max 39,643 over 19 samples.
/// The removal branch is unsampled and keeps MAX_COMPUTE_LIMIT.
pub const CU_EPOCH_MAINTENANCE: u32 = 119_000;

/// `compute_delegations`: observed max 22,414, but only 2 samples and it iterates the
/// whole validator list, so this keeps the previous implicit 200k default rather than
/// tightening. Explicit for clarity and for transaction v1, which requires a stated limit.
pub const CU_COMPUTE_DELEGATIONS: u32 = 200_000;

/// `copy_directed_stake_targets`: observed max 13,051 per instruction. Packed 8 per
/// transaction, so this is the per-instruction budget and callers must multiply by the
/// chunk size.
pub const CU_COPY_DIRECTED_STAKE_TARGETS_PER_IX: u32 = 40_000;
