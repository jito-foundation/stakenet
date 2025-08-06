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
    pub merkle_root_upload_authority: Option<String>,

    /// 0 if not superminority validator, 1 if superminority validator, otherwise NULL
    pub is_superminority: Option<String>,

    /// Rank of validator by stake amount, otherwise NULL
    pub rank: Option<String>,

    /// Most recent updated slot for epoch credits and commission
    pub vote_account_last_update_slot: Option<String>,

    /// MEV earned, stored as 1/100th SOL, otherwise NULL
    pub mev_earned: Option<String>,

    /// Priority Fee commission in basis point
    pub priority_fee_commission: Option<String>,

    /// Priority Fee tips that were transferred to the distribution account in lamports
    pub priority_fee_tips: Option<String>,

    /// The total priority fees the validator earned for the epoch
    pub total_priority_fees: Option<String>,

    /// The number of leader slots the validator had during the epoch
    pub total_leader_slots: Option<String>,

    /// The final number of blocks the validator produced during an epoch
    pub blocks_produced: Option<String>,

    /// The last slot the block data was last updated at
    pub block_data_updated_at_slot: Option<String>,

    /// Validator's Tip Distribution Account's merkle root upload authority
    pub priority_fee_merkle_root_upload_authority: Option<String>,
}

impl From<ValidatorHistoryEntry> for ValidatorHistoryEntryOutput {
    fn from(value: ValidatorHistoryEntry) -> Self {
        let default_entry = ValidatorHistoryEntry::default();

        Self {
            activated_stake_lamports: (!value
                .activated_stake_lamports
                .eq(&default_entry.activated_stake_lamports))
            .then_some(value.activated_stake_lamports.to_string()),

            mev_commission: (!value.mev_commission.eq(&default_entry.mev_commission))
                .then_some(value.mev_commission.to_string()),

            epoch_credits: (!value.epoch_credits.eq(&default_entry.epoch_credits))
                .then_some(value.epoch_credits.to_string()),

            commission: (!value.commission.eq(&default_entry.commission))
                .then_some(value.commission.to_string()),

            client_type: (!value.client_type.eq(&default_entry.client_type))
                .then_some(value.client_type.to_string()),

            version: (!(value.version.major.eq(&default_entry.version.major)
                && value.version.minor.eq(&default_entry.version.minor)
                && value.version.patch.eq(&default_entry.version.patch)))
            .then_some(format!(
                "{}.{}.{}",
                value.version.major, value.version.minor, value.version.patch
            )),

            ip: (!value.ip.eq(&default_entry.ip)).then_some(format!(
                "{}.{}.{}.{}",
                value.ip[0], value.ip[1], value.ip[2], value.ip[3]
            )),

            merkle_root_upload_authority: (!value
                .merkle_root_upload_authority
                .eq(&default_entry.merkle_root_upload_authority))
            .then_some((value.merkle_root_upload_authority as u8).to_string()),

            is_superminority: (!value.is_superminority.eq(&default_entry.is_superminority))
                .then_some(value.is_superminority.to_string()),

            rank: (!value.rank.eq(&default_entry.rank)).then_some(value.rank.to_string()),

            vote_account_last_update_slot: (!value
                .vote_account_last_update_slot
                .eq(&default_entry.vote_account_last_update_slot))
            .then_some(value.vote_account_last_update_slot.to_string()),

            mev_earned: (!value.mev_earned.eq(&default_entry.mev_earned))
                .then_some((value.mev_earned as f64 / 100.0).to_string()),

            priority_fee_commission: (!value
                .priority_fee_commission
                .eq(&default_entry.priority_fee_commission))
            .then_some(value.priority_fee_commission.to_string()),

            priority_fee_tips: (!value.priority_fee_tips.eq(&default_entry.priority_fee_tips))
                .then_some(lamports_to_sol(value.priority_fee_tips).to_string()),

            total_priority_fees: (!value
                .total_priority_fees
                .eq(&default_entry.total_priority_fees))
            .then_some(lamports_to_sol(value.total_priority_fees).to_string()),

            total_leader_slots: (!value
                .total_leader_slots
                .eq(&default_entry.total_leader_slots))
            .then_some(value.total_leader_slots.to_string()),

            blocks_produced: (!value.blocks_produced.eq(&default_entry.blocks_produced))
                .then_some(value.blocks_produced.to_string()),

            block_data_updated_at_slot: (!value
                .block_data_updated_at_slot
                .eq(&default_entry.block_data_updated_at_slot))
            .then_some(value.block_data_updated_at_slot.to_string()),

            priority_fee_merkle_root_upload_authority: (!value
                .priority_fee_merkle_root_upload_authority
                .eq(&default_entry.priority_fee_merkle_root_upload_authority))
            .then_some((value.priority_fee_merkle_root_upload_authority as u8).to_string()),
        }
    }
}
