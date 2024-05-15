use std::str::FromStr;

use anchor_lang::InstructionData;
use anchor_lang::ToAccountMetas;
use keeper_core::Address;
use keeper_core::UpdateInstruction;
use solana_client::rpc_response::RpcVoteAccountInfo;
use solana_sdk::{instruction::Instruction, pubkey::Pubkey};

use crate::derive_validator_history_address;
use crate::derive_validator_history_config_address;

pub struct StakeHistoryEntry {
    pub stake: u64,
    pub rank: u32,
    pub is_superminority: bool,
    pub vote_account: Pubkey,
    pub address: Pubkey,
    pub config: Pubkey,
    pub signer: Pubkey,
    pub program_id: Pubkey,
    pub epoch: u64,
}

impl StakeHistoryEntry {
    pub fn new(
        vote_account: &RpcVoteAccountInfo,
        program_id: &Pubkey,
        signer: &Pubkey,
        epoch: u64,
        rank: u32,
        is_superminority: bool,
    ) -> StakeHistoryEntry {
        let vote_pubkey =
            Pubkey::from_str(&vote_account.vote_pubkey).expect("Invalid vote account pubkey");
        let address = derive_validator_history_address(&vote_pubkey, program_id);
        let config = derive_validator_history_config_address(program_id);

        StakeHistoryEntry {
            stake: vote_account.activated_stake,
            rank,
            is_superminority,
            vote_account: vote_pubkey,
            address,
            config,
            signer: *signer,
            program_id: *program_id,
            epoch,
        }
    }
}

impl Address for StakeHistoryEntry {
    fn address(&self) -> Pubkey {
        self.address
    }
}

impl UpdateInstruction for StakeHistoryEntry {
    fn update_instruction(&self) -> Instruction {
        Instruction {
            program_id: self.program_id,
            accounts: validator_history::accounts::UpdateStakeHistory {
                validator_history_account: self.address,
                vote_account: self.vote_account,
                config: self.config,
                oracle_authority: self.signer,
            }
            .to_account_metas(None),
            data: validator_history::instruction::UpdateStakeHistory {
                lamports: self.stake,
                epoch: self.epoch,
                rank: self.rank,
                is_superminority: self.is_superminority,
            }
            .data(),
        }
    }
}
