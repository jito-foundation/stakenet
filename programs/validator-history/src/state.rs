#[cfg(feature = "idl-build")]
use anchor_lang::idl::{
    types::{IdlEnumVariant, IdlTypeDef, IdlTypeDefTy},
    IdlBuild,
};

use {
    crate::{
        constants::TVC_MULTIPLIER,
        crds_value::{ContactInfo, LegacyContactInfo, LegacyVersion, Version2},
        errors::ValidatorHistoryError,
        utils::{cast_epoch, find_insert_position, get_max_epoch, get_min_epoch},
    },
    anchor_lang::{
        prelude::*,
        solana_program::{pubkey, pubkey::Pubkey},
    },
    borsh::{BorshDeserialize, BorshSerialize},
    bytemuck::{Pod, Zeroable},
    std::{cmp::Ordering, collections::HashMap, mem::size_of, net::IpAddr},
    type_layout::TypeLayout,
};

static_assertions::const_assert_eq!(size_of::<Config>(), 392);

pub static DNE_AUTHORITY: Pubkey = pubkey!("11111111111111111111111111111111");
pub static JITO_LABS_AUTHORITY: Pubkey = pubkey!("GZctHpWXmsZC1YHACTGGcHhYxjdRqQvTpYkb9LMvxDib");
pub static TIP_ROUTER_AUTHORITY: Pubkey = pubkey!("8F4jGUmxF36vQ6yabnsxX6AQVXdKBhs8kGSUuRKSg8Xt");

#[account]
pub struct Config {
    // This program is used to distribute MEV + track which validators are running jito-solana for a given epoch
    pub tip_distribution_program: Pubkey,

    // Has the ability to upgrade config fields
    pub admin: Pubkey,

    // Has the ability to publish data for specific permissioned fields (e.g. stake per validator)
    pub oracle_authority: Pubkey,

    // Tracks number of initialized ValidatorHistory accounts
    pub counter: u32,

    pub bump: u8,

    pub padding0: [u8; 3],

    pub priority_fee_distribution_program: Pubkey,

    pub priority_fee_oracle_authority: Pubkey,

    pub reserve: [u8; 224],
}

impl Default for Config {
    fn default() -> Self {
        Self {
            tip_distribution_program: Default::default(),
            admin: Default::default(),
            oracle_authority: Default::default(),
            counter: Default::default(),
            bump: Default::default(),
            padding0: Default::default(),
            priority_fee_distribution_program: Default::default(),
            priority_fee_oracle_authority: Default::default(),
            reserve: [0u8; 224],
        }
    }
}

impl Config {
    pub const SEED: &'static [u8] = b"config";
    pub const SIZE: usize = 8 + size_of::<Self>();
}

static_assertions::const_assert_eq!(size_of::<ValidatorHistoryEntry>(), 128);

#[derive(BorshSerialize, Copy, Clone, Debug, Default, PartialEq)]
#[repr(u8)]
pub enum MerkleRootUploadAuthority {
    #[default]
    Unset = u8::MAX,
    Other = 1,
    OldJitoLabs = 2,
    TipRouter = 3,
    DNE = 4,
}

unsafe impl Zeroable for MerkleRootUploadAuthority {}
unsafe impl Pod for MerkleRootUploadAuthority {}
// FUTURE UPGRADE
// Add a `merkle_root_upload_authority` mapping to the `Config` struct
impl MerkleRootUploadAuthority {
    pub fn from_pubkey(tda_authority: &Pubkey) -> Self {
        if tda_authority.eq(&JITO_LABS_AUTHORITY) {
            Self::OldJitoLabs
        } else if tda_authority.eq(&TIP_ROUTER_AUTHORITY) {
            Self::TipRouter
        } else if tda_authority.eq(&DNE_AUTHORITY) {
            Self::DNE
        } else {
            Self::Other
        }
    }
}

#[cfg(feature = "idl-build")]
impl IdlBuild for MerkleRootUploadAuthority {
    fn create_type() -> Option<IdlTypeDef> {
        Some(IdlTypeDef {
            name: "MerkleRootUploadAuthority".to_string(),
            ty: IdlTypeDefTy::Enum {
                variants: vec![
                    IdlEnumVariant {
                        name: "Unset".to_string(),
                        fields: None,
                    },
                    IdlEnumVariant {
                        name: "Empty".to_string(),
                        fields: None,
                    },
                    IdlEnumVariant {
                        name: "Other".to_string(),
                        fields: None,
                    },
                    IdlEnumVariant {
                        name: "OldJitoLabs".to_string(),
                        fields: None,
                    },
                    IdlEnumVariant {
                        name: "TipRouter".to_string(),
                        fields: None,
                    },
                ],
            },
            docs: Default::default(),
            generics: Default::default(),
            serialization: Default::default(),
            repr: Default::default(),
        })
    }
}

#[derive(BorshSerialize, TypeLayout)]
#[zero_copy]
pub struct ValidatorHistoryEntry {
    pub activated_stake_lamports: u64,
    pub epoch: u16,
    // MEV commission in basis points
    pub mev_commission: u16,
    // Number of successful votes in current epoch. Not finalized until subsequent epoch
    pub epoch_credits: u32,
    // Validator commission in points
    pub commission: u8,
    // 0 if Solana Labs client, 1 if Jito client, >1 if other
    pub client_type: u8,
    pub version: ClientVersion,
    pub ip: [u8; 4],
    /// The enum mapping of the Validator's Tip Distribution Account's merkle root upload authority
    pub merkle_root_upload_authority: MerkleRootUploadAuthority,
    // 0 if not a superminority validator, 1 if superminority validator
    pub is_superminority: u8,
    // rank of validator by stake amount
    pub rank: u32,
    // Most recent updated slot for epoch credits and commission
    pub vote_account_last_update_slot: u64,
    // MEV earned, stored as 1/100th SOL. mev_earned = 100 means 1.00 SOL earned
    pub mev_earned: u32,
    // Priority Fee commission in basis points
    pub priority_fee_commission: u16,
    pub padding0: [u8; 2],
    // Priority Fee tips that were transferred to the distribution account in lamports
    pub priority_fee_tips: u64,
    // The total priority fees the validator earned for the epoch.
    pub total_priority_fees: u64,
    // The number of leader slots the validator had during the epoch
    pub total_leader_slots: u32,
    // The final number of blocks the validator produced during an epoch
    pub blocks_produced: u32,
    // The last slot the block data was last updated at
    pub block_data_updated_at_slot: u64,
    /// The enum mapping of the Validator's Tip Distribution Account's merkle root upload authority
    pub priority_fee_merkle_root_upload_authority: MerkleRootUploadAuthority,
    pub padding1: [u8; 47],
}

