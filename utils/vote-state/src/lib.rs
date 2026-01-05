//! This code was mostly copy-pasta'd from [here](https://github.com/solana-labs/solana/blob/df128573127c324cb5b53634a7e2d77427c6f2d8/programs/vote/src/vote_state/mod.rs#L1).
//! In all current releases [VoteState] is defined in the `solana-vote-program` crate which is not compatible
//! with programs targeting BPF bytecode due to some BPF-incompatible libraries being pulled in.
//! Additional methods added here for deserializing specific fields to get around runtime compute limits.
#![allow(clippy::arithmetic_side_effects)]
#![allow(unexpected_cfgs)]
use std::{
    collections::{BTreeMap, VecDeque},
    mem::size_of,
};

#[allow(deprecated)]
use anchor_lang::{error::ErrorCode::ConstraintOwner, prelude::*, solana_program::vote};
use serde::{Deserialize, Serialize};
use serde_big_array::BigArray;

#[error_code]
pub enum ErrorCode {
    #[msg("Serialized vote account data contents are malformed or not supported.")]
    VoteAccountDataNotValid,
}

type Epoch = u64;
type Slot = u64;
type UnixTimestamp = i64;

// Maximum number of votes to keep around, tightly coupled with epoch_schedule::MINIMUM_SLOTS_PER_EPOCH
pub const MAX_LOCKOUT_HISTORY: usize = 31;
pub const INITIAL_LOCKOUT: usize = 2;

#[derive(Clone, Serialize, Deserialize, Default, Debug, PartialEq, Eq)]
pub struct Lockout {
    pub slot: Slot,
    pub confirmation_count: u32,
}

#[derive(Default, Serialize, Deserialize)]
struct AuthorizedVoters {
    authorized_voters: BTreeMap<Epoch, Pubkey>,
}

const MAX_ITEMS: usize = 32;

#[derive(Default, Serialize, Deserialize)]
pub struct CircBuf<I> {
    buf: [I; MAX_ITEMS],
    /// next pointer
    idx: usize,
    is_empty: bool,
}

#[derive(Clone, Serialize, Deserialize, Default)]
pub struct BlockTimestamp {
    pub slot: Slot,
    pub timestamp: UnixTimestamp,
}

#[derive(Serialize, Default, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct LandedVote {
    // Latency is the difference in slot number between the slot that was voted on (lockout.slot) and the slot in
    // which the vote that added this Lockout landed.  For votes which were cast before versions of the validator
    // software which recorded vote latencies, latency is recorded as 0.
    pub latency: u8,
    pub lockout: Lockout,
}

#[derive(Serialize)]
pub enum VoteStateVersions {
    V0_23_5(Box<VoteState0_23_5>),
    V1_14_11(Box<VoteState1_14_11>),
    V1_16_0(Box<VoteState1_16_0>),
    Current(Box<VoteState>),
}

#[derive(Serialize)]
pub struct VoteState0_23_5 {
    /// the node that votes in this account
    pub node_pubkey: Pubkey,

    /// the signer for vote transactions
    pub authorized_voter: Pubkey,
    /// when the authorized voter was set/initialized
    pub authorized_voter_epoch: Epoch,

    /// history of prior authorized voters and the epoch ranges for which
    ///  they were set
    pub prior_voters: CircBuf<(Pubkey, Epoch, Epoch, Slot)>,

    /// the signer for withdrawals
    pub authorized_withdrawer: Pubkey,
    /// percentage (0-100) that represents what part of a rewards
    /// payout should be given to this VoteAccount
    pub commission: u8,

    pub votes: VecDeque<Lockout>,
    pub root_slot: Option<u64>,

    /// history of how many credits earned by the end of each epoch
    ///  each tuple is (Epoch, credits, prev_credits)
    pub epoch_credits: Vec<(Epoch, u64, u64)>,

    /// most recent timestamp submitted with a vote
    pub last_timestamp: BlockTimestamp,
}

#[derive(Serialize)]
pub struct VoteState1_14_11 {
    /// the node that votes in this account
    pub node_pubkey: Pubkey,

    /// the signer for withdrawals
    pub authorized_withdrawer: Pubkey,
    /// percentage (0-100) that represents what part of a rewards
    ///  payout should be given to this VoteAccount
    pub commission: u8,

    pub votes: VecDeque<Lockout>,

    /// This usually the last Lockout which was popped from self.votes.
    /// However, it can be arbitrary slot, when being used inside Tower
    pub root_slot: Option<Slot>,

    /// the signer for vote transactions
    authorized_voters: AuthorizedVoters,

    /// history of prior authorized voters and the epochs for which
    /// they were set, the bottom end of the range is inclusive,
    /// the top of the range is exclusive
    prior_voters: CircBuf<(Pubkey, Epoch, Epoch)>,

