use anchor_lang::{InstructionData, ToAccountMetas};
use jito_tip_distribution::sdk::derive_tip_distribution_account_address;
use keeper_core::{Address, UpdateInstruction};
use solana_program::{instruction::Instruction, pubkey::Pubkey};

use crate::{derive_validator_history_address, derive_validator_history_config_address};

#[derive(Clone)]
pub struct ValidatorMevCommissionEntry {
    pub vote_account: Pubkey,
    pub tip_distribution_account: Pubkey,
    pub validator_history_account: Pubkey,
    pub config: Pubkey,
    pub program_id: Pubkey,
    pub signer: Pubkey,
    pub epoch: u64,
}

impl ValidatorMevCommissionEntry {
    pub fn new(
        vote_account: &Pubkey,
        epoch: u64,
        program_id: &Pubkey,
        tip_distribution_program_id: &Pubkey,
        signer: &Pubkey,
    ) -> Self {
        let validator_history_account = derive_validator_history_address(vote_account, program_id);
        let (tip_distribution_account, _) = derive_tip_distribution_account_address(
            tip_distribution_program_id,
            vote_account,
            epoch,
        );
        let config = derive_validator_history_config_address(program_id);

        Self {
            vote_account: *vote_account,
            tip_distribution_account,
            validator_history_account,
            config,
            program_id: *program_id,
            signer: *signer,
            epoch,
        }
    }
}

impl Address for ValidatorMevCommissionEntry {
    fn address(&self) -> Pubkey {
        self.validator_history_account
    }
}

impl UpdateInstruction for ValidatorMevCommissionEntry {
    fn update_instruction(&self) -> Instruction {
        Instruction {
            program_id: self.program_id,
            accounts: validator_history::accounts::CopyTipDistributionAccount {
                validator_history_account: self.validator_history_account,
                vote_account: self.vote_account,
                tip_distribution_account: self.tip_distribution_account,
                config: self.config,
                signer: self.signer,
            }
            .to_account_metas(None),
            data: validator_history::instruction::CopyTipDistributionAccount { epoch: self.epoch }
                .data(),
        }
    }
}