// Default values for fields in `ValidatorHistoryEntry` are the type's max value.
// It's important to ensure that the max value is not a valid value for the field, so we can check if the field has been set.
impl Default for ValidatorHistoryEntry {
    fn default() -> Self {
        Self {
            activated_stake_lamports: u64::MAX,
            epoch: u16::MAX,
            mev_commission: u16::MAX,
            epoch_credits: u32::MAX,
            commission: u8::MAX,
            client_type: u8::MAX,
            version: ClientVersion {
                major: u8::MAX,
                minor: u8::MAX,
                patch: u16::MAX,
            },
            ip: [u8::MAX; 4],
            is_superminority: u8::MAX,
            rank: u32::MAX,
            vote_account_last_update_slot: u64::MAX,
            mev_earned: u32::MAX,
            merkle_root_upload_authority: MerkleRootUploadAuthority::Unset,
            priority_fee_tips: u64::MAX,
            total_priority_fees: u64::MAX,
            priority_fee_commission: u16::MAX,
            total_leader_slots: u32::MAX,
            blocks_produced: u32::MAX,
            padding0: [u8::MAX, 2],
            block_data_updated_at_slot: u64::MAX,
            priority_fee_merkle_root_upload_authority: MerkleRootUploadAuthority::Unset,
            padding1: [u8::MAX; 47],
        }
    }
}

#[derive(BorshSerialize, BorshDeserialize, PartialEq)]
#[zero_copy]
pub struct ClientVersion {
    pub major: u8,
    pub minor: u8,
    pub patch: u16,
}

const MAX_ITEMS: usize = 512;

#[derive(BorshSerialize)]
#[zero_copy]
pub struct CircBuf {
    pub idx: u64,
    pub is_empty: u8,
    pub padding: [u8; 7],
    pub arr: [ValidatorHistoryEntry; MAX_ITEMS],
}

impl Default for CircBuf {
    fn default() -> Self {
        Self {
            arr: [ValidatorHistoryEntry::default(); MAX_ITEMS],
            idx: 0,
            is_empty: 1,
            padding: [0; 7],
        }
    }
}

macro_rules! field_latest {
    ($self:expr, $field:ident) => {
        if let Some(entry) = $self.last() {
            if entry.$field != ValidatorHistoryEntry::default().$field {
                return Some(entry.$field);
            } else {
                None
            }
        } else {
            None
        }
    };
}

macro_rules! field_range {
    ($self:expr, $start_epoch:expr, $end_epoch:expr, $field:ident, $type:ty) => {{
        let epoch_range = $self.epoch_range($start_epoch, $end_epoch);
        epoch_range
            .iter()
            .map(|maybe_entry| {
                maybe_entry
                    .as_ref()
                    .map(|entry| entry.$field)
                    .filter(|&field| field != ValidatorHistoryEntry::default().$field)
            })
            .collect::<Vec<Option<$type>>>()
    }};
}

impl CircBuf {
    pub fn push(&mut self, item: ValidatorHistoryEntry) {
        self.idx = (self.idx + 1) % self.arr.len() as u64;
        self.arr[self.idx as usize] = item;
        self.is_empty = 0;
    }

    pub fn is_empty(&self) -> bool {
        self.is_empty == 1
    }

    pub fn last(&self) -> Option<&ValidatorHistoryEntry> {
        if self.is_empty() {
            None
        } else {
            Some(&self.arr[self.idx as usize])
        }
    }

    pub fn last_mut(&mut self) -> Option<&mut ValidatorHistoryEntry> {
        if self.is_empty() {
            None
        } else {
            Some(&mut self.arr[self.idx as usize])
        }
    }

    pub fn arr_mut(&mut self) -> &mut [ValidatorHistoryEntry] {
        &mut self.arr
    }

    /// Given a new entry and epoch, inserts the entry into the buffer in sorted order
    /// Will not insert if the epoch is out of range or already exists in the buffer
    fn insert(&mut self, entry: ValidatorHistoryEntry, epoch: u16) -> Result<()> {
        if self.is_empty() {
            return Err(ValidatorHistoryError::EpochOutOfRange.into());
        }

        // Find the lowest epoch in the buffer to ensure the new epoch is valid
        let min_epoch = {
            let next_i = (self.idx as usize + 1) % self.arr.len();
            if self.arr[next_i].epoch == ValidatorHistoryEntry::default().epoch {
                self.arr[0].epoch
            } else {
                self.arr[next_i].epoch
            }
        };

        // If epoch is less than min_epoch or greater than max_epoch in the buffer, return error
        if epoch < min_epoch || epoch > self.arr[self.idx as usize].epoch {
            return Err(ValidatorHistoryError::EpochOutOfRange.into());
        }

        let insert_pos = find_insert_position(&self.arr, self.idx as usize, epoch)
            .ok_or(ValidatorHistoryError::DuplicateEpoch)?;

        // If idx < insert_pos, the shifting needs to wrap around
        let end_index = if self.idx < insert_pos as u64 {
            self.idx as usize + self.arr.len()
        } else {
            self.idx as usize
        };

        // Shift all elements to the right to make space for the new entry, starting with current idx
        for i in (insert_pos..=end_index).rev() {
            let i = i % self.arr.len();
            let next_i = (i + 1) % self.arr.len();
            self.arr[next_i] = self.arr[i];
        }

        self.arr[insert_pos] = entry;

        self.idx = (self.idx + 1) % self.arr.len() as u64;
        Ok(())
    }

    /// Returns &ValidatorHistoryEntry for each existing entry in range [start_epoch, end_epoch] inclusive, factoring for wraparound
    /// Returns None for each epoch that doesn't exist in the CircBuf
    pub fn epoch_range(
        &self,
        start_epoch: u16,
        end_epoch: u16,
    ) -> Vec<Option<&ValidatorHistoryEntry>> {
        // creates an iterator that lays out the entries in consecutive order, handling wraparound
        let mut entries = self.arr[(self.idx as usize + 1)..] // if self.idx + 1 == self.arr.len() this will just return an empty slice
            .iter()
            .chain(self.arr[..=(self.idx as usize)].iter())
            .filter(|entry| entry.epoch >= start_epoch && entry.epoch <= end_epoch)
            .peekable();
        (start_epoch..=end_epoch)
            .map(|epoch| {
                if let Some(&entry) = entries.peek() {
                    if entry.epoch == epoch {
                        entries.next();
                        return Some(entry);
                    }
                }
                None
            })
            .collect()
    }

    pub fn commission_latest(&self) -> Option<u8> {
        field_latest!(self, commission)
    }

    pub fn commission_range(&self, start_epoch: u16, end_epoch: u16) -> Vec<Option<u8>> {
        field_range!(self, start_epoch, end_epoch, commission, u8)
    }

    pub fn mev_commission_latest(&self) -> Option<u16> {
        field_latest!(self, mev_commission)
    }

    pub fn mev_commission_range(&self, start_epoch: u16, end_epoch: u16) -> Vec<Option<u16>> {
        field_range!(self, start_epoch, end_epoch, mev_commission, u16)
    }

    pub fn epoch_credits_latest(&self) -> Option<u32> {
        field_latest!(self, epoch_credits)
    }

    pub fn merkle_root_upload_authority_latest(&self) -> Option<MerkleRootUploadAuthority> {
        field_latest!(self, merkle_root_upload_authority)
    }

    pub fn priority_fee_merkle_root_upload_authority_latest(
        &self,
    ) -> Option<MerkleRootUploadAuthority> {
        field_latest!(self, priority_fee_merkle_root_upload_authority)
    }

