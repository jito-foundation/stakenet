pub const MAX_ALLOC_BYTES: usize = 10240;
pub const MIN_VOTE_EPOCHS: usize = 5;
pub const TVC_MULTIPLIER: u32 = 16;
/// The [`crate::ValidatorHistory`] .validator_age and .validator_age_last_updated_epoch fields were previously padding bytes
/// and were migrated from zeroed bytes and so on first observation (default) will be zero.
pub const VALIDATOR_AGE_EPOCH_DEFAULT: u16 = 0;
