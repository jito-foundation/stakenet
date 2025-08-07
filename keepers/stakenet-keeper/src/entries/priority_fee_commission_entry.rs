use anchor_lang::{InstructionData, ToAccountMetas};
use jito_priority_fee_distribution::state::PriorityFeeDistributionAccount;
use solana_program::{instruction::Instruction, pubkey::Pubkey};
use stakenet_sdk::{
    models::entries::{Address, UpdateInstruction},
    utils::accounts::{get_validator_history_address, get_validator_history_config_address},
};

pub fn derive_priority_fee_distribution_account_address(
    priority_fee_distribution_program_id: &Pubkey,
    vote_pubkey: &Pubkey,
    epoch: u64,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[
            PriorityFeeDistributionAccount::SEED,
            vote_pubkey.to_bytes().as_ref(),
            epoch.to_le_bytes().as_ref(),
        ],
        priority_fee_distribution_program_id,
    )
}

#[derive(Clone)]
pub struct ValidatorPriorityFeeCommissionEntry {
    pub vote_account: Pubkey,
    pub priority_fee_distribution_account: Pubkey,
    pub validator_history_account: Pubkey,
    pub config: Pubkey,
    pub program_id: Pubkey,
    pub signer: Pubkey,
    pub epoch: u64,
}

impl ValidatorPriorityFeeCommissionEntry {
    pub fn new(
        vote_account: &Pubkey,
        epoch: u64,
        program_id: &Pubkey,
        priority_fee_distribution_program_id: &Pubkey,
        signer: &Pubkey,
    ) -> Self {
        let validator_history_account = get_validator_history_address(vote_account, program_id);
        let (priority_fee_distribution_account, _) =
            derive_priority_fee_distribution_account_address(
                priority_fee_distribution_program_id,
                vote_account,
                epoch,
            );
        let config = get_validator_history_config_address(program_id);

        Self {
            vote_account: *vote_account,
            priority_fee_distribution_account,
            validator_history_account,
            config,
            program_id: *program_id,
            signer: *signer,
            epoch,
        }
    }
}

impl Address for ValidatorPriorityFeeCommissionEntry {
    fn address(&self) -> Pubkey {
        self.validator_history_account
    }
}

impl UpdateInstruction for ValidatorPriorityFeeCommissionEntry {
    fn update_instruction(&self) -> Instruction {
        Instruction {
            program_id: self.program_id,
            accounts: validator_history::accounts::CopyPriorityFeeDistribution {
                validator_history_account: self.validator_history_account,
                vote_account: self.vote_account,
                config: self.config,
                signer: self.signer,
                distribution_account: self.priority_fee_distribution_account,
            }
            .to_account_metas(None),
            data: validator_history::instruction::CopyPriorityFeeDistribution { epoch: self.epoch }
                .data(),
        }
    }
}