    pub fn priority_fee_tips_range(&self, start_epoch: u16, end_epoch: u16) -> Vec<Option<u64>> {
        field_range!(self, start_epoch, end_epoch, priority_fee_tips, u64)
    }

    pub fn total_priority_fees_range(&self, start_epoch: u16, end_epoch: u16) -> Vec<Option<u64>> {
        field_range!(self, start_epoch, end_epoch, total_priority_fees, u64)
    }

    pub fn priority_fee_tips_latest(&self) -> Option<u64> {
        field_latest!(self, priority_fee_tips)
    }

    pub fn total_priority_fees_latest(&self) -> Option<u64> {
        field_latest!(self, total_priority_fees)
    }

    pub fn merkle_root_upload_authority_range(
        &self,
        start_epoch: u16,
        end_epoch: u16,
    ) -> Vec<Option<MerkleRootUploadAuthority>> {
        field_range!(
            self,
            start_epoch,
            end_epoch,
            merkle_root_upload_authority,
            MerkleRootUploadAuthority
        )
    }

    pub fn priority_fee_merkle_root_upload_authority_range(
        &self,
        start_epoch: u16,
        end_epoch: u16,
    ) -> Vec<Option<MerkleRootUploadAuthority>> {
        field_range!(
            self,
            start_epoch,
            end_epoch,
            priority_fee_merkle_root_upload_authority,
            MerkleRootUploadAuthority
        )
    }

    pub fn vote_account_last_update_slot_range(
        &self,
        start_epoch: u16,
        end_epoch: u16,
    ) -> Vec<Option<u64>> {
        field_range!(
            self,
            start_epoch,
            end_epoch,
            vote_account_last_update_slot,
            u64
        )
    }

    /// Normalized epoch credits, accounting for Timely Vote Credits making the max number of credits 16x higher
    /// for every epoch starting at `tvc_activation_epoch`
    pub fn epoch_credits_latest_normalized(
        &self,
        current_epoch: u64,
        tvc_activation_epoch: u64,
    ) -> Option<u32> {
        self.epoch_credits_latest().map(|credits| {
            if current_epoch < tvc_activation_epoch {
                credits.saturating_mul(TVC_MULTIPLIER)
            } else {
                credits
            }
        })
    }

    pub fn epoch_credits_range(&self, start_epoch: u16, end_epoch: u16) -> Vec<Option<u32>> {
        field_range!(self, start_epoch, end_epoch, epoch_credits, u32)
    }

    /// Normalized epoch credits, accounting for Timely Vote Credits making the max number of credits 8x higher
    /// for every epoch starting at `tvc_activation_epoch`
    pub fn epoch_credits_range_normalized(
        &self,
        start_epoch: u16,
        end_epoch: u16,
        tvc_activation_epoch: u64,
    ) -> Vec<Option<u32>> {
        field_range!(self, start_epoch, end_epoch, epoch_credits, u32)
            .into_iter()
            .zip(start_epoch..=end_epoch)
            .map(|(maybe_credits, epoch)| {
                maybe_credits.map(|credits| {
                    if (epoch as u64) < tvc_activation_epoch {
                        credits.saturating_mul(TVC_MULTIPLIER)
                    } else {
                        credits
                    }
                })
            })
            .collect()
    }

    pub fn superminority_latest(&self) -> Option<u8> {
        // Protect against unexpected values
        if let Some(value) = field_latest!(self, is_superminority) {
            if value == 0 || value == 1 {
                return Some(value);
            }
        }
        None
    }

    pub fn superminority_range(&self, start_epoch: u16, end_epoch: u16) -> Vec<Option<u8>> {
        field_range!(self, start_epoch, end_epoch, is_superminority, u8)
            .into_iter()
            .map(|maybe_value| {
                maybe_value.and_then(|value| {
                    if value == 0 || value == 1 {
                        Some(value)
                    } else {
                        None
                    }
                })
            })
            .collect()
    }

    pub fn vote_account_last_update_slot_latest(&self) -> Option<u64> {
        field_latest!(self, vote_account_last_update_slot)
    }

    pub fn activated_stake_lamports_latest(&self) -> Option<u64> {
        field_latest!(self, activated_stake_lamports)
    }

    pub fn activated_stake_lamports_range(
        &self,
        start_epoch: u16,
        end_epoch: u16,
    ) -> Vec<Option<u64>> {
        field_range!(self, start_epoch, end_epoch, activated_stake_lamports, u64)
    }

    pub fn client_type_latest(&self) -> Option<u8> {
        field_latest!(self, client_type)
    }

    pub fn client_type_range(&self, start_epoch: u16, end_epoch: u16) -> Vec<Option<u8>> {
        field_range!(self, start_epoch, end_epoch, client_type, u8)
    }

    pub fn version_latest(&self) -> Option<ClientVersion> {
        field_latest!(self, version)
    }

    pub fn version_range(&self, start_epoch: u16, end_epoch: u16) -> Vec<Option<ClientVersion>> {
        field_range!(self, start_epoch, end_epoch, version, ClientVersion)
    }

    pub fn ip_latest(&self) -> Option<[u8; 4]> {
        field_latest!(self, ip)
    }

    pub fn ip_range(&self, start_epoch: u16, end_epoch: u16) -> Vec<Option<[u8; 4]>> {
        field_range!(self, start_epoch, end_epoch, ip, [u8; 4])
    }

    pub fn rank_latest(&self) -> Option<u32> {
        field_latest!(self, rank)
    }

    pub fn rank_range(&self, start_epoch: u16, end_epoch: u16) -> Vec<Option<u32>> {
        field_range!(self, start_epoch, end_epoch, rank, u32)
    }

    pub fn mev_earned_latest(&self) -> Option<u32> {
        field_latest!(self, mev_earned)
    }

    pub fn mev_earned_range(&self, start_epoch: u16, end_epoch: u16) -> Vec<Option<u32>> {
        field_range!(self, start_epoch, end_epoch, mev_earned, u32)
    }

    pub fn priority_fee_commission_latest(&self) -> Option<u16> {
        field_latest!(self, priority_fee_commission)
    }

    pub fn priority_fee_commission_range(
        &self,
        start_epoch: u16,
        end_epoch: u16,
    ) -> Vec<Option<u16>> {
        field_range!(self, start_epoch, end_epoch, priority_fee_commission, u16)
    }

    pub fn total_leader_slots_latest(&self) -> Option<u32> {
        field_latest!(self, total_leader_slots)
    }

    pub fn total_leader_slots_range(&self, start_epoch: u16, end_epoch: u16) -> Vec<Option<u32>> {
        field_range!(self, start_epoch, end_epoch, total_leader_slots, u32)
    }

    pub fn blocks_produced_latest(&self) -> Option<u32> {
        field_latest!(self, blocks_produced)
    }

    pub fn blocks_produced_range(&self, start_epoch: u16, end_epoch: u16) -> Vec<Option<u32>> {
        field_range!(self, start_epoch, end_epoch, blocks_produced, u32)
    }

