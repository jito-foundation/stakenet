use anchor_lang::{InstructionData, ToAccountMetas};
use keeper_core::{Address, UpdateInstruction};
use solana_sdk::instruction::Instruction;
use solana_sdk::pubkey::Pubkey;
use validator_history::Config;
use validator_history::ValidatorHistory;

pub struct CopyVoteAccountEntry {
    pub vote_account: Pubkey,
    pub validator_history_account: Pubkey,
    pub config_address: Pubkey,
    pub program_id: Pubkey,
    pub signer: Pubkey,
}

impl CopyVoteAccountEntry {
    pub fn new(vote_account: &Pubkey, program_id: &Pubkey, signer: &Pubkey) -> Self {
        let (validator_history_account, _) = Pubkey::find_program_address(
            &[ValidatorHistory::SEED, &vote_account.to_bytes()],
            program_id,
        );
        let (config_address, _) = Pubkey::find_program_address(&[Config::SEED], program_id);
        Self {
            vote_account: *vote_account,
            validator_history_account,
            config_address,
            program_id: *program_id,
            signer: *signer,
        }
    }
}

impl Address for CopyVoteAccountEntry {
    fn address(&self) -> Pubkey {
        self.validator_history_account
    }
}

impl UpdateInstruction for CopyVoteAccountEntry {
    fn update_instruction(&self) -> Instruction {
        Instruction {
            program_id: self.program_id,
            accounts: validator_history::accounts::CopyVoteAccount {
                validator_history_account: self.validator_history_account,
                vote_account: self.vote_account,
                signer: self.signer,
            }
            .to_account_metas(None),
            data: validator_history::instruction::CopyVoteAccount {}.data(),
        }
    }
}
