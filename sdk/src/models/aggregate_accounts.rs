use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;

pub type Error = Box<dyn std::error::Error>;
use jito_steward::{
    utils::{StakePool, ValidatorList},
    Config as StewardConfig, StewardStateAccount,
};
use solana_sdk::account::Account;
pub struct AllStewardAccounts {
    pub config_account: Box<StewardConfig>,
    pub config_address: Pubkey,
    pub state_account: Box<StewardStateAccount>,
    pub state_address: Pubkey,
    pub stake_pool_account: Box<StakePool>,
    pub stake_pool_address: Pubkey,
    pub stake_pool_withdraw_authority: Pubkey,
    pub validator_list_account: Box<ValidatorList>,
    pub validator_list_address: Pubkey,
    pub reserve_stake_address: Pubkey,
    pub reserve_stake_account: Account,
}

#[derive(Default)]
pub struct AllValidatorAccounts {
    pub all_history_vote_account_map: HashMap<Pubkey, Option<Account>>,
    pub all_stake_account_map: HashMap<Pubkey, Option<Account>>,
    pub all_vote_account_map: HashMap<Pubkey, Option<Account>>,
}
