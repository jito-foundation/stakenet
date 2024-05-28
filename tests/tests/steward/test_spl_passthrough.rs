#![allow(clippy::await_holding_refcell_ref)]

use anchor_lang::{
    solana_program::{instruction::Instruction, pubkey::Pubkey, stake, sysvar},
    AccountDeserialize, AnchorDeserialize, InstructionData, ToAccountMetas,
};
use jito_steward::{
    constants::MAX_VALIDATORS,
    utils::{StakePool, ValidatorList},
    Config, Delegation, Staker, StewardStateAccount, StewardStateEnum,
};
use rand::prelude::SliceRandom;
use rand::{rngs::StdRng, SeedableRng};
use solana_program_test::*;
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction, epoch_schedule::EpochSchedule, signature::Keypair,
    signer::Signer, stake::state::StakeStateV2, system_program, transaction::Transaction,
};
use spl_stake_pool::{
    find_ephemeral_stake_program_address, instruction::PreferredValidatorType, minimum_delegation,
    state::StakeStatus,
};
use tests::steward_fixtures::{
    new_vote_account, serialized_stake_account, serialized_stake_pool_account,
    serialized_steward_state_account, TestFixture,
};

// ------------------------ HELPERS ------------------------
async fn _get_latest_blockhash(fixture: &TestFixture) -> solana_sdk::hash::Hash {
    let ctx = &fixture.ctx;

    // Borrow the context mutably and await the future in the same scope

    let latest_blockhash = {
        let mut ctx_mut = ctx.borrow_mut();
        ctx_mut.get_new_latest_blockhash().await
    };

    latest_blockhash.expect("Could not get latest blockhash")
}

async fn _simulate_stake_deposit(fixture: &TestFixture, stake_account_address: Pubkey, stake: u64) {
    let ctx = &fixture.ctx;
    let stake_program_minimum = fixture.fetch_minimum_delegation().await;
    let pool_minimum_delegation = minimum_delegation(stake_program_minimum);
    let stake_rent = fixture.fetch_stake_rent().await;
    let minimum_active_stake_with_rent = pool_minimum_delegation + stake_rent;

    let stake_account_data = fixture.get_account(&stake_account_address).await;

    let mut stake_account =
        StakeStateV2::deserialize(&mut stake_account_data.data.as_slice()).unwrap();

    let (stake_meta, mut stake_stake, stake_flags) =
        if let StakeStateV2::Stake(meta, stake, flags) = stake_account {
            (meta, stake, flags)
        } else {
            panic!("Stake account not in Stake state");
        };
    assert_eq!(stake_stake.delegation.stake, pool_minimum_delegation);
    stake_stake.delegation.stake += 2_000_000_000;
    stake_account = StakeStateV2::Stake(stake_meta, stake_stake, stake_flags);

    let stake_pool: StakePool = fixture
        .load_and_deserialize(&fixture.stake_pool_meta.stake_pool)
        .await;
    let mut stake_pool_spl = stake_pool.as_ref().clone();
    stake_pool_spl.pool_token_supply +=
        (MAX_VALIDATORS as u64 - 1) * (minimum_active_stake_with_rent + 1000) + stake;
    stake_pool_spl.total_lamports +=
        (MAX_VALIDATORS as u64 - 1) * (minimum_active_stake_with_rent + 1000) + stake;

    ctx.borrow_mut().set_account(
        &stake_account_address,
        &serialized_stake_account(stake_account, stake_account_data.lamports + stake).into(),
    );
    ctx.borrow_mut().set_account(
        &fixture.stake_pool_meta.stake_pool,
        &serialized_stake_pool_account(stake_pool_spl, std::mem::size_of::<StakePool>()).into(),
    );
}