    pub fn block_data_updated_at_slot_latest(&self) -> Option<u64> {
        field_latest!(self, block_data_updated_at_slot)
    }

    pub fn block_data_updated_at_slot_range(
        &self,
        start_epoch: u16,
        end_epoch: u16,
    ) -> Vec<Option<u64>> {
        field_range!(
            self,
            start_epoch,
            end_epoch,
            block_data_updated_at_slot,
            u64
        )
    }
}

pub enum ValidatorHistoryVersion {
    V0 = 0,
}

static_assertions::const_assert_eq!(size_of::<ValidatorHistory>(), 65848);

#[derive(BorshSerialize)]
#[account(zero_copy)]
pub struct ValidatorHistory {
    // Cannot be enum due to Pod and Zeroable trait limitations
    pub struct_version: u32,

    pub vote_account: Pubkey,
    // Index of validator of all ValidatorHistory accounts
    pub index: u32,

    pub bump: u8,

    pub _padding0: [u8; 7],

    // These Crds gossip values are only signed and dated once upon startup and then never updated
    // so we track latest time on-chain to make sure old messages aren't uploaded
    pub last_ip_timestamp: u64,
    pub last_version_timestamp: u64,

    // Persistent validator age tracking
    pub validator_age: u32, // Total epochs with non-zero vote credits
    pub validator_age_last_updated_epoch: u16, // Last epoch when age was updated

    pub _padding1: [u8; 226], // Reduced from 232 bytes to accommodate age fields

    pub history: CircBuf,
}

impl ValidatorHistory {
    pub const SIZE: usize = 8 + size_of::<Self>();
    pub const MAX_ITEMS: usize = MAX_ITEMS;
    pub const SEED: &'static [u8] = b"validator-history";

    pub fn set_mev_commission(
        &mut self,
        epoch: u16,
        commission: u16,
        mev_earned: u32,
        merkle_root_upload_authority: MerkleRootUploadAuthority,
    ) -> Result<()> {
        if let Some(entry) = self.history.last_mut() {
            match entry.epoch.cmp(&epoch) {
                Ordering::Equal => {
                    entry.mev_earned = mev_earned;
                    entry.mev_commission = commission;
                    entry.merkle_root_upload_authority = merkle_root_upload_authority;
                    return Ok(());
                }
                Ordering::Greater => {
                    if let Some(entry) = self
                        .history
                        .arr_mut()
                        .iter_mut()
                        .find(|entry| entry.epoch == epoch)
                    {
                        entry.mev_earned = mev_earned;
                        entry.mev_commission = commission;
                        entry.merkle_root_upload_authority = merkle_root_upload_authority;
                    }
                    return Ok(());
                }
                Ordering::Less => {}
            }
        }
        let entry = ValidatorHistoryEntry {
            epoch,
            mev_commission: commission,
            mev_earned,
            merkle_root_upload_authority,
            ..ValidatorHistoryEntry::default()
        };
        self.history.push(entry);

        Ok(())
    }

    pub fn set_priority_fees_transferred_and_commission(
        &mut self,
        epoch: u16,
        commission: u16,
        priority_fee_tips: u64,
        merkle_root_upload_authority: MerkleRootUploadAuthority,
    ) -> Result<()> {
        if let Some(entry) = self.history.last_mut() {
            match entry.epoch.cmp(&epoch) {
                Ordering::Equal => {
                    entry.priority_fee_commission = commission;
                    entry.priority_fee_tips = priority_fee_tips;
                    entry.priority_fee_merkle_root_upload_authority = merkle_root_upload_authority;
                    return Ok(());
                }
                Ordering::Greater => {
                    if let Some(entry) = self
                        .history
                        .arr_mut()
                        .iter_mut()
                        .find(|entry| entry.epoch == epoch)
                    {
                        entry.priority_fee_commission = commission;
                        entry.priority_fee_tips = priority_fee_tips;
                        entry.priority_fee_merkle_root_upload_authority =
                            merkle_root_upload_authority;
                    }
                    return Ok(());
                }
                Ordering::Less => {}
            }
        }
        let entry = ValidatorHistoryEntry {
            epoch,
            priority_fee_commission: commission,
            priority_fee_tips,
            priority_fee_merkle_root_upload_authority: merkle_root_upload_authority,
            ..ValidatorHistoryEntry::default()
        };
        self.history.push(entry);

        Ok(())
    }

    pub fn set_stake(
        &mut self,
        epoch: u16,
        stake: u64,
        rank: u32,
        is_superminority: bool,
    ) -> Result<()> {
        // Only one authority for upload here, so any epoch can be updated in case of missed upload
        if let Some(entry) = self.history.last_mut() {
            match entry.epoch.cmp(&epoch) {
                Ordering::Equal => {
                    entry.activated_stake_lamports = stake;
                    entry.rank = rank;
                    entry.is_superminority = is_superminority as u8;
                    return Ok(());
                }
                Ordering::Greater => {
                    for entry in self.history.arr_mut().iter_mut() {
                        if entry.epoch == epoch {
                            entry.activated_stake_lamports = stake;
                            entry.rank = rank;
                            entry.is_superminority = is_superminority as u8;
                            return Ok(());
                        }
                    }
                    return Err(ValidatorHistoryError::EpochOutOfRange.into());
                }
                Ordering::Less => {}
            }
        }
        let entry = ValidatorHistoryEntry {
            epoch,
            activated_stake_lamports: stake,
            rank,
            is_superminority: is_superminority as u8,
            ..ValidatorHistoryEntry::default()
        };
        self.history.push(entry);
        Ok(())
    }

    pub fn set_total_priority_fees_and_block_metadata(
        &mut self,
        epoch: u16,
        total_priority_fees: u64,
        total_leader_slots: u32,
        blocks_produced: u32,
        highest_oracle_recorded_slot: u64,
    ) -> Result<()> {
        // Only one authority for upload here, so any epoch can be updated in case of missed upload
        if let Some(entry) = self.history.last_mut() {
            match entry.epoch.cmp(&epoch) {
                Ordering::Equal => {
                    entry.total_priority_fees = total_priority_fees;
                    entry.total_leader_slots = total_leader_slots;
                    entry.blocks_produced = blocks_produced;
                    entry.block_data_updated_at_slot = highest_oracle_recorded_slot;
                    return Ok(());
                }
                Ordering::Greater => {
                    for entry in self.history.arr_mut().iter_mut() {
                        if entry.epoch == epoch {
                            entry.total_priority_fees = total_priority_fees;
                            entry.total_leader_slots = total_leader_slots;
                            entry.blocks_produced = blocks_produced;
                            entry.block_data_updated_at_slot = highest_oracle_recorded_slot;
                            return Ok(());
                        }
                    }
                    return Err(ValidatorHistoryError::EpochOutOfRange.into());
                }
                Ordering::Less => {}
            }
        }
        let entry = ValidatorHistoryEntry {
            epoch,
            total_priority_fees,
            total_leader_slots,
            blocks_produced,
            block_data_updated_at_slot: highest_oracle_recorded_slot,
            ..ValidatorHistoryEntry::default()
        };
        self.history.push(entry);
        Ok(())
    }

