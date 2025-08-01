use serde::{Deserialize, Serialize};
use solana_native_token::lamports_to_sol;
use validator_history::ValidatorHistoryEntry;

#[derive(Serialize, Deserialize)]
pub struct ValidatorHistoryEntryOutput {
    /// Validator commission in points, otherwise NULL
    pub commission: String,

    /// Number of successful votes in current epoch. Not finalized until subsequent epoch,
    /// otherwise NULL
    pub epoch_credits: String,

    /// MEV commission in basis points, otherwise NULL
    pub mev_commission: String,

    /// MEV earned, stored as 1/100th SOL, otherwise NULL
    pub mev_earned: String,

    /// Active stake amount, otherwitse NULL
    pub stake: String,

    /// IP address, otherwise NULL
    pub ip: String,

    /// Client type, otherwise NULL
    pub client_type: String,

    /// Client version, otherwise NULL,
    pub client_version: String,

    /// Rank of validator by stake amount, otherwise NULL
    pub rank: String,

    /// 0 if not superminority validator, 1 if superminority validator, otherwise NULL
    pub superminority: String,

    /// Most recent updated slot for epoch credits and commission, otherwise NULL
    pub last_update_slot: String,

    /// Priority Fee tips that were transferred to the distribution account in lamports,
    /// otherwise NULL
    pub priority_fee_tips: String,

    /// The total priority fees the validator earned for the epoch
    pub total_priority_fees: String,

    /// Priority Fee commission in basis point
    pub priority_fee_commission: String,

    /// The number of leader slots the validator had during the epoch
    pub total_leader_slots: String,
}

impl From<ValidatorHistoryEntry> for ValidatorHistoryEntryOutput {
    fn from(value: ValidatorHistoryEntry) -> Self {
        let default_entry = ValidatorHistoryEntry::default();

        Self {
            commission: if value.commission.eq(&default_entry.commission) {
                "[NULL]".to_string()
            } else {
                value.commission.to_string()
            },
            epoch_credits: if value.epoch_credits.eq(&default_entry.epoch_credits) {
                "[NULL]".to_string()
            } else {
                value.epoch_credits.to_string()
            },
            mev_commission: if value.mev_commission.eq(&default_entry.mev_commission) {
                "[NULL]".to_string()
            } else {
                value.mev_commission.to_string()
            },
            mev_earned: if value.mev_earned.eq(&default_entry.mev_earned) {
                "[NULL]".to_string()
            } else {
                (value.mev_earned as f64 / 100.0).to_string()
            },
            stake: if value
                .activated_stake_lamports
                .eq(&default_entry.activated_stake_lamports)
            {
                "[NULL]".to_string()
            } else {
                value.activated_stake_lamports.to_string()
            },
            ip: if value.ip.eq(&default_entry.ip) {
                "[NULL]".to_string()
            } else {
                format!(
                    "{}.{}.{}.{}",
                    value.ip[0], value.ip[1], value.ip[2], value.ip[3]
                )
            },
            client_type: if value.client_type.eq(&default_entry.client_type) {
                "[NULL]".to_string()
            } else {
                value.client_type.to_string()
            },
            client_version: if value.version.major.eq(&default_entry.version.major)
                && value.version.minor.eq(&default_entry.version.minor)
                && value.version.patch.eq(&default_entry.version.patch)
            {
                "[NULL]".to_string()
            } else {
                format!(
                    "{}.{}.{}",
                    value.version.major, value.version.minor, value.version.patch
                )
            },
            rank: if value.rank.eq(&default_entry.rank) {
                "[NULL]".to_string()
            } else {
                value.rank.to_string()
            },
            superminority: if value.is_superminority.eq(&default_entry.is_superminority) {
                "[NULL]".to_string()
            } else {
                value.is_superminority.to_string()
            },

            last_update_slot: if value
                .vote_account_last_update_slot
                .eq(&default_entry.vote_account_last_update_slot)
            {
                "[NULL]".to_string()
            } else {
                value.vote_account_last_update_slot.to_string()
            },

            priority_fee_tips: lamports_to_sol(value.priority_fee_tips).to_string(),
            total_priority_fees: lamports_to_sol(value.total_priority_fees).to_string(),
            priority_fee_commission: value.priority_fee_commission.to_string(),
            total_leader_slots: value.total_leader_slots.to_string(),
        }
    }
}