async fn _setup_test_steward_state(
    fixture: &TestFixture,
    validators_to_add: usize,
    starting_lamport_balance: u64,
) {
    let ctx = &fixture.ctx;

    let epoch_schedule: EpochSchedule = fixture.get_sysvar().await;

    let mut steward_config: Config = fixture
        .load_and_deserialize(&fixture.steward_config.pubkey())
        .await;
    let mut steward_state_account: StewardStateAccount =
        fixture.load_and_deserialize(&fixture.steward_state).await;
    steward_config.parameters.scoring_unstake_cap_bps = 0;
    steward_config.parameters.instant_unstake_cap_bps = 0;
    steward_config.parameters.stake_deposit_unstake_cap_bps = 0;
    steward_state_account.state.state_tag = StewardStateEnum::Idle;
    steward_state_account.state.num_pool_validators = validators_to_add - 1;
    steward_state_account.state.next_cycle_epoch = epoch_schedule.first_normal_epoch + 10;
    steward_state_account.state.current_epoch = epoch_schedule.first_normal_epoch;

    let mut rng = StdRng::from_seed([42; 32]);
    let mut arr: Vec<u16> = (0..validators_to_add as u16).collect();
    arr.shuffle(&mut rng);

    // Ensure that the validator with validator_list_index MAX_VALIDATORS - 1 is the last element in the scores array
    // This guarantees we will iterate through all scores as well as all validators in the CPI, for max compute
    let last_validator_index = arr
        .iter()
        .position(|&x| x == validators_to_add as u16 - 1)
        .unwrap();

    arr.swap(last_validator_index, validators_to_add - 1);

    steward_state_account
        .state
        .sorted_score_indices
        .copy_from_slice(&arr);

    steward_state_account
        .state
        .sorted_yield_score_indices
        .copy_from_slice(&arr);

    for i in 0..validators_to_add {
        steward_state_account.state.delegations[i] = Delegation {
            numerator: 0,
            denominator: 1,
        };
        steward_state_account.state.validator_lamport_balances[i] = starting_lamport_balance;
    }
    steward_state_account.state.delegations[validators_to_add - 1] = Delegation {
        numerator: 1,
        denominator: 1,
    };

    ctx.borrow_mut().set_account(
        &fixture.steward_state,
        &serialized_steward_state_account(steward_state_account).into(),
    );
}

async fn _add_test_validator(fixture: &TestFixture, vote_account: Pubkey) {
    // Setup New Validator
    let node_pubkey = Pubkey::new_unique();
    let epoch_credits = vec![(0, 1, 0), (1, 2, 1), (2, 3, 2), (3, 4, 3), (4, 5, 4)];

    let ctx = &fixture.ctx;

    ctx.borrow_mut().set_account(
        &vote_account,
        &new_vote_account(node_pubkey, vote_account, 1, Some(epoch_credits)).into(),
    );

    let (stake_account_address, _, withdraw_authority) =
        fixture.stake_accounts_for_validator(vote_account).await;

    // Add Validator
    let instruction = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::AddValidatorToPool {
            config: fixture.steward_config.pubkey(),
            stake_pool_program: spl_stake_pool::id(),
            stake_pool: fixture.stake_pool_meta.stake_pool,
            staker: fixture.staker,
            reserve_stake: fixture.stake_pool_meta.reserve,
            withdraw_authority,
            validator_list: fixture.stake_pool_meta.validator_list,
            stake_account: stake_account_address,
            vote_account,
            rent: sysvar::rent::id(),
            clock: sysvar::clock::id(),
            stake_history: sysvar::stake_history::id(),
            stake_config: stake::config::ID,
            system_program: system_program::id(),
            stake_program: stake::program::id(),
            signer: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::AddValidatorToPool {
            validator_seed: None,
        }
        .data(),
    };

    let latest_blockhash = _get_latest_blockhash(fixture).await;

    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        latest_blockhash,
    );
    fixture.submit_transaction_assert_success(transaction).await;

    {
        // Assert the validator was added to the validator list
        let validator_list_account_raw = fixture
            .get_account(&fixture.stake_pool_meta.validator_list)
            .await;
        let validator_list_account: ValidatorList = ValidatorList::try_deserialize_unchecked(
            &mut validator_list_account_raw.data.as_slice(),
        )
        .expect("Failed to deserialize validator list account");

        let does_contain_new_validator = validator_list_account
            .validators
            .iter()
            .any(|validator| validator.vote_account_address.eq(&vote_account));

        assert!(does_contain_new_validator);
    }
}