    /// Given epoch credits from the vote account, determines which entries do not exist in the history and inserts them.
    /// Shifts all existing entries that come later in the history and evicts the oldest entries if the buffer is full.
    /// Skips entries which are not already in the (min_epoch, max_epoch) range of the buffer.
    pub fn insert_missing_entries(
        &mut self,
        epoch_credits: &[(
            u64, /* epoch */
            u64, /* epoch cumulative votes */
            u64, /* prev epoch cumulative votes */
        )],
    ) -> Result<()> {
        // For each epoch in the list, insert a new entry if it doesn't exist
        let start_epoch = get_min_epoch(epoch_credits)?;
        let end_epoch = get_max_epoch(epoch_credits)?;

        let entries = self
            .history
            .epoch_range(start_epoch, end_epoch)
            .iter()
            .map(|entry| entry.is_some())
            .collect::<Vec<bool>>();

        let epoch_credits_map: HashMap<u16, u32> =
            HashMap::from_iter(epoch_credits.iter().map(|(epoch, cur, prev)| {
                (
                    cast_epoch(*epoch).unwrap(), // all epochs in list will be valid if current epoch is valid
                    (cur.checked_sub(*prev)
                        .ok_or(ValidatorHistoryError::InvalidEpochCredits)
                        .unwrap() as u32),
                )
            }));

        for (entry_is_some, epoch) in entries.iter().zip(start_epoch as u16..=end_epoch) {
            if !*entry_is_some && epoch_credits_map.contains_key(&epoch) {
                // Inserts blank entry that will have credits copied to it later
                let entry = ValidatorHistoryEntry {
                    epoch,
                    ..ValidatorHistoryEntry::default()
                };
                // Skips if epoch is out of range or duplicate
                self.history.insert(entry, epoch).unwrap_or_default();
            }
        }

        Ok(())
    }

    pub fn set_epoch_credits(
        &mut self,
        epoch_credits: &[(
            u64, /* epoch */
            u64, /* epoch cumulative votes */
            u64, /* prev epoch cumulative votes */
        )],
    ) -> Result<()> {
        // Assumes `set_commission` has already been run in `copy_vote_account`,
        // guaranteeing an entry exists for the current epoch
        if epoch_credits.is_empty() {
            return Ok(());
        }
        let epoch_credits_map: HashMap<u16, u32> =
            HashMap::from_iter(epoch_credits.iter().map(|(epoch, cur, prev)| {
                (
                    cast_epoch(*epoch).unwrap(), // all epochs in list will be valid if current epoch is valid
                    (cur.checked_sub(*prev)
                        .ok_or(ValidatorHistoryError::InvalidEpochCredits)
                        .unwrap() as u32),
                )
            }));

        let min_epoch = get_min_epoch(epoch_credits)?;

        // Traverses entries in reverse order, breaking once we hit the lowest epoch in epoch_credits
        let len = self.history.arr.len();
        for i in 0..len {
            let position = (self.history.idx as usize + len - i) % len;
            let entry = &mut self.history.arr[position];
            if let Some(&epoch_credits) = epoch_credits_map.get(&entry.epoch) {
                entry.epoch_credits = epoch_credits;
            }
            if entry.epoch == min_epoch {
                break;
            }
        }

        Ok(())
    }

    pub fn set_commission_and_slot(&mut self, epoch: u16, commission: u8, slot: u64) -> Result<()> {
        if let Some(entry) = self.history.last_mut() {
            match entry.epoch.cmp(&epoch) {
                Ordering::Equal => {
                    entry.commission = commission;
                    entry.vote_account_last_update_slot = slot;
                    return Ok(());
                }
                Ordering::Greater => {
                    if let Some(entry) = self
                        .history
                        .arr_mut()
                        .iter_mut()
                        .find(|entry| entry.epoch == epoch)
                    {
                        entry.commission = commission;
                        entry.vote_account_last_update_slot = slot;
                    }
                    return Ok(());
                }
                Ordering::Less => {}
            }
        }
        let entry = ValidatorHistoryEntry {
            epoch,
            commission,
            vote_account_last_update_slot: slot,
            ..ValidatorHistoryEntry::default()
        };
        self.history.push(entry);

        Ok(())
    }

    pub fn set_contact_info(
        &mut self,
        epoch: u16,
        contact_info: &ContactInfo,
        contact_info_ts: u64,
    ) -> Result<()> {
        let ip = if let IpAddr::V4(address) = contact_info.addrs[0] {
            address.octets()
        } else {
            return Err(ValidatorHistoryError::UnsupportedIpFormat.into());
        };

        if self.last_ip_timestamp > contact_info_ts || self.last_version_timestamp > contact_info_ts
        {
            return Err(ValidatorHistoryError::GossipDataTooOld.into());
        }
        self.last_ip_timestamp = contact_info_ts;
        self.last_version_timestamp = contact_info_ts;

        if let Some(entry) = self.history.last_mut() {
            match entry.epoch.cmp(&epoch) {
                Ordering::Equal => {
                    entry.ip = ip;
                    entry.client_type = contact_info.version.client as u8;
                    entry.version.major = contact_info.version.major as u8;
                    entry.version.minor = contact_info.version.minor as u8;
                    entry.version.patch = contact_info.version.patch;
                    return Ok(());
                }
                Ordering::Greater => {
                    if let Some(entry) = self
                        .history
                        .arr_mut()
                        .iter_mut()
                        .find(|entry| entry.epoch == epoch)
                    {
                        entry.ip = ip;
                        entry.client_type = contact_info.version.client as u8;
                        entry.version.major = contact_info.version.major as u8;
                        entry.version.minor = contact_info.version.minor as u8;
                        entry.version.patch = contact_info.version.patch;
                    }
                    return Ok(());
                }
                Ordering::Less => {}
            }
        }
        let entry = ValidatorHistoryEntry {
            epoch,
            ip,
            client_type: contact_info.version.client as u8,
            version: ClientVersion {
                major: contact_info.version.major as u8,
                minor: contact_info.version.minor as u8,
                patch: contact_info.version.patch,
            },
            ..ValidatorHistoryEntry::default()
        };
        self.history.push(entry);

        Ok(())
    }

    pub fn set_legacy_contact_info(
        &mut self,
        epoch: u16,
        legacy_contact_info: &LegacyContactInfo,
        contact_info_ts: u64,
    ) -> Result<()> {
        let ip = if let IpAddr::V4(address) = legacy_contact_info.gossip.ip() {
            address.octets()
        } else {
            return Err(ValidatorHistoryError::UnsupportedIpFormat.into());
        };
        if self.last_ip_timestamp > contact_info_ts {
            return Err(ValidatorHistoryError::GossipDataTooOld.into());
        }
        self.last_ip_timestamp = contact_info_ts;

        if let Some(entry) = self.history.last_mut() {
            match entry.epoch.cmp(&epoch) {
                Ordering::Equal => {
                    entry.ip = ip;
                    return Ok(());
                }
                Ordering::Greater => {
                    if let Some(entry) = self
                        .history
                        .arr_mut()
                        .iter_mut()
                        .find(|entry| entry.epoch == epoch)
                    {
                        entry.ip = ip;
                    }
                    return Ok(());
                }
                Ordering::Less => {}
            }
        }

        let entry = ValidatorHistoryEntry {
            epoch,
            ip,
            ..ValidatorHistoryEntry::default()
        };
        self.history.push(entry);
        Ok(())
    }

