use anchor_lang::InstructionData;
use anchor_lang::ToAccountMetas;
use bytemuck::{bytes_of, Pod, Zeroable};
use keeper_core::Address;
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction, instruction::Instruction, pubkey::Pubkey,
    signature::Signature,
};

use crate::{derive_validator_history_address, derive_validator_history_config_address};

#[derive(Clone, Debug)]
pub struct GossipEntry {
    pub vote_account: Pubkey,
    pub validator_history_account: Pubkey,
    pub config: Pubkey,
    pub signature: Signature,
    pub message: Vec<u8>,
    pub program_id: Pubkey,
    pub identity: Pubkey,
    pub signer: Pubkey,
}

impl GossipEntry {
    pub fn new(
        vote_account: &Pubkey,
        signature: &Signature,
        message: &[u8],
        program_id: &Pubkey,
        identity: &Pubkey,
        signer: &Pubkey,
    ) -> Self {
        let validator_history_account = derive_validator_history_address(vote_account, program_id);
        let config = derive_validator_history_config_address(program_id);
        Self {
            vote_account: *vote_account,
            validator_history_account,
            config,
            signature: *signature,
            message: message.to_vec(),
            program_id: *program_id,
            identity: *identity,
            signer: *signer,
        }
    }
}

impl Address for GossipEntry {
    fn address(&self) -> Pubkey {
        self.validator_history_account
    }
}

impl GossipEntry {
    pub fn build_update_tx(&self, priority_fee: u64) -> Vec<Instruction> {
        let mut ixs = vec![
            ComputeBudgetInstruction::set_compute_unit_limit(100_000),
            ComputeBudgetInstruction::set_compute_unit_price(priority_fee),
            build_verify_signature_ix(
                self.signature.as_ref(),
                self.identity.to_bytes(),
                &self.message,
            ),
        ];

        ixs.push(Instruction {
            program_id: self.program_id,
            accounts: validator_history::accounts::CopyGossipContactInfo {
                validator_history_account: self.validator_history_account,
                vote_account: self.vote_account,
                instructions: solana_program::sysvar::instructions::id(),
                config: self.config,
                oracle_authority: self.signer,
            }
            .to_account_metas(None),
            data: validator_history::instruction::CopyGossipContactInfo {}.data(),
        });
        ixs
    }
}

// CODE BELOW SLIGHTLY MODIFIED FROM
// solana_sdk/src/ed25519_instruction.rs

pub const PUBKEY_SERIALIZED_SIZE: usize = 32;
pub const SIGNATURE_SERIALIZED_SIZE: usize = 64;
pub const SIGNATURE_OFFSETS_SERIALIZED_SIZE: usize = 14;
// bytemuck requires structures to be aligned
pub const SIGNATURE_OFFSETS_START: usize = 2;
pub const DATA_START: usize = SIGNATURE_OFFSETS_SERIALIZED_SIZE + SIGNATURE_OFFSETS_START;

#[derive(Default, Debug, Copy, Clone, Zeroable, Pod, Eq, PartialEq)]
#[repr(C)]
pub struct Ed25519SignatureOffsets {
    signature_offset: u16,             // offset to ed25519 signature of 64 bytes
    signature_instruction_index: u16,  // instruction index to find signature
    public_key_offset: u16,            // offset to public key of 32 bytes
    public_key_instruction_index: u16, // instruction index to find public key
    message_data_offset: u16,          // offset to start of message data
    message_data_size: u16,            // size of message data
    message_instruction_index: u16,    // index of instruction data to get message data
}

// This code is modified from solana_sdk/src/ed25519_instruction.rs
// due to that function requiring a keypair, and generating the signature within the function.
// In our case we don't have the keypair, we just have the signature and pubkey.
pub fn build_verify_signature_ix(
    signature: &[u8],
    pubkey: [u8; 32],
    message: &[u8],
) -> Instruction {
    assert_eq!(pubkey.len(), PUBKEY_SERIALIZED_SIZE);
    assert_eq!(signature.len(), SIGNATURE_SERIALIZED_SIZE);

    let mut instruction_data = Vec::with_capacity(
        DATA_START
            .saturating_add(SIGNATURE_SERIALIZED_SIZE)
            .saturating_add(PUBKEY_SERIALIZED_SIZE)
            .saturating_add(message.len()),
    );

    let num_signatures: u8 = 1;
    let public_key_offset = DATA_START;
    let signature_offset = public_key_offset.saturating_add(PUBKEY_SERIALIZED_SIZE);
    let message_data_offset = signature_offset.saturating_add(SIGNATURE_SERIALIZED_SIZE);

    // add padding byte so that offset structure is aligned
    instruction_data.extend_from_slice(bytes_of(&[num_signatures, 0]));

    let offsets = Ed25519SignatureOffsets {
        signature_offset: signature_offset as u16,
        signature_instruction_index: u16::MAX,
        public_key_offset: public_key_offset as u16,
        public_key_instruction_index: u16::MAX,
        message_data_offset: message_data_offset as u16,
        message_data_size: message.len() as u16,
        message_instruction_index: u16::MAX,
    };

    instruction_data.extend_from_slice(bytes_of(&offsets));

    debug_assert_eq!(instruction_data.len(), public_key_offset);

    instruction_data.extend_from_slice(&pubkey);

    debug_assert_eq!(instruction_data.len(), signature_offset);

    instruction_data.extend_from_slice(signature);

    debug_assert_eq!(instruction_data.len(), message_data_offset);

    instruction_data.extend_from_slice(message);

    Instruction {
        program_id: solana_program::ed25519_program::id(),
        accounts: vec![],
        data: instruction_data,
    }
}
