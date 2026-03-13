use anchor_lang::{InstructionData, ToAccountMetas};
use solana_sdk::{instruction::Instruction, pubkey::Pubkey};
use stakenet_sdk::{
    models::entries::{Address, UpdateInstruction},
    utils::accounts::{get_validator_history_address, get_validator_history_config_address},
};

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