    /// history of how many credits earned by the end of each epoch
    ///  each tuple is (Epoch, credits, prev_credits)
    pub(crate) epoch_credits: Vec<(Epoch, u64, u64)>,

    /// most recent timestamp submitted with a vote
    pub last_timestamp: BlockTimestamp,
}

#[derive(Serialize)]
pub struct VoteState1_16_0 {
    /// the node that votes in this account
    pub node_pubkey: Pubkey,

    /// the signer for withdrawals
    pub authorized_withdrawer: Pubkey,
    /// percentage (0-100) that represents what part of a rewards
    ///  payout should be given to this VoteAccount
    pub commission: u8,

    pub votes: VecDeque<LandedVote>,

    // This usually the last Lockout which was popped from self.votes.
    // However, it can be arbitrary slot, when being used inside Tower
    pub root_slot: Option<Slot>,

    /// the signer for vote transactions
    authorized_voters: AuthorizedVoters,

    /// history of prior authorized voters and the epochs for which
    /// they were set, the bottom end of the range is inclusive,
    /// the top of the range is exclusive
    prior_voters: CircBuf<(Pubkey, Epoch, Epoch)>,

    /// history of how many credits earned by the end of each epoch
    ///  each tuple is (Epoch, credits, prev_credits)
    pub epoch_credits: Vec<(Epoch, u64, u64)>,

    /// most recent timestamp submitted with a vote
    pub last_timestamp: BlockTimestamp,
}

// Newest version as of 3.10.0
#[derive(Serialize)]
pub struct VoteState {
    pub node_pubkey: Pubkey,
    pub authorized_withdrawer: Pubkey,
    pub inflation_rewards_collector: Pubkey,
    pub block_revenue_collector: Pubkey,
    pub inflation_rewards_commission_bps: u16,
    pub block_revenue_commission_bps: u16,
    pub pending_delegator_rewards: u64,
    pub bls_pubkey_compressed: Option<BLSPubkey>,
    pub votes: VecDeque<LandedVote>,
    pub root_slot: Option<u64>,
    authorized_voters: AuthorizedVoters,
    pub(crate) epoch_credits: Vec<(Epoch, u64, u64)>,
    pub last_timestamp: BlockTimestamp,
}

#[derive(Serialize)]
pub struct BLSPubkey {
    #[serde(with = "BigArray")]
    pub bytes: [u8; 48],
}

impl VoteStateVersions {
    // Enum index + (4*Pubkey)
    const VOTE_STATE_COMMISSION_INDEX: usize = 132;
    // Enum index + Pubkey + Pubkey
    const VOTE_STATE_1_16_0_COMMISSION_INDEX: usize = 68;
    const VOTE_STATE_1_14_1_COMMISSION_INDEX: usize = 68;
    // Enum index + Pubkey + Pubkey + Epoch + (CircBuf: 32 * (Pubkey + 2 * Epoch + Slot) + usize + bool) + Pubkey
    const VOTE_STATE_0_23_5_COMMISSION_INDEX: usize = 1909;
    const INFLATION_REWARDS_COMMISSION_BPS_BYTES: usize = 2;
    const BLOCK_REVENUE_COMMISSION_BPS_BYTES: usize = 2;
    const PENDING_DELEGATOR_REWARDS_BYTES: usize = 8;
    const COLLECTION_LEN_BYTES: usize = 8;
    const ENUM_LEN_BYTES: usize = 4;
    const SLOT_BYTES: usize = 8;
    const EPOCH_BYTES: usize = 8;
    const PUBKEY_BYTES: usize = 32;

    /*
    VoteState account is too large to fully deserialize, and can't be zero-copied due to
    not implementing Zeroable, so this method manually extracts the field from the bincode-serialized data
    */
    pub fn deserialize_commission(account_info: &AccountInfo) -> Result<u8> {
        if account_info.owner != &vote::program::ID.key() {
            return Err(ConstraintOwner.into());
        }

        let data = account_info.data.borrow();

        let enum_index = Self::enum_value_at_index(&data, 0)?;
        match enum_index {
            0 => {
                if data.len() < Self::VOTE_STATE_0_23_5_COMMISSION_INDEX {
                    return Err(ErrorCode::VoteAccountDataNotValid.into());
                }
                bincode::deserialize::<u8>(&data[Self::VOTE_STATE_0_23_5_COMMISSION_INDEX..])
                    .map_err(|_| ErrorCode::VoteAccountDataNotValid.into())
            }
            1 => {
                if data.len() < Self::VOTE_STATE_1_14_1_COMMISSION_INDEX {
                    return Err(ErrorCode::VoteAccountDataNotValid.into());
                }
                bincode::deserialize::<u8>(&data[Self::VOTE_STATE_1_14_1_COMMISSION_INDEX..])
                    .map_err(|_| ErrorCode::VoteAccountDataNotValid.into())
            }
            2 => {
                if data.len() < Self::VOTE_STATE_1_16_0_COMMISSION_INDEX {
                    return Err(ErrorCode::VoteAccountDataNotValid.into());
                }
                bincode::deserialize::<u8>(&data[Self::VOTE_STATE_1_16_0_COMMISSION_INDEX..])
                    .map_err(|_| ErrorCode::VoteAccountDataNotValid.into())
            }
            3 => {
                if data.len() < Self::VOTE_STATE_COMMISSION_INDEX {
                    return Err(ErrorCode::VoteAccountDataNotValid.into());
                }
                bincode::deserialize::<u8>(&data[Self::VOTE_STATE_COMMISSION_INDEX..])
                    .map_err(|_| ErrorCode::VoteAccountDataNotValid.into())
            }
            _ => Err(ErrorCode::VoteAccountDataNotValid.into()),
        }
    }