async fn _set_and_check_preferred_validator(
    fixture: &TestFixture,
    preferred_validator_type: &PreferredValidatorType,
    validator: Option<Pubkey>,
) {
    let instruction = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::SetPreferredValidator {
            config: fixture.steward_config.pubkey(),
            stake_pool_program: spl_stake_pool::id(),
            stake_pool: fixture.stake_pool_meta.stake_pool,
            staker: fixture.staker,
            validator_list: fixture.stake_pool_meta.validator_list,
            signer: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::SetPreferredValidator {
            validator_type: preferred_validator_type.clone(),
            validator,
        }
        .data(),
    };

    let latest_blockhash = _get_latest_blockhash(fixture).await;

    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        latest_blockhash,
    );
    fixture.submit_transaction_assert_success(transaction).await;

    let stake_pool_account_raw = fixture
        .get_account(&fixture.stake_pool_meta.stake_pool)
        .await;
    let stake_pool_account: StakePool =
        StakePool::try_deserialize_unchecked(&mut stake_pool_account_raw.data.as_slice())
            .expect("Failed to deserialize stake pool account");

    match preferred_validator_type {
        PreferredValidatorType::Deposit => {
            assert_eq!(
                stake_pool_account.preferred_deposit_validator_vote_address,
                validator
            );
        }
        PreferredValidatorType::Withdraw => {
            assert_eq!(
                stake_pool_account.preferred_withdraw_validator_vote_address,
                validator
            );
        }
    }
}

async fn _increase_and_check_stake(
    fixture: &TestFixture,
    validator_list_index: usize,
    lamports_to_stake: u64,
) {
    let validator_list_account_raw = fixture
        .get_account(&fixture.stake_pool_meta.validator_list)
        .await;
    let validator_list_account: ValidatorList =
        ValidatorList::try_deserialize_unchecked(&mut validator_list_account_raw.data.as_slice())
            .expect("Failed to deserialize validator list account");

    let validator_to_increase_stake = validator_list_account
        .validators
        .get(validator_list_index)
        .expect("Validator is not in list");

    let vote_account = validator_to_increase_stake.vote_account_address;
    let (stake_account_address, transient_stake_account_address, withdraw_authority) =
        fixture.stake_accounts_for_validator(vote_account).await;

    let validator_history =
        fixture.initialize_validator_history_with_credits(vote_account, validator_list_index);

    let state_account_before: StewardStateAccount =
        fixture.load_and_deserialize(&fixture.steward_state).await;
    let lamports_before_increase = *state_account_before
        .state
        .validator_lamport_balances
        .get(validator_list_index)
        .expect("Lamport balance out of bounds");

    let instruction = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::IncreaseValidatorStake {
            config: fixture.steward_config.pubkey(),
            steward_state: fixture.steward_state,
            validator_history,
            stake_pool_program: spl_stake_pool::id(),
            stake_pool: fixture.stake_pool_meta.stake_pool,
            staker: fixture.staker,
            withdraw_authority,
            validator_list: fixture.stake_pool_meta.validator_list,
            reserve_stake: fixture.stake_pool_meta.reserve,
            transient_stake_account: transient_stake_account_address,
            stake_account: stake_account_address,
            vote_account,
            rent: sysvar::rent::id(),
            clock: sysvar::clock::id(),
            stake_history: sysvar::stake_history::id(),
            stake_config: stake::config::ID,
            system_program: system_program::id(),
            stake_program: stake::program::id(),
            signer: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::IncreaseValidatorStake {
            lamports: lamports_to_stake,
            transient_seed: validator_to_increase_stake.transient_seed_suffix.into(),
        }
        .data(),
    };

    let latest_blockhash = _get_latest_blockhash(fixture).await;

    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        latest_blockhash,
    );
    fixture.submit_transaction_assert_success(transaction).await;

    let state_account_after: StewardStateAccount =
        fixture.load_and_deserialize(&fixture.steward_state).await;
    let lamports_after_increase = *state_account_after
        .state
        .validator_lamport_balances
        .get(validator_list_index)
        .expect("Lamport balance out of bounds");

    assert_eq!(
        lamports_after_increase,
        lamports_before_increase + lamports_to_stake
    );
}

