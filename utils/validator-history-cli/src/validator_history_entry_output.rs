use serde::{Deserialize, Serialize};
use solana_native_token::lamports_to_sol;
use validator_history::ValidatorHistoryEntry;

#[derive(Serialize, Deserialize)]
pub struct ValidatorHistoryEntryOutput {
    /// Active stake amount, otherwitse NULL
    pub activated_stake_lamports: Option<String>,

    /// MEV commission in basis points, otherwise NULL
    pub mev_commission: Option<String>,

    /// Number of successful votes in current epoch. Not finalized until subsequent epoch,
    /// otherwise NULL
    pub epoch_credits: Option<String>,

    /// Validator commission in points, otherwise NULL
    pub commission: Option<String>,

    /// Client type, otherwise NULL
    pub client_type: Option<String>,

    /// Client version, otherwise NULL,
    pub version: Option<String>,

    /// IP address, otherwise NULL
    pub ip: Option<String>,

    /// Validator's Tip Distribution Account's merkle root upload authority
    pub merkle_root_upload_authority: String,

    /// 0 if not superminority validator, 1 if superminority validator, otherwise NULL
    pub is_superminority: Option<String>,

    /// Rank of validator by stake amount, otherwise NULL
    pub rank: Option<String>,

    /// Most recent updated slot for epoch credits and commission
    pub vote_account_last_update_slot: Option<String>,

    /// MEV earned, stored as 1/100th SOL, otherwise NULL
    pub mev_earned: Option<String>,

    /// Priority Fee commission in basis point
    pub priority_fee_commission: String,

    /// Priority Fee tips that were transferred to the distribution account in lamports
    pub priority_fee_tips: String,

    /// The total priority fees the validator earned for the epoch
    pub total_priority_fees: String,

    /// The number of leader slots the validator had during the epoch
    pub total_leader_slots: String,

    /// The final number of blocks the validator produced during an epoch
    pub blocks_produced: String,

    /// The last slot the block data was last updated at
    pub block_data_updated_at_slot: String,

    /// Validator's Tip Distribution Account's merkle root upload authority
    pub priority_fee_merkle_root_upload_authority: String,
}

impl From<ValidatorHistoryEntry> for ValidatorHistoryEntryOutput {
    fn from(value: ValidatorHistoryEntry) -> Self {
        let default_entry = ValidatorHistoryEntry::default();

        Self {
            activated_stake_lamports: if value
                .activated_stake_lamports
                .eq(&default_entry.activated_stake_lamports)
            {
                None
            } else {
                Some(value.activated_stake_lamports.to_string())
            },
            mev_commission: if value.mev_commission.eq(&default_entry.mev_commission) {
                None
            } else {
                Some(value.mev_commission.to_string())
            },
            epoch_credits: if value.epoch_credits.eq(&default_entry.epoch_credits) {
                None
            } else {
                Some(value.epoch_credits.to_string())
            },
            commission: if value.commission.eq(&default_entry.commission) {
                None
            } else {
                Some(value.commission.to_string())
            },
            client_type: if value.client_type.eq(&default_entry.client_type) {
                None
            } else {
                Some(value.client_type.to_string())
            },
            version: if value.version.major.eq(&default_entry.version.major)
                && value.version.minor.eq(&default_entry.version.minor)
                && value.version.patch.eq(&default_entry.version.patch)
            {
                None
            } else {
                Some(format!(
                    "{}.{}.{}",
                    value.version.major, value.version.minor, value.version.patch
                ))
            },
            ip: if value.ip.eq(&default_entry.ip) {
                None
            } else {
                Some(format!(
                    "{}.{}.{}.{}",
                    value.ip[0], value.ip[1], value.ip[2], value.ip[3]
                ))
            },
            merkle_root_upload_authority: (value.merkle_root_upload_authority as u8).to_string(),
            is_superminority: if value.is_superminority.eq(&default_entry.is_superminority) {
                None
            } else {
                Some(value.is_superminority.to_string())
            },
            rank: if value.rank.eq(&default_entry.rank) {
                None
            } else {
                Some(value.rank.to_string())
            },
            vote_account_last_update_slot: if value
                .vote_account_last_update_slot
                .eq(&default_entry.vote_account_last_update_slot)
            {
                None
            } else {
                Some(value.vote_account_last_update_slot.to_string())
            },
            mev_earned: if value.mev_earned.eq(&default_entry.mev_earned) {
                None
            } else {
                Some((value.mev_earned as f64 / 100.0).to_string())
            },
            priority_fee_commission: value.priority_fee_commission.to_string(),
            priority_fee_tips: lamports_to_sol(value.priority_fee_tips).to_string(),
            total_priority_fees: lamports_to_sol(value.total_priority_fees).to_string(),
            total_leader_slots: value.total_leader_slots.to_string(),
            blocks_produced: value.blocks_produced.to_string(),
            block_data_updated_at_slot: value.block_data_updated_at_slot.to_string(),
            priority_fee_merkle_root_upload_authority: (value
                .priority_fee_merkle_root_upload_authority
                as u8)
                .to_string(),
        }
    }
}