    pub fn deserialize_epoch_credits(account_info: &AccountInfo) -> Result<Vec<(Epoch, u64, u64)>> {
        /*
        VoteState is too large to fully deserialize given the compute budget, so we must manually parse the serialized bincode.
        This function navigates to the proper index, because the existence of several variable-size collections and Options
        before the target field means epoch credits are not at a predefined address.

        Bincode reference: https://github.com/bincode-org/bincode/blob/trunk/docs/spec.md

        Serialized with FixintEncoding.
        Byte size reference:
            bool: 1
            u8: 1
            u32: 4
            u64: 8
            usize: 8
        */
        if account_info.owner != &vote::program::ID.key() {
            return Err(ConstraintOwner.into());
        }

        let data = account_info.data.borrow();
        let enum_index = Self::enum_value_at_index(&data, 0)?;
        match enum_index {
            // VoteState::0_23_5
            0 => {
                let prior_voters_idx: usize =
                    Self::ENUM_LEN_BYTES + 2 * Self::PUBKEY_BYTES + Self::EPOCH_BYTES;
                let prior_voters_size = MAX_ITEMS
                    * (Self::PUBKEY_BYTES + 2 * Self::EPOCH_BYTES + Self::SLOT_BYTES)
                    + 8
                    + 1;

                let votes_idx = prior_voters_idx + prior_voters_size + Self::PUBKEY_BYTES + 1;
                let votes_len = Self::collection_length_at_index(&data, votes_idx)?;

                let root_slot_idx =
                    votes_idx + Self::COLLECTION_LEN_BYTES + (votes_len * (Self::SLOT_BYTES + 4));

                let root_slot_option_variant: u8 = data[root_slot_idx];
                let epoch_credits_idx = match root_slot_option_variant {
                    0 => root_slot_idx + 1,
                    1 => root_slot_idx + 1 + 8,
                    _ => {
                        return Err(ErrorCode::VoteAccountDataNotValid.into());
                    }
                };

                return Self::deserialize_epoch_credits_at_index(&data, epoch_credits_idx);
            }
            // VoteState::Current
            1 => {
                let votes_idx: usize = Self::ENUM_LEN_BYTES + 2 * Self::PUBKEY_BYTES + 1;
                let votes_len = Self::collection_length_at_index(&data, votes_idx)?;

                let root_slot_idx =
                    votes_idx + Self::COLLECTION_LEN_BYTES + (votes_len * (Self::SLOT_BYTES + 4));
                let root_slot_option_variant: u8 = data[root_slot_idx];

                let authorized_voters_idx = match root_slot_option_variant {
                    0 => root_slot_idx + 1,
                    1 => root_slot_idx + 1 + Self::SLOT_BYTES,
                    _ => {
                        return Err(ErrorCode::VoteAccountDataNotValid.into());
                    }
                };
                let authorized_voters_len =
                    Self::collection_length_at_index(&data, authorized_voters_idx)?;

                let prior_voters_len =
                    MAX_ITEMS * (Self::PUBKEY_BYTES + 2 * Self::EPOCH_BYTES) + 8 + 1;

                let epoch_credits_idx: usize = authorized_voters_idx
                    + Self::COLLECTION_LEN_BYTES
                    + authorized_voters_len * (Self::EPOCH_BYTES + Self::PUBKEY_BYTES)
                    + prior_voters_len;

                return Self::deserialize_epoch_credits_at_index(&data, epoch_credits_idx);
            }
            2 => {
                let votes_idx: usize = Self::ENUM_LEN_BYTES + 2 * Self::PUBKEY_BYTES + 1;
                let votes_len = Self::collection_length_at_index(&data, votes_idx)?;

                let root_slot_idx = votes_idx
                    + Self::COLLECTION_LEN_BYTES
                    + (votes_len * (1 + Self::SLOT_BYTES + 4));
                let root_slot_option_variant: u8 = data[root_slot_idx];

                let authorized_voters_idx = match root_slot_option_variant {
                    0 => root_slot_idx + 1,
                    1 => root_slot_idx + 1 + Self::SLOT_BYTES,
                    _ => {
                        return Err(ErrorCode::VoteAccountDataNotValid.into());
                    }
                };
                let authorized_voters_len =
                    Self::collection_length_at_index(&data, authorized_voters_idx)?;

                let prior_voters_len =
                    MAX_ITEMS * (Self::PUBKEY_BYTES + 2 * Self::EPOCH_BYTES) + 8 + 1;

                let epoch_credits_idx: usize = authorized_voters_idx
                    + Self::COLLECTION_LEN_BYTES
                    + authorized_voters_len * (Self::EPOCH_BYTES + Self::PUBKEY_BYTES)
                    + prior_voters_len;

                return Self::deserialize_epoch_credits_at_index(&data, epoch_credits_idx);
            }
            3 => {
                let bls_key_option_variant_idx: usize = Self::ENUM_LEN_BYTES
                    + (4 * Self::PUBKEY_BYTES)
                    + Self::ENUM_LEN_BYTES
                    + (4 * Self::PUBKEY_BYTES)
                    + Self::INFLATION_REWARDS_COMMISSION_BPS_BYTES
                    + Self::BLOCK_REVENUE_COMMISSION_BPS_BYTES
                    + Self::PENDING_DELEGATOR_REWARDS_BYTES;
                let votes_idx = match data[bls_key_option_variant_idx] {
                    0 => bls_key_option_variant_idx + 1,
                    1 => bls_key_option_variant_idx + 1 + 48,
                    _ => {
                        return Err(ErrorCode::VoteAccountDataNotValid.into());
                    }
                };

                let votes_len = Self::collection_length_at_index(&data, votes_idx)?;

                let root_slot_idx = votes_idx
                    + Self::COLLECTION_LEN_BYTES
                    + (votes_len * (1 + Self::SLOT_BYTES + 4));
                let root_slot_option_variant: u8 = data[root_slot_idx];

                let authorized_voters_idx = match root_slot_option_variant {
                    0 => root_slot_idx + 1,
                    1 => root_slot_idx + 1 + Self::SLOT_BYTES,
                    _ => {
                        return Err(ErrorCode::VoteAccountDataNotValid.into());
                    }
                };
                let authorized_voters_len =
                    Self::collection_length_at_index(&data, authorized_voters_idx)?;

                let epoch_credits_idx: usize = authorized_voters_idx
                    + Self::COLLECTION_LEN_BYTES
                    + authorized_voters_len * (Self::EPOCH_BYTES + Self::PUBKEY_BYTES);

                return Self::deserialize_epoch_credits_at_index(&data, epoch_credits_idx);
            }
            _ => {}
        }

        Ok(vec![])
    }