async fn _increase_and_check_additional_stake(
    fixture: &TestFixture,
    validator_list_index: usize,
    lamports_to_stake: u64,
) {
    let validator_list_account_raw = fixture
        .get_account(&fixture.stake_pool_meta.validator_list)
        .await;
    let validator_list_account: ValidatorList =
        ValidatorList::try_deserialize_unchecked(&mut validator_list_account_raw.data.as_slice())
            .expect("Failed to deserialize validator list account");

    let validator_to_increase_stake = validator_list_account
        .validators
        .get(validator_list_index)
        .expect("Validator is not in list");

    let vote_account = validator_to_increase_stake.vote_account_address;
    let (stake_account_address, transient_stake_account_address, withdraw_authority) =
        fixture.stake_accounts_for_validator(vote_account).await;

    let validator_history =
        fixture.initialize_validator_history_with_credits(vote_account, validator_list_index);

    let state_account_before: StewardStateAccount =
        fixture.load_and_deserialize(&fixture.steward_state).await;
    let lamports_before_increase = *state_account_before
        .state
        .validator_lamport_balances
        .get(validator_list_index)
        .expect("Lamport balance out of bounds");

    let (ephemeral_stake_account, _) = find_ephemeral_stake_program_address(
        &spl_stake_pool::id(),
        &fixture.stake_pool_meta.stake_pool,
        0,
    );

    let instruction = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::IncreaseAdditionalValidatorStake {
            config: fixture.steward_config.pubkey(),
            steward_state: fixture.steward_state,
            validator_history,
            stake_pool_program: spl_stake_pool::id(),
            stake_pool: fixture.stake_pool_meta.stake_pool,
            staker: fixture.staker,
            withdraw_authority,
            validator_list: fixture.stake_pool_meta.validator_list,
            reserve_stake: fixture.stake_pool_meta.reserve,
            transient_stake_account: transient_stake_account_address,
            stake_account: stake_account_address,
            vote_account,
            clock: sysvar::clock::id(),
            stake_history: sysvar::stake_history::id(),
            stake_config: stake::config::ID,
            system_program: system_program::id(),
            stake_program: stake::program::id(),
            signer: fixture.keypair.pubkey(),
            ephemeral_stake_account,
        }
        .to_account_metas(None),
        data: jito_steward::instruction::IncreaseAdditionalValidatorStake {
            lamports: lamports_to_stake,
            transient_seed: validator_to_increase_stake.transient_seed_suffix.into(),
            ephemeral_seed: 0,
        }
        .data(),
    };

    let latest_blockhash = _get_latest_blockhash(fixture).await;

    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        latest_blockhash,
    );
    fixture.submit_transaction_assert_success(transaction).await;

    let state_account_after: StewardStateAccount =
        fixture.load_and_deserialize(&fixture.steward_state).await;
    let lamports_after_increase = *state_account_after
        .state
        .validator_lamport_balances
        .get(validator_list_index)
        .expect("Lamport balance out of bounds");

    assert_eq!(
        lamports_after_increase,
        lamports_before_increase + lamports_to_stake
    );
}

pub async fn _set_staker(fixture: &TestFixture, staker: Pubkey, new_staker: Pubkey) {
    let instruction = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::SetStaker {
            config: fixture.steward_config.pubkey(),
            stake_pool_program: spl_stake_pool::id(),
            stake_pool: fixture.stake_pool_meta.stake_pool,
            staker,
            new_staker,
            signer: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::SetStaker {}.data(),
    };

    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        fixture.ctx.borrow().last_blockhash,
    );
    fixture.submit_transaction_assert_success(transaction).await;

    let stake_pool_account_raw = fixture
        .get_account(&fixture.stake_pool_meta.stake_pool)
        .await;
    let stake_pool_account: StakePool =
        StakePool::try_deserialize_unchecked(&mut stake_pool_account_raw.data.as_slice())
            .expect("Failed to deserialize stake pool account");

    assert!(stake_pool_account.staker.eq(&new_staker));
}
// ------------------------ TESTS ------------------------

#[tokio::test]
async fn test_add_validator_to_pool() {
    // Set up the test fixture
    let fixture = TestFixture::new().await;
    fixture.initialize_stake_pool().await;
    fixture.initialize_config().await;
    fixture.initialize_steward_state().await;

    {
        // Test add 1 validator
        _add_test_validator(&fixture, Pubkey::new_unique()).await;
    }

    {
        // Add 5 validators
        for _ in 0..10 {
            _add_test_validator(&fixture, Pubkey::new_unique()).await;
        }
    }

    drop(fixture);
}

