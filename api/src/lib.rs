use error::ApiError;
use serde_derive::{Deserialize, Serialize};
use solana_program::pubkey::Pubkey;
use validator_history::{ClientVersion, ValidatorHistory, ValidatorHistoryEntry};

pub mod error;
pub mod router;

pub type Result<T> = std::result::Result<T, ApiError>;

#[derive(Serialize, Deserialize)]
pub(crate) struct ValidatorHistoryResponse {
    /// Cannot be enum due to Pod and Zeroable trait limitations
    pub(crate) struct_version: u32,

    pub(crate) vote_account: Pubkey,
    /// Index of validator of all ValidatorHistory accounts
    pub(crate) index: u32,

    /// These Crds gossip values are only signed and dated once upon startup and then never updated
    /// so we track latest time on-chain to make sure old messages aren't uploaded
    pub(crate) last_ip_timestamp: u64,
    pub(crate) last_version_timestamp: u64,

    pub(crate) history: Vec<ValidatorHistoryEntryResponse>,
}

impl ValidatorHistoryResponse {
    pub fn from_validator_history(
        acc: ValidatorHistory,
        history_entries: Vec<ValidatorHistoryEntryResponse>,
    ) -> Self {
        Self {
            struct_version: acc.struct_version,
            vote_account: acc.vote_account,
            index: acc.index,
            last_ip_timestamp: acc.last_ip_timestamp,
            last_version_timestamp: acc.last_version_timestamp,
            history: history_entries,
        }
    }
}

#[derive(Serialize, Deserialize)]
pub(crate) struct ValidatorHistoryEntryResponse {
    pub(crate) activated_stake_lamports: u64,
    pub(crate) epoch: u16,

    // MEV commission in basis points
    pub(crate) mev_commission: u16,

    // Number of successful votes in current epoch. Not finalized until subsequent epoch
    pub(crate) epoch_credits: u32,

    // Validator commission in points
    pub(crate) commission: u8,

    // 0 if Solana Labs client, 1 if Jito client, >1 if other
    pub(crate) client_type: u8,
    pub(crate) version: ClientVersionResponse,
    pub(crate) ip: [u8; 4],

    // 0 if not a superminority validator, 1 if superminority validator
    pub(crate) is_superminority: u8,

    // rank of validator by stake amount
    pub(crate) rank: u32,

    // Most recent updated slot for epoch credits and commission
    pub(crate) vote_account_last_update_slot: u64,

    // MEV earned, stored as 1/100th SOL. mev_earned = 100 means 1.00 SOL earned
    pub(crate) mev_earned: u32,
}

impl ValidatorHistoryEntryResponse {
    pub fn from_validator_history_entry(entry: &ValidatorHistoryEntry) -> Self {
        let version = ClientVersionResponse::from_client_version(entry.version);
        Self {
            activated_stake_lamports: entry.activated_stake_lamports,
            epoch: entry.epoch,
            mev_commission: entry.mev_commission,
            epoch_credits: entry.epoch_credits,
            commission: entry.commission,
            client_type: entry.client_type,
            version,
            ip: entry.ip,
            is_superminority: entry.is_superminority,
            rank: entry.rank,
            vote_account_last_update_slot: entry.vote_account_last_update_slot,
            mev_earned: entry.mev_earned,
        }
    }
}

#[derive(Serialize, Deserialize)]
pub(crate) struct ClientVersionResponse {
    pub(crate) major: u8,
    pub(crate) minor: u8,
    pub(crate) patch: u16,
}

impl ClientVersionResponse {
    pub fn from_client_version(version: ClientVersion) -> Self {
        Self {
            major: version.major,
            minor: version.minor,
            patch: version.patch,
        }
    }
}