    pub fn deserialize_node_pubkey(account_info: &AccountInfo) -> Result<Pubkey> {
        if account_info.owner != &vote::program::ID.key() {
            return Err(ConstraintOwner.into());
        }

        let data = account_info.data.borrow();
        let node_pubkey_idx = Self::ENUM_LEN_BYTES;
        let node_pubkey_bytes = &data[node_pubkey_idx..node_pubkey_idx + Self::PUBKEY_BYTES];
        bincode::deserialize(node_pubkey_bytes)
            .map_err(|_| ErrorCode::VoteAccountDataNotValid.into())
    }

    fn collection_length_at_index(bincode_data: &[u8], index: usize) -> Result<usize> {
        bincode::deserialize::<u64>(&bincode_data[index..index + Self::COLLECTION_LEN_BYTES])
            .map(|x| x as usize)
            .map_err(|_| ErrorCode::VoteAccountDataNotValid.into())
    }

    fn enum_value_at_index(bincode_data: &[u8], index: usize) -> Result<usize> {
        bincode::deserialize::<u32>(&bincode_data[index..index + Self::ENUM_LEN_BYTES])
            .map(|x| x as usize)
            .map_err(|_| ErrorCode::VoteAccountDataNotValid.into())
    }

    fn deserialize_epoch_credits_at_index(
        bincode_data: &[u8],
        epoch_credits_idx: usize,
    ) -> Result<Vec<(Epoch, u64, u64)>> {
        let epoch_credits_len = Self::collection_length_at_index(bincode_data, epoch_credits_idx)?;
        let epoch_credits_size = epoch_credits_len * size_of::<(Epoch, u64, u64)>();

        let epoch_credits_bytes = &bincode_data[(epoch_credits_idx)
            ..(epoch_credits_idx + Self::COLLECTION_LEN_BYTES + epoch_credits_size)];

        bincode::deserialize(epoch_credits_bytes)
            .map_err(|_| ErrorCode::VoteAccountDataNotValid.into())
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        AuthorizedVoters, BLSPubkey, BlockTimestamp, CircBuf, Lockout, VoteState0_23_5,
        VoteStateVersions, MAX_LOCKOUT_HISTORY,
    };
    #[allow(deprecated)]
    use anchor_lang::{
        prelude::{AccountInfo, Pubkey},
        solana_program::{clock::Epoch, vote},
        Key,
    };
    use std::collections::VecDeque;

