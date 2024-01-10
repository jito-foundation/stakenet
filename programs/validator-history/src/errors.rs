use anchor_lang::prelude::*;

#[error_code]
pub enum ValidatorHistoryError {
    #[msg("Account already reached proper size, no more allocations allowed")]
    AccountFullySized,
    #[msg("Invalid epoch credits, credits must be greater than previous credits")]
    InvalidEpochCredits,
    #[msg("Epoch is out of range of history")]
    EpochOutOfRange,
    #[msg("Gossip Signature Verification not performed")]
    NotSigVerified,
    #[msg("Gossip Data Invalid")]
    GossipDataInvalid,
    #[msg("Unsupported IP Format, only IpAddr::V4 is supported")]
    UnsupportedIpFormat,
    #[msg("Not enough voting history to create account. Minimum 10 epochs required")]
    NotEnoughVotingHistory,
    #[msg(
        "Gossip data too old. Data cannot be older than the last recorded timestamp for a field"
    )]
    GossipDataTooOld,
    #[msg("Gossip timestamp too far in the future")]
    GossipDataInFuture,
    #[msg("Arithmetic Error (overflow/underflow)")]
    ArithmeticError,
    #[msg("Slot history sysvar is not containing expected slots")]
    SlotHistoryOutOfDate,
}
