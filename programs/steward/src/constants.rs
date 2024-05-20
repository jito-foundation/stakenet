pub const MAX_ALLOC_BYTES: usize = 10240;
pub const STAKE_POOL_WITHDRAW_SEED: &[u8] = b"withdraw";
pub const STAKE_POOL_TRANSIENT_SEED: &[u8] = b"transient";
pub const MAX_VALIDATORS: usize = 5000;
pub const BASIS_POINTS_MAX: u16 = 10000;
pub const COMMISSION_MAX: u8 = 100;
pub const SORTED_INDEX_DEFAULT: u16 = u16::MAX;
// Need at least 1% of slots remaining (4320 slots) to execute steps in state machine
pub const EPOCH_PROGRESS_MAX: f64 = 0.99;
// Cannot go more than 100 epochs without scoring
pub const NUM_EPOCHS_BETWEEN_SCORING_MAX: u64 = 100;
// Cannot score validators in under 100 slots, to submit 1 instruction per validator
pub const COMPUTE_SCORE_SLOT_RANGE_MIN: usize = 100;
pub const VALIDATOR_HISTORY_FIRST_RELIABLE_EPOCH: usize = 520;