    #[test]
    fn test_deserialize_epoch_credits() {
        let test_epoch_credits: Vec<(Epoch, u64, u64)> = vec![(1, 2, 3), (6, 4, 5)];
        let test_votes = VecDeque::from(vec![Lockout::default(); MAX_LOCKOUT_HISTORY]);
        let mut authorized_voters = AuthorizedVoters::default();
        let my_pubkey = Pubkey::new_unique();
        authorized_voters.authorized_voters.insert(99, my_pubkey);
        // Test Current
        // None
        let vote_state = VoteStateVersions::V1_14_11(Box::new(crate::VoteState1_14_11 {
            node_pubkey: Pubkey::new_unique(),
            authorized_withdrawer: Pubkey::new_unique(),
            commission: 96,
            votes: test_votes,
            root_slot: None,
            authorized_voters,
            prior_voters: CircBuf::default(),
            epoch_credits: test_epoch_credits,
            last_timestamp: BlockTimestamp {
                slot: 1,
                timestamp: 2,
            },
        }));

        let mut encoded = bincode::serialize(&vote_state).unwrap();

        let mut lamports: u64 = 0;
        let key = Pubkey::new_unique();
        let owner = vote::program::ID.key();

        let account_info = AccountInfo::new(
            &key,
            false,
            false,
            &mut lamports,
            encoded.as_mut_slice(),
            &owner,
            false,
            0,
        );
        let epoch_credits_result =
            VoteStateVersions::deserialize_epoch_credits(&account_info).unwrap();
        assert!(epoch_credits_result == vec![(1, 2, 3), (6, 4, 5)]);

        // Test V0235
        let test_epoch_credits: Vec<(Epoch, u64, u64)> = vec![(70, 6, 9), (321, 4, 20)];
        let vote_state_0_23_5 = VoteStateVersions::V0_23_5(Box::new(VoteState0_23_5 {
            node_pubkey: Pubkey::new_unique(),
            authorized_voter: Pubkey::new_unique(),
            authorized_voter_epoch: 0,
            prior_voters: CircBuf::default(),
            authorized_withdrawer: Pubkey::new_unique(),
            commission: 69,
            votes: VecDeque::new(),
            root_slot: None,
            epoch_credits: test_epoch_credits,
            last_timestamp: BlockTimestamp::default(),
        }));
        let mut ser_0_23_5 = bincode::serialize(&vote_state_0_23_5).unwrap();
        let account_info = AccountInfo::new(
            &key,
            false,
            false,
            &mut lamports,
            ser_0_23_5.as_mut_slice(),
            &owner,
            false,
            0,
        );
        let epoch_credits_result =
            VoteStateVersions::deserialize_epoch_credits(&account_info).unwrap();
        assert!(epoch_credits_result == vec![(70, 6, 9), (321, 4, 20)]);

        let test_epoch_credits: Vec<(Epoch, u64, u64)> = vec![(70, 9, 6), (321, 20, 4)];
        let vote_state_1_16_0 = VoteStateVersions::V1_16_0(Box::new(crate::VoteState1_16_0 {
            node_pubkey: Pubkey::new_unique(),
            authorized_withdrawer: Pubkey::new_unique(),
            commission: 99,
            votes: VecDeque::new(),
            root_slot: None,
            authorized_voters: AuthorizedVoters::default(),
            prior_voters: CircBuf::default(),
            epoch_credits: test_epoch_credits,
            last_timestamp: BlockTimestamp::default(),
        }));
        let mut ser_1_16_0 = bincode::serialize(&vote_state_1_16_0).unwrap();
        let account_info = AccountInfo::new(
            &key,
            false,
            false,
            &mut lamports,
            ser_1_16_0.as_mut_slice(),
            &owner,
            false,
            0,
        );
        let epoch_credits_result =
            VoteStateVersions::deserialize_epoch_credits(&account_info).unwrap();
        assert!(epoch_credits_result == vec![(70, 9, 6), (321, 20, 4)]);

        // Test empty and non-empty variants of variable length fields

        let test_epoch_credits: Vec<(Epoch, u64, u64)> = vec![(70, 9, 6), (321, 20, 4)];
        let vote_state_current = VoteStateVersions::Current(Box::new(crate::VoteState {
            node_pubkey: Pubkey::new_unique(),
            authorized_withdrawer: Pubkey::new_unique(),
            inflation_rewards_collector: Pubkey::new_unique(),
            block_revenue_collector: Pubkey::new_unique(),
            inflation_rewards_commission_bps: 99,
            block_revenue_commission_bps: 99,
            pending_delegator_rewards: 0,
            bls_pubkey_compressed: Some(BLSPubkey { bytes: [0; 48] }),
            votes: VecDeque::new(),
            root_slot: Some(1),
            authorized_voters: AuthorizedVoters::default(),
            epoch_credits: test_epoch_credits.clone(),
            last_timestamp: BlockTimestamp::default(),
        }));
        let mut ser_current = bincode::serialize(&vote_state_current).unwrap();
        let account_current = AccountInfo::new(
            &key,
            false,
            false,
            &mut lamports,
            ser_current.as_mut_slice(),
            &owner,
            false,
            0,
        );
        let epoch_credits_result =
            VoteStateVersions::deserialize_epoch_credits(&account_current).unwrap();
        assert_eq!(epoch_credits_result, test_epoch_credits);

        let test_epoch_credits: Vec<(Epoch, u64, u64)> = vec![(70, 9, 6), (321, 20, 4)];
        let vote_state_current = VoteStateVersions::Current(Box::new(crate::VoteState {
            node_pubkey: Pubkey::new_unique(),
            authorized_withdrawer: Pubkey::new_unique(),
            inflation_rewards_collector: Pubkey::new_unique(),
            block_revenue_collector: Pubkey::new_unique(),
            inflation_rewards_commission_bps: 99,
            block_revenue_commission_bps: 99,
            pending_delegator_rewards: 0,
            bls_pubkey_compressed: Some(BLSPubkey { bytes: [0; 48] }),
            votes: VecDeque::new(),
            root_slot: None, // Empty root slot
            authorized_voters: AuthorizedVoters::default(),
            epoch_credits: test_epoch_credits.clone(),
            last_timestamp: BlockTimestamp::default(),
        }));
        let mut ser_current = bincode::serialize(&vote_state_current).unwrap();
        let account_current = AccountInfo::new(
            &key,
            false,
            false,
            &mut lamports,
            ser_current.as_mut_slice(),
            &owner,
            false,
            0,
        );
        let epoch_credits_result =
            VoteStateVersions::deserialize_epoch_credits(&account_current).unwrap();
        assert!(epoch_credits_result == test_epoch_credits);

        let test_epoch_credits: Vec<(Epoch, u64, u64)> = vec![(70, 9, 6), (321, 20, 4)];
        let vote_state_current = VoteStateVersions::Current(Box::new(crate::VoteState {
            node_pubkey: Pubkey::new_unique(),
            authorized_withdrawer: Pubkey::new_unique(),
            inflation_rewards_collector: Pubkey::new_unique(),
            block_revenue_collector: Pubkey::new_unique(),
            inflation_rewards_commission_bps: 99,
            block_revenue_commission_bps: 99,
            pending_delegator_rewards: 0,
            bls_pubkey_compressed: None, // Empty BLS pubkey
            votes: VecDeque::new(),
            root_slot: Some(1),
            authorized_voters: AuthorizedVoters::default(),
            epoch_credits: test_epoch_credits.clone(),
            last_timestamp: BlockTimestamp::default(),
        }));
        let mut ser_current = bincode::serialize(&vote_state_current).unwrap();
        let account_current = AccountInfo::new(
            &key,
            false,
            false,
            &mut lamports,
            ser_current.as_mut_slice(),
            &owner,
            false,
            0,
        );
        let epoch_credits_result =
            VoteStateVersions::deserialize_epoch_credits(&account_current).unwrap();
        assert!(epoch_credits_result == test_epoch_credits);

        let test_epoch_credits: Vec<(Epoch, u64, u64)> = vec![(70, 9, 6), (321, 20, 4)];
        let vote_state_current = VoteStateVersions::Current(Box::new(crate::VoteState {
            node_pubkey: Pubkey::new_unique(),
            authorized_withdrawer: Pubkey::new_unique(),
            inflation_rewards_collector: Pubkey::new_unique(),
            block_revenue_collector: Pubkey::new_unique(),
            inflation_rewards_commission_bps: 99,
            block_revenue_commission_bps: 99,
            pending_delegator_rewards: 0,
            bls_pubkey_compressed: None, // Empty BLS pubkey
            votes: VecDeque::new(),
            root_slot: None, // Empty root slot
            authorized_voters: AuthorizedVoters::default(),
            epoch_credits: test_epoch_credits.clone(),
            last_timestamp: BlockTimestamp::default(),
        }));
        let mut ser_current = bincode::serialize(&vote_state_current).unwrap();
        let account_current = AccountInfo::new(
            &key,
            false,
            false,
            &mut lamports,
            ser_current.as_mut_slice(),
            &owner,
            false,
            0,
        );
        let epoch_credits_result =
            VoteStateVersions::deserialize_epoch_credits(&account_current).unwrap();
        assert!(epoch_credits_result == test_epoch_credits);

        let non_empty_authorized_voters = AuthorizedVoters {
            authorized_voters: std::collections::BTreeMap::from([(0, Pubkey::new_unique())]),
        };
        let test_epoch_credits: Vec<(Epoch, u64, u64)> = vec![(70, 9, 6), (321, 20, 4)];
        let vote_state_current = VoteStateVersions::Current(Box::new(crate::VoteState {
            node_pubkey: Pubkey::new_unique(),
            authorized_withdrawer: Pubkey::new_unique(),
            inflation_rewards_collector: Pubkey::new_unique(),
            block_revenue_collector: Pubkey::new_unique(),
            inflation_rewards_commission_bps: 99,
            block_revenue_commission_bps: 99,
            pending_delegator_rewards: 0,
            bls_pubkey_compressed: Some(BLSPubkey { bytes: [0; 48] }),
            votes: VecDeque::new(),
            root_slot: Some(1),
            authorized_voters: non_empty_authorized_voters,
            epoch_credits: test_epoch_credits.clone(),
            last_timestamp: BlockTimestamp::default(),
        }));
        let mut ser_current = bincode::serialize(&vote_state_current).unwrap();
        let account_current = AccountInfo::new(
            &key,
            false,
            false,
            &mut lamports,
            ser_current.as_mut_slice(),
            &owner,
            false,
            0,
        );
        let epoch_credits_result =
            VoteStateVersions::deserialize_epoch_credits(&account_current).unwrap();
        assert!(epoch_credits_result == test_epoch_credits);
    }

