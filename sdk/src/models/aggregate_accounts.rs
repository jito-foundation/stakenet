use std::collections::HashMap;

use jito_steward::{
    stake_pool_utils::{StakePool, ValidatorList},
    Config as StewardConfig, DirectedStakeMeta, StewardStateAccountV2,
};
use solana_sdk::{account::Account, pubkey::Pubkey};

pub type Error = Box<dyn std::error::Error>;

pub struct AllStewardAccounts {
    pub config_account: Box<StewardConfig>,
    pub config_address: Pubkey,
    pub state_account: Box<StewardStateAccountV2>,
    pub state_address: Pubkey,
    pub stake_pool_account: Box<StakePool>,

    /// Jito stake pool address
    pub stake_pool_address: Pubkey,
    pub stake_pool_withdraw_authority: Pubkey,
    pub validator_list_account: Box<ValidatorList>,
    pub validator_list_address: Pubkey,
    pub reserve_stake_account: Account,

    /// Directed stake meta account
    pub directed_stake_meta_account: DirectedStakeMeta,

    /// Directed stake meta address
    pub directed_stake_meta_address: Pubkey,
}

#[derive(Default)]
pub struct AllValidatorAccounts {
    pub all_history_vote_account_map: HashMap<Pubkey, Option<Account>>,
    pub all_stake_account_map: HashMap<Pubkey, Option<Account>>,
    pub all_vote_account_map: HashMap<Pubkey, Option<Account>>,
}