#[tokio::test]
async fn test_remove_validator_from_pool() {
    // Set up the test fixture
    let fixture = TestFixture::new().await;
    fixture.initialize_stake_pool().await;
    fixture.initialize_config().await;
    fixture.initialize_steward_state().await;

    // Setup the steward state
    _setup_test_steward_state(&fixture, MAX_VALIDATORS, 1_000_000_000).await;

    // Assert the validator was added to the validator list
    _add_test_validator(&fixture, Pubkey::new_unique()).await;

    let validator_list_index = 0;
    let validator_list_account_raw = fixture
        .get_account(&fixture.stake_pool_meta.validator_list)
        .await;
    let validator_list_account: ValidatorList =
        ValidatorList::try_deserialize_unchecked(&mut validator_list_account_raw.data.as_slice())
            .expect("Failed to deserialize validator list account");

    let validator_to_remove = validator_list_account
        .validators
        .get(validator_list_index)
        .expect("Validator is not in list");

    let (stake_account_address, transient_stake_account_address, withdraw_authority) = fixture
        .stake_accounts_for_validator(validator_to_remove.vote_account_address)
        .await;

    let instruction = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::RemoveValidatorFromPool {
            config: fixture.steward_config.pubkey(),
            steward_state: fixture.steward_state,
            stake_pool_program: spl_stake_pool::id(),
            stake_pool: fixture.stake_pool_meta.stake_pool,
            staker: fixture.staker,
            withdraw_authority,
            validator_list: fixture.stake_pool_meta.validator_list,
            stake_account: stake_account_address,
            transient_stake_account: transient_stake_account_address,
            clock: sysvar::clock::id(),
            system_program: system_program::id(),
            stake_program: stake::program::id(),
            signer: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::RemoveValidatorFromPool {
            validator_list_index,
        }
        .data(),
    };

    let transaction = Transaction::new_signed_with_payer(
        &[
            ComputeBudgetInstruction::set_compute_unit_limit(1_400_000),
            instruction,
        ],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        fixture.ctx.borrow().last_blockhash,
    );
    fixture.submit_transaction_assert_success(transaction).await;

    {
        // Assert the validator was removed from the validator list
        let validator_list_account_raw = fixture
            .get_account(&fixture.stake_pool_meta.validator_list)
            .await;
        let validator_list_account: ValidatorList = ValidatorList::try_deserialize_unchecked(
            &mut validator_list_account_raw.data.as_slice(),
        )
        .expect("Failed to deserialize validator list account");

        let old_validator = validator_list_account
            .validators
            .iter()
            .find(|validator| {
                validator
                    .vote_account_address
                    .eq(&validator_to_remove.vote_account_address)
            })
            .expect("Validator is not in list");

        let stake_status =
            StakeStatus::try_from(old_validator.status).expect("Invalid stake status");

        assert!(!stake_status.eq(&StakeStatus::Active));
    }

    drop(fixture);
}

#[tokio::test]
async fn test_set_preferred_validator() {
    // Set up the test fixture
    let fixture = TestFixture::new().await;
    fixture.initialize_stake_pool().await;
    fixture.initialize_config().await;
    fixture.initialize_steward_state().await;

    // Assert the validator was added to the validator list
    _add_test_validator(&fixture, Pubkey::new_unique()).await;

    let validator_list_index = 0;
    let validator_list_account_raw = fixture
        .get_account(&fixture.stake_pool_meta.validator_list)
        .await;
    let validator_list_account: ValidatorList =
        ValidatorList::try_deserialize_unchecked(&mut validator_list_account_raw.data.as_slice())
            .expect("Failed to deserialize validator list account");

    let validator_to_set_as_preferred = validator_list_account
        .validators
        .get(validator_list_index)
        .expect("Validator is not in list");

    {
        // Set Deposit
        _set_and_check_preferred_validator(
            &fixture,
            &PreferredValidatorType::Deposit,
            Some(validator_to_set_as_preferred.vote_account_address),
        )
        .await;
    }

    {
        // Set Withdraw
        _set_and_check_preferred_validator(
            &fixture,
            &PreferredValidatorType::Withdraw,
            Some(validator_to_set_as_preferred.vote_account_address),
        )
        .await;
    }

    {
        // Remove Deposit
        _set_and_check_preferred_validator(&fixture, &PreferredValidatorType::Deposit, None).await;
    }

    {
        // Remove Withdraw
        _set_and_check_preferred_validator(&fixture, &PreferredValidatorType::Withdraw, None).await;
    }

    drop(fixture);
}