    #[test]
    fn test_deserialize_commission() {
        let vote_state = VoteStateVersions::V1_14_11(Box::new(crate::VoteState1_14_11 {
            node_pubkey: Pubkey::new_unique(),
            authorized_withdrawer: Pubkey::new_unique(),
            commission: 96,
            votes: VecDeque::new(),
            root_slot: None,
            authorized_voters: AuthorizedVoters::default(),
            prior_voters: CircBuf::default(),
            epoch_credits: Vec::new(),
            last_timestamp: BlockTimestamp::default(),
        }));

        let mut ser = bincode::serialize(&vote_state).unwrap();

        let mut lamports: u64 = 0;
        let key = Pubkey::new_unique();
        let owner = vote::program::ID.key();

        let account = AccountInfo::new(
            &key,
            false,
            false,
            &mut lamports,
            ser.as_mut_slice(),
            &owner,
            false,
            0,
        );

        assert_eq!(
            VoteStateVersions::deserialize_commission(&account).unwrap(),
            96
        );

        let vote_state_0_23_5 = VoteStateVersions::V0_23_5(Box::new(VoteState0_23_5 {
            node_pubkey: Pubkey::new_unique(),
            authorized_voter: Pubkey::new_unique(),
            authorized_voter_epoch: 0,
            prior_voters: CircBuf::default(),
            authorized_withdrawer: Pubkey::new_unique(),
            commission: 69,
            votes: VecDeque::new(),
            root_slot: None,
            epoch_credits: Vec::new(),
            last_timestamp: BlockTimestamp::default(),
        }));

        let mut ser_0_23_5 = bincode::serialize(&vote_state_0_23_5).unwrap();

        let mut lamports: u64 = 0;
        let key = Pubkey::new_unique();
        let owner = vote::program::ID.key();

        let account_0_23_5 = AccountInfo::new(
            &key,
            false,
            false,
            &mut lamports,
            ser_0_23_5.as_mut_slice(),
            &owner,
            false,
            0,
        );

        assert_eq!(
            VoteStateVersions::deserialize_commission(&account_0_23_5).unwrap(),
            69
        );

        let vote_state_current = VoteStateVersions::V1_16_0(Box::new(crate::VoteState1_16_0 {
            node_pubkey: Pubkey::new_unique(),
            authorized_withdrawer: Pubkey::new_unique(),
            commission: 99,
            votes: VecDeque::new(),
            root_slot: None,
            authorized_voters: AuthorizedVoters::default(),
            prior_voters: CircBuf::default(),
            epoch_credits: Vec::new(),
            last_timestamp: BlockTimestamp::default(),
        }));
        let mut ser_current = bincode::serialize(&vote_state_current).unwrap();
        let account_current = AccountInfo::new(
            &key,
            false,
            false,
            &mut lamports,
            ser_current.as_mut_slice(),
            &owner,
            false,
            0,
        );
        assert_eq!(
            VoteStateVersions::deserialize_commission(&account_current).unwrap(),
            99
        );
    }

