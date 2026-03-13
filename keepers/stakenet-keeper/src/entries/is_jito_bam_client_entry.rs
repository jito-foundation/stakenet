//! Entry type for the `CopyIsJitoBamClient` instruction.
//!
//! Represents a single validator's BAM client status to be written on-chain.
//! Implements [`UpdateInstruction`] to generate the Anchor instruction that
//! copies the BAM participation flag into the validator's history account.

use anchor_lang::{InstructionData, ToAccountMetas};
use solana_sdk::{instruction::Instruction, pubkey::Pubkey};
use stakenet_sdk::{
    models::entries::{Address, UpdateInstruction},
    utils::accounts::{get_validator_history_address, get_validator_history_config_address},
};

/// Holds all data needed to build a `CopyIsJitoBamClient` instruction for one validator.
pub struct IsJitoBamClientEntry {
    /// Is Jito BAM Client
    pub is_jito_bam_client: bool,

    /// Vote account
    pub vote_account: Pubkey,

    /// Validator History Address
    pub address: Pubkey,

    /// Validator History Config Address
    pub config: Pubkey,

    /// Signer
    pub signer: Pubkey,

    /// Program ID
    pub program_id: Pubkey,

    /// Epoch
    pub epoch: u64,
}

impl IsJitoBamClientEntry {
    /// Creates a new entry, deriving the validator history and config PDAs from
    /// the vote account and program ID.
    pub fn new(
        vote_account: Pubkey,
        program_id: &Pubkey,
        signer: &Pubkey,
        epoch: u64,
        is_jito_bam_client: bool,
    ) -> Self {
        let address = get_validator_history_address(&vote_account, program_id);
        let config = get_validator_history_config_address(program_id);

        Self {
            is_jito_bam_client,
            vote_account,
            address,
            config,
            signer: *signer,
            program_id: *program_id,
            epoch,
        }
    }
}

impl Address for IsJitoBamClientEntry {
    fn address(&self) -> Pubkey {
        self.address
    }
}

impl UpdateInstruction for IsJitoBamClientEntry {
    /// Builds the `CopyIsJitoBamClient` instruction, converting the boolean
    /// `is_jito_bam_client` flag to a `u8` for the on-chain program.
    fn update_instruction(&self) -> Instruction {
        Instruction {
            program_id: self.program_id,
            accounts: validator_history::accounts::CopyIsJitoBamClient {
                config: self.config,
                validator_history_account: self.address,
                vote_account: self.vote_account,
                oracle_authority: self.signer,
            }
            .to_account_metas(None),
            data: validator_history::instruction::CopyIsJitoBamClient {
                is_jito_bam_client: self.is_jito_bam_client as u8,
            }
            .data(),
        }
    }
}
