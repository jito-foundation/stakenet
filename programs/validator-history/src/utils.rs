use anchor_lang::prelude::{AccountInfo, Pubkey};

pub fn cast_epoch(epoch: u64) -> u16 {
    (epoch % u16::MAX as u64).try_into().unwrap()
}

pub fn get_vote_account(validator_history_account_info: &AccountInfo) -> Pubkey {
    let pubkey_bytes = &validator_history_account_info.data.borrow()[8..32 + 8];
    let mut data = [0; 32];
    data.copy_from_slice(pubkey_bytes);
    Pubkey::from(data)
}