    #[test]
    fn test_deserialize_node_pubkey() {
        let node_pubkey = Pubkey::new_unique();
        let vote_state_0_23_5 = VoteStateVersions::V0_23_5(Box::new(VoteState0_23_5 {
            node_pubkey,
            authorized_voter: Pubkey::new_unique(),
            authorized_voter_epoch: 0,
            prior_voters: CircBuf::default(),
            authorized_withdrawer: Pubkey::new_unique(),
            commission: 69,
            votes: VecDeque::new(),
            root_slot: None,
            epoch_credits: Vec::new(),
            last_timestamp: BlockTimestamp::default(),
        }));

        let mut ser = bincode::serialize(&vote_state_0_23_5).unwrap();

        let mut lamports: u64 = 0;
        let key = Pubkey::new_unique();
        let owner = vote::program::ID.key();

        let account = AccountInfo::new(
            &key,
            false,
            false,
            &mut lamports,
            ser.as_mut_slice(),
            &owner,
            false,
            0,
        );

        let node_pubkey_result = VoteStateVersions::deserialize_node_pubkey(&account).unwrap();
        assert_eq!(node_pubkey_result, node_pubkey);

        let vote_state = VoteStateVersions::V1_14_11(Box::new(crate::VoteState1_14_11 {
            node_pubkey,
            authorized_withdrawer: Pubkey::new_unique(),
            commission: 96,
            votes: VecDeque::new(),
            root_slot: None,
            authorized_voters: AuthorizedVoters::default(),
            prior_voters: CircBuf::default(),
            epoch_credits: Vec::new(),
            last_timestamp: BlockTimestamp::default(),
        }));

        let mut ser = bincode::serialize(&vote_state).unwrap();

        let mut lamports: u64 = 0;
        let key = Pubkey::new_unique();
        let owner = vote::program::ID.key();

        let account = AccountInfo::new(
            &key,
            false,
            false,
            &mut lamports,
            ser.as_mut_slice(),
            &owner,
            false,
            0,
        );

        let node_pubkey_result = VoteStateVersions::deserialize_node_pubkey(&account).unwrap();
        assert_eq!(node_pubkey_result, node_pubkey);

        let vote_state_1_16_0 = VoteStateVersions::V1_16_0(Box::new(crate::VoteState1_16_0 {
            node_pubkey,
            authorized_withdrawer: Pubkey::new_unique(),
            commission: 99,
            votes: VecDeque::new(),
            root_slot: None,
            authorized_voters: AuthorizedVoters::default(),
            prior_voters: CircBuf::default(),
            epoch_credits: Vec::new(),
            last_timestamp: BlockTimestamp::default(),
        }));

        let mut ser_1_16_0 = bincode::serialize(&vote_state_1_16_0).unwrap();
        let account_1_16_0 = AccountInfo::new(
            &key,
            false,
            false,
            &mut lamports,
            ser_1_16_0.as_mut_slice(),
            &owner,
            false,
            0,
        );
        let node_pubkey_result =
            VoteStateVersions::deserialize_node_pubkey(&account_1_16_0).unwrap();
        assert_eq!(node_pubkey_result, node_pubkey);

        let vote_state_current = VoteStateVersions::Current(Box::new(crate::VoteState {
            node_pubkey,
            authorized_withdrawer: Pubkey::new_unique(),
            inflation_rewards_collector: Pubkey::new_unique(),
            block_revenue_collector: Pubkey::new_unique(),
            inflation_rewards_commission_bps: 99,
            block_revenue_commission_bps: 99,
            pending_delegator_rewards: 0,
            bls_pubkey_compressed: None,
            votes: VecDeque::new(),
            root_slot: None,
            authorized_voters: AuthorizedVoters::default(),
            epoch_credits: Vec::new(),
            last_timestamp: BlockTimestamp::default(),
        }));

        let mut ser_current = bincode::serialize(&vote_state_current).unwrap();
        let account_current = AccountInfo::new(
            &key,
            false,
            false,
            &mut lamports,
            ser_current.as_mut_slice(),
            &owner,
            false,
            0,
        );
        let node_pubkey_result =
            VoteStateVersions::deserialize_node_pubkey(&account_current).unwrap();
        assert_eq!(node_pubkey_result, node_pubkey);
    }
}