    pub fn set_version(&mut self, epoch: u16, version: &Version2, version_ts: u64) -> Result<()> {
        if self.last_version_timestamp > version_ts {
            return Err(ValidatorHistoryError::GossipDataTooOld.into());
        }
        self.last_version_timestamp = version_ts;

        if let Some(entry) = self.history.last_mut() {
            match entry.epoch.cmp(&epoch) {
                Ordering::Equal => {
                    entry.version.major = version.version.major as u8;
                    entry.version.minor = version.version.minor as u8;
                    entry.version.patch = version.version.patch;
                    return Ok(());
                }
                Ordering::Greater => {
                    if let Some(entry) = self
                        .history
                        .arr_mut()
                        .iter_mut()
                        .find(|entry| entry.epoch == epoch)
                    {
                        entry.version.major = version.version.major as u8;
                        entry.version.minor = version.version.minor as u8;
                        entry.version.patch = version.version.patch;
                    }
                    return Ok(());
                }
                Ordering::Less => {}
            }
        }
        let entry = ValidatorHistoryEntry {
            epoch,
            version: ClientVersion {
                major: version.version.major as u8,
                minor: version.version.minor as u8,
                patch: version.version.patch,
            },
            ..ValidatorHistoryEntry::default()
        };
        self.history.push(entry);
        Ok(())
    }

    pub fn set_legacy_version(
        &mut self,
        epoch: u16,
        legacy_version: &LegacyVersion,
        version_ts: u64,
    ) -> Result<()> {
        if self.last_version_timestamp > version_ts {
            return Err(ValidatorHistoryError::GossipDataTooOld.into());
        }
        self.last_version_timestamp = version_ts;

        if let Some(entry) = self.history.last_mut() {
            match entry.epoch.cmp(&epoch) {
                Ordering::Equal => {
                    entry.version.major = legacy_version.version.major as u8;
                    entry.version.minor = legacy_version.version.minor as u8;
                    entry.version.patch = legacy_version.version.patch;
                    return Ok(());
                }
                Ordering::Greater => {
                    if let Some(entry) = self
                        .history
                        .arr_mut()
                        .iter_mut()
                        .find(|entry| entry.epoch == epoch)
                    {
                        entry.version.major = legacy_version.version.major as u8;
                        entry.version.minor = legacy_version.version.minor as u8;
                        entry.version.patch = legacy_version.version.patch;
                    }
                    return Ok(());
                }
                Ordering::Less => {}
            }
        }
        let entry = ValidatorHistoryEntry {
            epoch,
            version: ClientVersion {
                major: legacy_version.version.major as u8,
                minor: legacy_version.version.minor as u8,
                patch: legacy_version.version.patch,
            },
            ..ValidatorHistoryEntry::default()
        };
        self.history.push(entry);
        Ok(())
    }

    /// Initialize validator age by counting epochs with non-zero vote credits in the circular buffer
    /// This is idempotent - safe to call multiple times
    pub fn initialize_validator_age(&mut self, _current_epoch: u16) {
        // If never initialized, scan the buffer and count epochs with vote credits
        if self.validator_age_last_updated_epoch == 0 {
            let mut age_count = 0u32;
            let mut latest_epoch_with_credits = 0u16;

            // Scan through the circular buffer for epochs with non-zero vote credits
            for entry in self.history.arr.iter() {
                if entry.epoch != u16::MAX
                    && entry.epoch_credits != u32::MAX
                    && entry.epoch_credits > 0
                {
                    age_count = age_count.saturating_add(1);
                    if entry.epoch > latest_epoch_with_credits {
                        latest_epoch_with_credits = entry.epoch;
                    }
                }
            }

            self.validator_age = age_count;
            self.validator_age_last_updated_epoch = latest_epoch_with_credits.max(1);
        }
    }

    /// Update validator age for the current epoch based on epoch credits
    /// This is idempotent - safe to call multiple times in the same epoch
    pub fn update_validator_age(&mut self, epoch: u16) -> Result<()> {
        // Initialize if needed
        if self.validator_age_last_updated_epoch == 0 {
            self.initialize_validator_age(epoch);
        }

        // Check if this epoch has non-zero vote credits
        let has_vote_credits = self.history.arr.iter().any(|entry| {
            entry.epoch == epoch && entry.epoch_credits != u32::MAX && entry.epoch_credits > 0
        });

        // Only update if this is a new epoch and validator had vote credits
        if epoch > self.validator_age_last_updated_epoch && has_vote_credits {
            self.validator_age = self.validator_age.saturating_add(1);
            self.validator_age_last_updated_epoch = epoch;
        }

        Ok(())
    }

    /// Get the validator age (total epochs with non-zero vote credits)
    pub fn validator_age(&self) -> u32 {
        self.validator_age
    }
}

#[derive(BorshSerialize)]
#[account(zero_copy)]
pub struct ClusterHistory {
    pub struct_version: u64,
    pub bump: u8,
    pub _padding0: [u8; 7],
    pub cluster_history_last_update_slot: u64,
    pub _padding1: [u8; 232],
    pub history: CircBufCluster,
}

#[derive(BorshSerialize)]
#[zero_copy]
pub struct ClusterHistoryEntry {
    pub total_blocks: u32,
    pub epoch: u16,
    pub padding0: [u8; 2],
    pub epoch_start_timestamp: u64,
    pub padding: [u8; 240],
}

impl Default for ClusterHistoryEntry {
    fn default() -> Self {
        Self {
            total_blocks: u32::MAX,
            epoch: u16::MAX,
            padding0: [u8::MAX; 2],
            epoch_start_timestamp: u64::MAX,
            padding: [u8::MAX; 240],
        }
    }
}

#[derive(BorshSerialize)]
#[zero_copy]
pub struct CircBufCluster {
    pub idx: u64,
    pub is_empty: u8,
    pub padding: [u8; 7],
    pub arr: [ClusterHistoryEntry; MAX_ITEMS],
}

impl Default for CircBufCluster {
    fn default() -> Self {
        Self {
            arr: [ClusterHistoryEntry::default(); MAX_ITEMS],
            idx: 0,
            is_empty: 1,
            padding: [0; 7],
        }
    }
}

impl CircBufCluster {
    pub fn push(&mut self, item: ClusterHistoryEntry) {
        self.idx = (self.idx + 1) % self.arr.len() as u64;
        self.arr[self.idx as usize] = item;
        self.is_empty = 0;
    }

    pub fn is_empty(&self) -> bool {
        self.is_empty == 1
    }

    pub fn last(&self) -> Option<&ClusterHistoryEntry> {
        if self.is_empty() {
            None
        } else {
            Some(&self.arr[self.idx as usize])
        }
    }