#[tokio::test]
async fn test_increase_validator_stake() {
    // Set up the test fixture
    let fixture = TestFixture::new().await;

    fixture.initialize_stake_pool().await;
    fixture.initialize_config().await;
    fixture.initialize_steward_state().await;

    // Assert the validator was added to the validator list
    _add_test_validator(&fixture, Pubkey::new_unique()).await;

    let validator_list_index = 0;
    let lamports_to_stake = 1_000_000_000;

    {
        _increase_and_check_stake(&fixture, validator_list_index, lamports_to_stake).await;
    }

    drop(fixture);
}

#[tokio::test]
async fn test_decrease_validator_stake() {
    // Set up the test fixture
    let fixture = TestFixture::new().await;

    fixture.initialize_stake_pool().await;
    fixture.initialize_config().await;
    fixture.initialize_steward_state().await;

    _add_test_validator(&fixture, Pubkey::new_unique()).await;

    _setup_test_steward_state(&fixture, MAX_VALIDATORS, 2_000_000_000).await;

    let validator_list_index = 0;
    let lamports_to_decrease = 1_000_000;

    let validator_list_account_raw = fixture
        .get_account(&fixture.stake_pool_meta.validator_list)
        .await;
    let validator_list_account: ValidatorList =
        ValidatorList::try_deserialize_unchecked(&mut validator_list_account_raw.data.as_slice())
            .expect("Failed to deserialize validator list account");

    let validator_to_increase_stake = validator_list_account
        .validators
        .get(validator_list_index)
        .expect("Validator is not in list");

    let vote_account = validator_to_increase_stake.vote_account_address;
    let (stake_account_address, transient_stake_account_address, withdraw_authority) =
        fixture.stake_accounts_for_validator(vote_account).await;

    _simulate_stake_deposit(&fixture, stake_account_address, 2_000_000_000).await;

    let validator_history =
        fixture.initialize_validator_history_with_credits(vote_account, validator_list_index);

    let instruction = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::DecreaseValidatorStake {
            config: fixture.steward_config.pubkey(),
            steward_state: fixture.steward_state,
            validator_history,
            stake_pool_program: spl_stake_pool::id(),
            stake_pool: fixture.stake_pool_meta.stake_pool,
            staker: fixture.staker,
            withdraw_authority,
            validator_list: fixture.stake_pool_meta.validator_list,
            reserve_stake: fixture.stake_pool_meta.reserve,
            transient_stake_account: transient_stake_account_address,
            stake_account: stake_account_address,
            vote_account,
            rent: sysvar::rent::id(),
            clock: sysvar::clock::id(),
            stake_history: sysvar::stake_history::id(),
            system_program: system_program::id(),
            stake_program: stake::program::id(),
            signer: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::DecreaseValidatorStake {
            lamports: lamports_to_decrease,
            transient_seed: validator_to_increase_stake.transient_seed_suffix.into(),
        }
        .data(),
    };

    let latest_blockhash = _get_latest_blockhash(&fixture).await;

    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        latest_blockhash,
    );
    fixture.submit_transaction_assert_success(transaction).await;

    drop(fixture);
}

#[tokio::test]
async fn test_increase_additional_validator_stake() {
    // Set up the test fixture
    let fixture = TestFixture::new().await;

    fixture.initialize_stake_pool().await;
    fixture.initialize_config().await;
    fixture.initialize_steward_state().await;

    // Assert the validator was added to the validator list
    _add_test_validator(&fixture, Pubkey::new_unique()).await;

    let validator_list_index = 0;
    let lamports_to_stake = 1_000_000_000;

    {
        _increase_and_check_additional_stake(&fixture, validator_list_index, lamports_to_stake)
            .await;
    }

    drop(fixture);
}

