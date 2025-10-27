use borsh1::BorshSerialize;
use jito_steward::utils::get_transient_stake_seed_at_index;
use solana_sdk::pubkey::Pubkey;
use spl_pod::primitives::{PodU32, PodU64};
use spl_stake_pool::state::{PodStakeStatus, StakeStatus, ValidatorStakeInfo};
#[test]
fn test_extract_transient_seed_from_validator_list() {
    // Create a validator list with hardcoded transient seeds
    let mut spl_validator_list = spl_stake_pool::state::ValidatorList::new(3);

    // Create validators with specific transient seeds
    let validators = vec![
        ValidatorStakeInfo {
            vote_account_address: Pubkey::new_unique(),
            active_stake_lamports: PodU64::from(1_000_000_000),
            transient_seed_suffix: PodU64::from(123),
            status: PodStakeStatus::from(StakeStatus::Active),
            transient_stake_lamports: PodU64::from(1_000_000_000),
            last_update_epoch: PodU64::from(1),
            unused: PodU32::from(0),
            validator_seed_suffix: PodU32::from(0),
        },
        ValidatorStakeInfo {
            vote_account_address: Pubkey::new_unique(),
            active_stake_lamports: PodU64::from(2_000_000_000),
            transient_seed_suffix: PodU64::from(456),
            status: PodStakeStatus::from(StakeStatus::Active),
            transient_stake_lamports: PodU64::from(2_000_000_000),
            last_update_epoch: PodU64::from(1),
            unused: PodU32::from(0),
            validator_seed_suffix: PodU32::from(0),
        },
        ValidatorStakeInfo {
            vote_account_address: Pubkey::new_unique(),
            active_stake_lamports: PodU64::from(3_000_000_000),
            transient_seed_suffix: PodU64::from(789),
            status: PodStakeStatus::from(StakeStatus::Active),
            transient_stake_lamports: PodU64::from(3_000_000_000),
            last_update_epoch: PodU64::from(1),
            unused: PodU32::from(0),
            validator_seed_suffix: PodU32::from(0),
        },
    ];

    // Set the validators in the list
    for (i, validator) in validators.iter().enumerate() {
        spl_validator_list.validators[i] = *validator;
    }

    let mut buffer = Vec::new();
    spl_validator_list.serialize(&mut buffer).unwrap();

    let mut lamports: u64 = 0;

    // Serialize ValidatorList into a byte array with borsh and assign to AccountInfo data, pass
    // it to the utility function
    let validator_list_account_info = solana_sdk::account_info::AccountInfo {
        key: &Pubkey::new_unique(),
        is_signer: false,
        is_writable: false,
        lamports: std::rc::Rc::new(std::cell::RefCell::new(&mut lamports)),
        data: std::rc::Rc::new(std::cell::RefCell::new(&mut buffer)),
        owner: &Pubkey::new_unique(),
        executable: false,
        rent_epoch: 0,
    };

    #[allow(clippy::needless_range_loop)]
    for i in 0..3 {
        let extracted_seed =
            get_transient_stake_seed_at_index(&validator_list_account_info, i).unwrap();
        let expected_seed = validators[i].transient_seed_suffix;
        assert_eq!(PodU64::from(extracted_seed), expected_seed);
    }
}
