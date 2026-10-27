//! Entry type for the `UpdateLeaderSlots` instruction.
//!
//! Represents the number of leader slots a validator is scheduled for in an epoch.
//! Implements [`UpdateInstruction`] to generate the Anchor instruction that writes
//! `total_leader_slots` into the validator's history account.

use anchor_lang::{InstructionData, ToAccountMetas};
use solana_sdk::{instruction::Instruction, pubkey::Pubkey};
use stakenet_sdk::{
    models::entries::{Address, UpdateInstruction},
    utils::accounts::{get_validator_history_address, get_validator_history_config_address},
};

/// Holds all data needed to build an `UpdateLeaderSlots` instruction for one validator.
pub struct LeaderSlotsEntry {
    /// Number of leader slots the validator is scheduled for in `epoch`
    pub total_leader_slots: u32,

    /// Vote account
    pub vote_account: Pubkey,

    /// Validator History Address
    pub address: Pubkey,

    /// Validator History Config Address
    pub config: Pubkey,

    /// Signer, must be the config's oracle authority
    pub signer: Pubkey,

    /// Program ID
    pub program_id: Pubkey,

    /// Epoch
    pub epoch: u64,
}

impl LeaderSlotsEntry {
    /// Creates a new entry, deriving the validator history and config PDAs from
    /// the vote account and program ID.
    pub fn new(
        vote_account: Pubkey,
        program_id: &Pubkey,
        signer: &Pubkey,
        epoch: u64,
        total_leader_slots: u32,
    ) -> Self {
        let address = get_validator_history_address(&vote_account, program_id);
        let config = get_validator_history_config_address(program_id);

        Self {
            total_leader_slots,
            vote_account,
            address,
            config,
            signer: *signer,
            program_id: *program_id,
            epoch,
        }
    }
}

impl Address for LeaderSlotsEntry {
    fn address(&self) -> Pubkey {
        self.address
    }
}

impl UpdateInstruction for LeaderSlotsEntry {
    fn update_instruction(&self) -> Instruction {
        Instruction {
            program_id: self.program_id,
            accounts: validator_history::accounts::UpdateLeaderSlots {
                config: self.config,
                validator_history_account: self.address,
                vote_account: self.vote_account,
                oracle_authority: self.signer,
            }
            .to_account_metas(None),
            data: validator_history::instruction::UpdateLeaderSlots {
                epoch: self.epoch,
                total_leader_slots: self.total_leader_slots,
            }
            .data(),
        }
    }
}
