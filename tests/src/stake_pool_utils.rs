use borsh1::BorshSerialize;
use solana_sdk::account::Account;
use spl_stake_pool::state::ValidatorList as SPLValidatorList;

// TODO write a function to serialize any account with T: AnchorSerialize
pub fn serialized_validator_list_account(
    validator_list: SPLValidatorList,
    account_size: Option<usize>,
) -> Account {
    // Passes in size because zeros at the end will be truncated during serialization
    let mut data = vec![];
    validator_list.serialize(&mut data).unwrap();
    let account_size = account_size.unwrap_or(5 + 4 + 73 * validator_list.validators.len());
    data.extend(vec![0; account_size - data.len()]);
    Account {
        lamports: 1_000_000_000,
        data,
        owner: spl_stake_pool::id(),
        ..Account::default()
    }
}

pub fn serialized_stake_pool_account(
    stake_pool: spl_stake_pool::state::StakePool,
    account_size: usize,
) -> Account {
    let mut data = vec![];
    stake_pool.serialize(&mut data).unwrap();
    data.extend(vec![0; account_size - data.len()]);
    Account {
        lamports: 10_000_000_000,
        data,
        owner: spl_stake_pool::id(),
        ..Account::default()
    }
}
