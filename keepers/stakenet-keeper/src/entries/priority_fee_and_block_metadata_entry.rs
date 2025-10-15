use anchor_lang::{InstructionData, ToAccountMetas};
use solana_program::{instruction::Instruction, pubkey::Pubkey};
use stakenet_sdk::{
    models::entries::{Address, UpdateInstruction},
    utils::accounts::{get_validator_history_address, get_validator_history_config_address},
};

#[derive(Clone)]
pub struct PriorityFeeAndBlockMetadataEntry {
    pub validator_history_account: Pubkey,
    pub vote_account: Pubkey,
    pub config: Pubkey,
    pub priority_fee_oracle_authority: Pubkey,
    pub program_id: Pubkey,
    pub epoch: u64,
    pub total_priority_fees: u64,
    pub total_leader_slots: u32,
    pub blocks_produced: u32,
    pub highest_global_done_slot: u64, // Highest Done slot globally
    pub highest_done_slot: u64,        // Highest Done slot within an epoch
    // Aux Data
    pub blocks_left: u32,
    pub blocks_error: u32,
    pub blocks_missed: u32,
}

impl PriorityFeeAndBlockMetadataEntry {
    pub fn new(
        vote_account: &Pubkey,
        epoch: u64,
        program_id: &Pubkey,
        priority_fee_oracle_authority: &Pubkey,
    ) -> Self {
        let validator_history_account = get_validator_history_address(vote_account, program_id);
        let config = get_validator_history_config_address(program_id);

        Self {
            validator_history_account,
            vote_account: *vote_account,
            config,
            program_id: *program_id,
            priority_fee_oracle_authority: *priority_fee_oracle_authority,
            epoch,
            total_priority_fees: 0,
            total_leader_slots: 0,
            blocks_produced: 0,
            highest_global_done_slot: 0,
            blocks_left: 0,
            blocks_error: 0,
            blocks_missed: 0,
            highest_done_slot: 0,
        }
    }
}

impl Address for PriorityFeeAndBlockMetadataEntry {
    fn address(&self) -> Pubkey {
        self.validator_history_account
    }
}

impl UpdateInstruction for PriorityFeeAndBlockMetadataEntry {
    fn update_instruction(&self) -> Instruction {
        Instruction {
            program_id: self.program_id,
            accounts: validator_history::accounts::UpdatePriorityFeeHistory {
                validator_history_account: self.validator_history_account,
                vote_account: self.vote_account,
                config: self.config,
                priority_fee_oracle_authority: self.priority_fee_oracle_authority,
            }
            .to_account_metas(None),
            data: validator_history::instruction::UpdatePriorityFeeHistory {
                epoch: self.epoch,
                total_priority_fees: self.total_priority_fees,
                total_leader_slots: self.total_leader_slots,
                blocks_produced: self.blocks_produced,
                highest_oracle_recorded_slot: self.highest_global_done_slot,
            }
            .data(),
        }
    }
}