    pub fn last_mut(&mut self) -> Option<&mut ClusterHistoryEntry> {
        if self.is_empty() {
            None
        } else {
            Some(&mut self.arr[self.idx as usize])
        }
    }

    pub fn arr_mut(&mut self) -> &mut [ClusterHistoryEntry] {
        &mut self.arr
    }

    /// Returns &ClusterHistoryEntry for each existing entry in range [start_epoch, end_epoch], factoring for wraparound
    /// Returns None for each epoch that doesn't exist in the CircBuf
    pub fn epoch_range(
        &self,
        start_epoch: u16,
        end_epoch: u16,
    ) -> Vec<Option<&ClusterHistoryEntry>> {
        // creates an iterator that lays out the entries in consecutive order, handling wraparound
        let mut entries = self.arr[(self.idx as usize + 1)..] // if self.idx + 1 == self.arr.len() this will just return an empty slice
            .iter()
            .chain(self.arr[..=(self.idx as usize)].iter())
            .filter(|entry| entry.epoch >= start_epoch && entry.epoch <= end_epoch)
            .peekable();
        (start_epoch..=end_epoch)
            .map(|epoch| {
                if let Some(&entry) = entries.peek() {
                    if entry.epoch == epoch {
                        entries.next();
                        return Some(entry);
                    }
                }
                None
            })
            .collect()
    }

    pub fn total_blocks_latest(&self) -> Option<u32> {
        if let Some(entry) = self.last() {
            if entry.total_blocks != ClusterHistoryEntry::default().total_blocks {
                Some(entry.total_blocks)
            } else {
                None
            }
        } else {
            None
        }
    }

    pub fn total_blocks_range(&self, start_epoch: u16, end_epoch: u16) -> Vec<Option<u32>> {
        let epoch_range = self.epoch_range(start_epoch, end_epoch);
        epoch_range
            .iter()
            .map(|maybe_entry| {
                maybe_entry
                    .as_ref()
                    .map(|entry| entry.total_blocks)
                    .filter(|&field| field != ClusterHistoryEntry::default().total_blocks)
            })
            .collect::<Vec<Option<u32>>>()
    }
}

impl ClusterHistory {
    pub const SIZE: usize = 8 + size_of::<Self>();
    pub const MAX_ITEMS: usize = MAX_ITEMS;
    pub const SEED: &'static [u8] = b"cluster-history";

    // Sets total blocks for the target epoch
    pub fn set_blocks(&mut self, epoch: u16, blocks_in_epoch: u32) -> Result<()> {
        if let Some(entry) = self.history.last_mut() {
            match entry.epoch.cmp(&epoch) {
                Ordering::Equal => {
                    entry.total_blocks = blocks_in_epoch;
                    return Ok(());
                }
                Ordering::Greater => {
                    if let Some(entry) = self
                        .history
                        .arr_mut()
                        .iter_mut()
                        .find(|entry| entry.epoch == epoch)
                    {
                        entry.total_blocks = blocks_in_epoch;
                    }
                    return Ok(());
                }
                Ordering::Less => {}
            }
        }
        let entry = ClusterHistoryEntry {
            epoch,
            total_blocks: blocks_in_epoch,
            ..ClusterHistoryEntry::default()
        };
        self.history.push(entry);

        Ok(())
    }

