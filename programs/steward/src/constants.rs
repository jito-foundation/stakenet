pub const MAX_ALLOC_BYTES: usize = 10_240;
pub const VEC_SIZE_BYTES: usize = 4;
pub const PUBKEY_SIZE: usize = 32;
pub const U64_SIZE: usize = 8;
pub const STAKE_STATUS_OFFSET: usize = 40;
pub const VOTE_ADDRESS_OFFSET: usize = 41;
pub const STAKE_POOL_WITHDRAW_SEED: &[u8] = b"withdraw";
pub const STAKE_POOL_TRANSIENT_SEED: &[u8] = b"transient";
pub const MAX_VALIDATORS: usize = 5_000;
pub const BASIS_POINTS_MAX: u16 = 10_000;
pub const COMMISSION_MAX: u8 = 100;
pub const SORTED_INDEX_DEFAULT: u16 = u16::MAX;
pub const LAMPORT_BALANCE_DEFAULT: u64 = u64::MAX;
pub const EPOCH_DEFAULT: u16 = u16::MAX;
// Need at least 1% of slots remaining (4320 slots) to execute steps in state machine
pub const EPOCH_PROGRESS_MAX: f64 = 0.99;
// Cannot go more than 100 epochs without scoring
pub const NUM_EPOCHS_BETWEEN_SCORING_MAX: u64 = 100;
// Cannot score validators in under 100 slots, to submit 1 instruction per validator
pub const COMPUTE_SCORE_SLOT_RANGE_MIN: u64 = 100;
#[cfg(feature = "mainnet-beta")]
pub const VALIDATOR_HISTORY_FIRST_RELIABLE_EPOCH: u64 = 520;
#[cfg(not(feature = "mainnet-beta"))]
pub const VALIDATOR_HISTORY_FIRST_RELIABLE_EPOCH: u64 = 0;
pub const TVC_FEATURE_PUBKEY: &str = "tvcF6b1TRz353zKuhBjinZkKzjmihXmBAHJdjNYw1sQ";
#[cfg(feature = "mainnet-beta")]
pub const TVC_ACTIVATION_EPOCH: u64 = 703;
#[cfg(all(not(feature = "mainnet-beta"), feature = "testnet"))]
pub const TVC_ACTIVATION_EPOCH: u64 = 705;
#[cfg(all(not(feature = "mainnet-beta"), not(feature = "testnet")))]
pub const TVC_ACTIVATION_EPOCH: u64 = 0;
