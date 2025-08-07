use jito_priority_fee_distribution::state::PriorityFeeDistributionAccount;
use solana_sdk::{clock::Epoch, pubkey::Pubkey};

pub fn derive_priority_fee_distribution_account_address(
    priority_fee_distribution_program_id: &Pubkey,
    vote_pubkey: &Pubkey,
    epoch: Epoch,
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