    pub fn set_epoch_start_timestamp(
        &mut self,
        epoch: u16,
        epoch_start_timestamp: u64,
    ) -> Result<()> {
        // Always called after `set_blocks` so we can assume the entry for this epoch exists
        if let Some(entry) = self.history.last_mut() {
            if entry.epoch == epoch {
                entry.epoch_start_timestamp = epoch_start_timestamp;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Utility test to see struct layout
    #[test]
    fn test_validator_history_layout() {
        println!("{}", ValidatorHistoryEntry::type_layout());
    }

    #[test]
    fn test_epoch_range() {
        // Add in 4 CircBuf entries, with epoch 0, 1, 2, 3
        let mut circ_buf = CircBuf::default();
        for i in 0..4 {
            let entry = ValidatorHistoryEntry {
                epoch: i,
                ..ValidatorHistoryEntry::default()
            };
            circ_buf.push(entry);
        }
        // Test epoch range [0, 3]
        let epoch_range: Vec<Option<&ValidatorHistoryEntry>> = circ_buf.epoch_range(0, 3);
        assert_eq!(
            epoch_range
                .iter()
                .filter_map(|maybe_e| maybe_e.map(|e| e.epoch))
                .collect::<Vec<u16>>(),
            vec![0, 1, 2, 3]
        );

        // Creates a new CircBuf with entries from epochs [0, 1, 3]
        circ_buf = CircBuf::default();
        for i in 0..4 {
            if i == 2 {
                continue;
            }
            let entry = ValidatorHistoryEntry {
                epoch: i,
                ..ValidatorHistoryEntry::default()
            };
            circ_buf.push(entry);
        }

        // Test epoch range [0, 3]
        let epoch_range = circ_buf.epoch_range(0, 3);
        assert_eq!(
            epoch_range
                .iter()
                .map(|maybe_e| maybe_e.map(|e| e.epoch))
                .collect::<Vec<Option<u16>>>(),
            vec![Some(0), Some(1), None, Some(3)]
        );

        // same start and end epoch
        // Test end epoch out of range
        let epoch_range = circ_buf.epoch_range(0, 5);
        assert_eq!(
            epoch_range
                .iter()
                .map(|maybe_e| maybe_e.map(|e| e.epoch))
                .collect::<Vec<Option<u16>>>(),
            vec![Some(0), Some(1), None, Some(3), None, None]
        );

        // None if start epoch is none
        let epoch_range = circ_buf.epoch_range(2, 3);
        assert_eq!(
            epoch_range
                .iter()
                .map(|maybe_e| maybe_e.map(|e| e.epoch))
                .collect::<Vec<Option<u16>>>(),
            vec![None, Some(3)]
        );

        let epoch_range = circ_buf.epoch_range(3, 3);
        assert_eq!(
            epoch_range
                .iter()
                .map(|maybe_e| maybe_e.map(|e| e.epoch))
                .collect::<Vec<Option<u16>>>(),
            vec![Some(3)]
        );

        let epoch_range = circ_buf.epoch_range(4, 3);
        assert_eq!(epoch_range.len(), 0);

        // Create entries that wrap around
        circ_buf = CircBuf::default();
        (0..=circ_buf.arr.len() + 4).for_each(|i| {
            circ_buf.push(ValidatorHistoryEntry {
                epoch: i as u16,
                ..ValidatorHistoryEntry::default()
            })
        });

        let epoch_range =
            circ_buf.epoch_range(circ_buf.arr.len() as u16 - 4, circ_buf.arr.len() as u16 + 4);
        assert_eq!(
            epoch_range
                .iter()
                .filter_map(|maybe_e| maybe_e.map(|e| e.epoch))
                .collect::<Vec<u16>>(),
            vec![508, 509, 510, 511, 512, 513, 514, 515, 516]
        );

        // Test ClusterHistory CircBuf epoch range with wraparound
        let mut cluster_circ_buf = CircBufCluster::default();
        (0..=cluster_circ_buf.arr.len() + 4).for_each(|i| {
            cluster_circ_buf.push(ClusterHistoryEntry {
                epoch: i as u16,
                ..ClusterHistoryEntry::default()
            })
        });
        let epoch_range = cluster_circ_buf.epoch_range(508, 516);
        assert_eq!(
            epoch_range
                .iter()
                .filter_map(|maybe_e| maybe_e.map(|e| e.epoch))
                .collect::<Vec<u16>>(),
            vec![508, 509, 510, 511, 512, 513, 514, 515, 516]
        );

        cluster_circ_buf = CircBufCluster::default();
        for i in 0..4 {
            if i == 2 {
                continue;
            }
            let entry = ClusterHistoryEntry {
                epoch: i,
                ..ClusterHistoryEntry::default()
            };
            cluster_circ_buf.push(entry);
        }

        // Test with None epoch
        let epoch_range = cluster_circ_buf.epoch_range(0, 3);
        assert_eq!(
            epoch_range
                .iter()
                .map(|maybe_e| maybe_e.map(|e| e.epoch))
                .collect::<Vec<Option<u16>>>(),
            vec![Some(0), Some(1), None, Some(3)]
        );
    }

    #[test]
    fn test_insert() {
        let mut default_circ_buf = CircBuf {
            idx: MAX_ITEMS as u64 - 1,
            ..Default::default()
        };
        for _ in 0..MAX_ITEMS {
            let entry = ValidatorHistoryEntry {
                ..ValidatorHistoryEntry::default()
            };
            default_circ_buf.push(entry);
        }
        default_circ_buf.is_empty = 1;

        // Test partially full CircBuf
        let mut circ_buf = default_circ_buf;
        for i in 0..MAX_ITEMS / 2 {
            let entry = ValidatorHistoryEntry {
                epoch: i as u16,
                ..ValidatorHistoryEntry::default()
            };
            // Skip an entry
            if i != 100 {
                circ_buf.push(entry);
            }
        }

        // Insert an entry at epoch 100
        let entry = ValidatorHistoryEntry {
            epoch: 100,
            ..ValidatorHistoryEntry::default()
        };
        circ_buf.insert(entry, 100).unwrap();

        // Check that the entry was inserted
        let range = circ_buf.epoch_range(99, 101);
        let epochs = range
            .iter()
            .filter_map(|maybe_e| maybe_e.map(|e| e.epoch))
            .collect::<Vec<u16>>();
        assert_eq!(epochs, vec![99, 100, 101]);

        // Test full CircBuf with wraparound. Will contain epochs 512-1023, skipping 600 - 610
        let mut circ_buf = default_circ_buf;
        for i in 0..MAX_ITEMS * 2 {
            let entry = ValidatorHistoryEntry {
                epoch: i as u16,
                ..ValidatorHistoryEntry::default()
            };
            if !(600..=610).contains(&i) {
                circ_buf.push(entry);
            }
        }

        // Insert an entry where there are valid entries after idx and insertion position < idx
        let entry = ValidatorHistoryEntry {
            epoch: 600,
            ..ValidatorHistoryEntry::default()
        };
        circ_buf.insert(entry, 600).unwrap();

        let range = circ_buf.epoch_range(599, 601);
        let epochs = range
            .iter()
            .filter_map(|maybe_e| maybe_e.map(|e| e.epoch))
            .collect::<Vec<u16>>();
        assert_eq!(epochs, vec![599, 600]);

        // Insert an entry where insertion position > idx
        let mut circ_buf = default_circ_buf;
        for i in 0..MAX_ITEMS * 3 / 2 {
            let entry = ValidatorHistoryEntry {
                epoch: i as u16,
                ..ValidatorHistoryEntry::default()
            };
            if i != 500 {
                circ_buf.push(entry);
            }
        }
        assert!(circ_buf.last().unwrap().epoch == 767);
        assert!(circ_buf.idx == 254);

        let entry = ValidatorHistoryEntry {
            epoch: 500,
            ..ValidatorHistoryEntry::default()
        };
        circ_buf.insert(entry, 500).unwrap();

        let range = circ_buf.epoch_range(256, 767);
        assert!(range.iter().all(|maybe_e| maybe_e.is_some()));

        // Test wraparound correctly when inserting at the end
        let mut circ_buf = default_circ_buf;
        for i in 0..2 * MAX_ITEMS - 1 {
            let entry = ValidatorHistoryEntry {
                epoch: i as u16,
                ..ValidatorHistoryEntry::default()
            };
            circ_buf.push(entry);
        }
        circ_buf.push(ValidatorHistoryEntry {
            epoch: 2 * MAX_ITEMS as u16,
            ..ValidatorHistoryEntry::default()
        });

        circ_buf
            .insert(
                ValidatorHistoryEntry {
                    epoch: 2 * MAX_ITEMS as u16 - 1,
                    ..ValidatorHistoryEntry::default()
                },
                2 * MAX_ITEMS as u16 - 1,
            )
            .unwrap();
        let range = circ_buf.epoch_range(MAX_ITEMS as u16 + 1, 2 * MAX_ITEMS as u16);

        assert!(range.iter().all(|maybe_e| maybe_e.is_some()));
    }

    #[test]
    fn test_insert_errors() {
        // test insert empty
        let mut circ_buf = CircBuf {
            idx: 0,
            is_empty: 1,
            padding: [0; 7],
            arr: [ValidatorHistoryEntry::default(); MAX_ITEMS],
        };

        let entry = ValidatorHistoryEntry {
            epoch: 10,
            ..ValidatorHistoryEntry::default()
        };

        assert!(
            circ_buf.insert(entry, 10) == Err(Error::from(ValidatorHistoryError::EpochOutOfRange))
        );

        let mut circ_buf = CircBuf {
            idx: 4,
            is_empty: 0,
            padding: [0; 7],
            arr: [ValidatorHistoryEntry::default(); MAX_ITEMS],
        };

        for i in 0..5 {
            circ_buf.arr[i] = ValidatorHistoryEntry {
                epoch: (i * 10) as u16 + 6,
                ..ValidatorHistoryEntry::default()
            };
        }

        let entry = ValidatorHistoryEntry {
            epoch: 5,
            ..ValidatorHistoryEntry::default()
        };

        assert!(
            circ_buf.insert(entry, 5) == Err(Error::from(ValidatorHistoryError::EpochOutOfRange))
        );

        let mut circ_buf = CircBuf {
            idx: 4,
            is_empty: 0,
            padding: [0; 7],
            arr: [ValidatorHistoryEntry::default(); MAX_ITEMS],
        };

        for i in 0..5 {
            circ_buf.arr[i] = ValidatorHistoryEntry {
                epoch: (i * 10) as u16,
                ..ValidatorHistoryEntry::default()
            };
        }

        let entry = ValidatorHistoryEntry {
            epoch: 50,
            ..ValidatorHistoryEntry::default()
        };

        assert!(
            circ_buf.insert(entry, 50) == Err(Error::from(ValidatorHistoryError::EpochOutOfRange))
        );
    }
}