#[tokio::test]
async fn test_decrease_additional_validator_stake() {
    // Set up the test fixture
    let fixture = TestFixture::new().await;

    fixture.initialize_stake_pool().await;
    fixture.initialize_config().await;
    fixture.initialize_steward_state().await;

    _add_test_validator(&fixture, Pubkey::new_unique()).await;

    _setup_test_steward_state(&fixture, MAX_VALIDATORS, 2_000_000_000).await;

    let validator_list_index = 0;
    let lamports_to_decrease = 1_000_000;

    let validator_list_account_raw = fixture
        .get_account(&fixture.stake_pool_meta.validator_list)
        .await;
    let validator_list_account: ValidatorList =
        ValidatorList::try_deserialize_unchecked(&mut validator_list_account_raw.data.as_slice())
            .expect("Failed to deserialize validator list account");

    let validator_to_increase_stake = validator_list_account
        .validators
        .get(validator_list_index)
        .expect("Validator is not in list");

    let vote_account = validator_to_increase_stake.vote_account_address;
    let (stake_account_address, transient_stake_account_address, withdraw_authority) =
        fixture.stake_accounts_for_validator(vote_account).await;

    _simulate_stake_deposit(&fixture, stake_account_address, 2_000_000_000).await;

    let validator_history =
        fixture.initialize_validator_history_with_credits(vote_account, validator_list_index);

    let (ephemeral_stake_account, _) = find_ephemeral_stake_program_address(
        &spl_stake_pool::id(),
        &fixture.stake_pool_meta.stake_pool,
        0,
    );

    let instruction = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::DecreaseAdditionalValidatorStake {
            config: fixture.steward_config.pubkey(),
            steward_state: fixture.steward_state,
            validator_history,
            stake_pool_program: spl_stake_pool::id(),
            stake_pool: fixture.stake_pool_meta.stake_pool,
            staker: fixture.staker,
            withdraw_authority,
            validator_list: fixture.stake_pool_meta.validator_list,
            reserve_stake: fixture.stake_pool_meta.reserve,
            transient_stake_account: transient_stake_account_address,
            stake_account: stake_account_address,
            vote_account,
            clock: sysvar::clock::id(),
            stake_history: sysvar::stake_history::id(),
            system_program: system_program::id(),
            stake_program: stake::program::id(),
            signer: fixture.keypair.pubkey(),
            ephemeral_stake_account,
        }
        .to_account_metas(None),
        data: jito_steward::instruction::DecreaseAdditionalValidatorStake {
            lamports: lamports_to_decrease,
            transient_seed: validator_to_increase_stake.transient_seed_suffix.into(),
            ephemeral_seed: 0,
        }
        .data(),
    };

    let latest_blockhash = _get_latest_blockhash(&fixture).await;

    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        latest_blockhash,
    );
    fixture.submit_transaction_assert_success(transaction).await;

    drop(fixture);
}

#[tokio::test]
async fn test_set_staker() {
    // Set up the test fixture
    let fixture = TestFixture::new().await;
    fixture.initialize_stake_pool().await;
    fixture.initialize_config().await;
    fixture.initialize_steward_state().await;

    let new_staker = Keypair::new();

    {
        // Assert all accounts are correct
        let config_account: Config = fixture
            .load_and_deserialize(&fixture.steward_config.pubkey())
            .await;

        let stake_pool_account_raw = fixture.get_account(&config_account.stake_pool).await;
        let stake_pool_account: StakePool =
            StakePool::try_deserialize_unchecked(&mut stake_pool_account_raw.data.as_slice())
                .expect("Failed to deserialize stake pool account");

        let (staker, _) = Pubkey::find_program_address(
            &[Staker::SEED, fixture.steward_config.pubkey().as_ref()],
            &jito_steward::id(),
        );

        // Assert accounts are set up correctly
        assert!(stake_pool_account.staker.eq(&staker));
        assert!(fixture.staker.eq(&staker));
        assert!(config_account.authority.eq(&fixture.keypair.pubkey()));
        assert!(config_account
            .stake_pool
            .eq(&fixture.stake_pool_meta.stake_pool));
    }

    {
        // Test 1: Set staker to same staker
        _set_staker(&fixture, fixture.staker, fixture.staker).await;
    }

    {
        // Test 2: Set staker to different staker
        _set_staker(&fixture, fixture.staker, new_staker.pubkey()).await;
    }

    drop(fixture);
}
