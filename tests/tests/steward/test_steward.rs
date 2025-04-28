#![allow(clippy::await_holding_refcell_ref)]
/// Basic integration test
use anchor_lang::{
    solana_program::{instruction::Instruction, pubkey::Pubkey, stake, sysvar},
    InstructionData, ToAccountMetas,
};
use jito_steward::{
    instructions::AuthorityType,
    utils::{StakePool, ValidatorList},
    Config, StewardStateAccount,
};
use solana_program_test::*;
use solana_sdk::{
    clock::Clock,
    signature::Keypair,
    signer::Signer,
    stake::{
        stake_flags::StakeFlags,
        state::{Authorized, Delegation, Lockup, Meta, Stake, StakeStateV2},
    },
    transaction::Transaction,
};
use spl_stake_pool::state::StakeStatus;
use tests::steward_fixtures::{
    closed_vote_account, crank_epoch_maintenance, crank_stake_pool, manual_remove_validator,
    new_vote_account, serialized_stake_account, serialized_validator_history_account,
    serialized_validator_list_account, system_account, validator_history_default, TestFixture,
};
use validator_history::{ValidatorHistory, ValidatorHistoryEntry};

async fn _auto_add_validator_to_pool(fixture: &TestFixture, vote_account: &Pubkey) {
    let ctx = &fixture.ctx;
    let vote_account = *vote_account;
    let epoch_credits = vec![(0, 1, 0), (1, 2, 1), (2, 3, 2), (3, 4, 3), (4, 5, 4)];
    let validator_history_account = Pubkey::find_program_address(
        &[ValidatorHistory::SEED, vote_account.as_ref()],
        &validator_history::id(),
    )
    .0;
    fixture.ctx.borrow_mut().set_account(
        &vote_account,
        &new_vote_account(Pubkey::new_unique(), vote_account, 1, Some(epoch_credits)).into(),
    );
    fixture.ctx.borrow_mut().set_account(
        &validator_history_account,
        &serialized_validator_history_account(validator_history_default(vote_account, 0)).into(),
    );

    let (validator_history_account, _) = Pubkey::find_program_address(
        &[ValidatorHistory::SEED, vote_account.as_ref()],
        &validator_history::id(),
    );

    let (stake_account_address, _, withdraw_authority) =
        fixture.stake_accounts_for_validator(vote_account).await;

    let add_validator_to_pool_ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::AutoAddValidator {
            steward_state: fixture.steward_state,
            validator_history_account,
            config: fixture.steward_config.pubkey(),
            stake_pool_program: spl_stake_pool::id(),
            stake_pool: fixture.stake_pool_meta.stake_pool,
            reserve_stake: fixture.stake_pool_meta.reserve,
            withdraw_authority,
            validator_list: fixture.stake_pool_meta.validator_list,
            stake_account: stake_account_address,
            vote_account,
            rent: sysvar::rent::id(),
            clock: sysvar::clock::id(),
            stake_history: sysvar::stake_history::id(),
            stake_config: stake::config::ID,
            stake_program: stake::program::id(),
            system_program: solana_program::system_program::id(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::AutoAddValidatorToPool {}.data(),
    };
    let tx = Transaction::new_signed_with_payer(
        &[add_validator_to_pool_ix],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        ctx.borrow().last_blockhash,
    );
    fixture
        .submit_transaction_assert_error(tx.clone(), "ValidatorBelowLivenessMinimum")
        .await;

    // fixture.
    let mut validator_history = validator_history_default(vote_account, 0);
    for i in 0..20 {
        validator_history.history.push(ValidatorHistoryEntry {
            epoch: i,
            activated_stake_lamports: 100_000_000_000_000,
            epoch_credits: 400000,
            vote_account_last_update_slot: 100,
            ..ValidatorHistoryEntry::default()
        });
    }
    fixture.ctx.borrow_mut().set_account(
        &validator_history_account,
        &serialized_validator_history_account(validator_history).into(),
    );
    fixture.submit_transaction_assert_success(tx).await;

    let validator_list: ValidatorList = fixture
        .load_and_deserialize(&fixture.stake_pool_meta.validator_list)
        .await;

    let validator_stake_info_idx = validator_list
        .validators
        .iter()
        .position(|&v| v.vote_account_address == vote_account)
        .unwrap();
    assert!(
        validator_list.validators[validator_stake_info_idx].vote_account_address == vote_account
    );
}

#[tokio::test]
async fn test_auto_add_validator_to_pool() {
    let fixture = TestFixture::new().await;

    fixture.initialize_stake_pool().await;
    fixture.initialize_steward(None).await;
    fixture.realloc_steward_state().await;

    _auto_add_validator_to_pool(&fixture, &Pubkey::new_unique()).await;

    drop(fixture);
}

#[tokio::test]
async fn test_auto_remove() {
    let fixture = TestFixture::new().await;

    fixture.initialize_stake_pool().await;
    fixture.initialize_steward(None).await;
    fixture.realloc_steward_state().await;

    let vote_account = Pubkey::new_unique();

    let (validator_history_account, _) = Pubkey::find_program_address(
        &[ValidatorHistory::SEED, vote_account.as_ref()],
        &validator_history::id(),
    );

    let (stake_account_address, transient_stake_account_address, withdraw_authority) =
        fixture.stake_accounts_for_validator(vote_account).await;

    // Add vote account

    _auto_add_validator_to_pool(&fixture, &vote_account).await;

    let auto_remove_validator_ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::AutoRemoveValidator {
            validator_history_account,
            config: fixture.steward_config.pubkey(),
            state_account: fixture.steward_state,
            stake_pool_program: spl_stake_pool::id(),
            stake_pool: fixture.stake_pool_meta.stake_pool,
            reserve_stake: fixture.stake_pool_meta.reserve,
            withdraw_authority,
            validator_list: fixture.stake_pool_meta.validator_list,
            stake_account: stake_account_address,
            transient_stake_account: transient_stake_account_address,
            vote_account,
            rent: sysvar::rent::id(),
            clock: sysvar::clock::id(),
            stake_history: sysvar::stake_history::id(),
            stake_config: stake::config::ID,
            stake_program: stake::program::id(),
            system_program: solana_program::system_program::id(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::AutoRemoveValidatorFromPool {
            validator_list_index: 0,
        }
        .data(),
    };

    let tx = Transaction::new_signed_with_payer(
        &[auto_remove_validator_ix],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        fixture.ctx.borrow().last_blockhash,
    );

    fixture
        .submit_transaction_assert_error(tx.clone(), "ValidatorNotRemovable")
        .await;

    // "Close" vote account
    fixture
        .ctx
        .borrow_mut()
        .set_account(&vote_account, &closed_vote_account().into());

    fixture.submit_transaction_assert_success(tx).await;

    let steward_state_account: StewardStateAccount =
        fixture.load_and_deserialize(&fixture.steward_state).await;

    assert!(
        steward_state_account
            .state
            .validators_for_immediate_removal
            .count()
            == 1
    );

    let instant_remove_validator_ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::InstantRemoveValidator {
            config: fixture.steward_config.pubkey(),
            state_account: fixture.steward_state,
            validator_list: fixture.stake_pool_meta.validator_list,
            stake_pool: fixture.stake_pool_meta.stake_pool,
        }
        .to_account_metas(None),
        data: jito_steward::instruction::InstantRemoveValidator {
            validator_index_to_remove: 0,
        }
        .data(),
    };

    let tx = Transaction::new_signed_with_payer(
        &[instant_remove_validator_ix.clone()],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        fixture.ctx.borrow().last_blockhash,
    );

    fixture
        .submit_transaction_assert_error(tx, "ValidatorsHaveNotBeenRemoved")
        .await;

    drop(fixture);
}

async fn _auto_remove_validator_tx(
    fixture: &TestFixture,
    vote_account: Pubkey,
    validator_index_to_remove: u64,
) -> Transaction {
    let config = fixture.steward_config.pubkey();
    let state_account = fixture.steward_state;
    let stake_pool = fixture.stake_pool_meta.stake_pool;
    let reserve_stake = fixture.stake_pool_meta.reserve;
    let validator_list = fixture.stake_pool_meta.validator_list;
    let (stake_account, transient_stake_account, withdraw_authority) =
        fixture.stake_accounts_for_validator(vote_account).await;

    Transaction::new_signed_with_payer(
        &[Instruction {
            program_id: jito_steward::id(),
            accounts: jito_steward::accounts::AutoRemoveValidator {
                config,
                validator_history_account: Pubkey::find_program_address(
                    &[ValidatorHistory::SEED, vote_account.as_ref()],
                    &validator_history::id(),
                )
                .0,
                state_account,
                stake_pool,
                reserve_stake,
                withdraw_authority,
                validator_list,
                stake_account,
                transient_stake_account,
                vote_account,
                stake_history: sysvar::stake_history::id(),
                stake_config: stake::config::ID,
                stake_program: stake::program::id(),
                stake_pool_program: spl_stake_pool::id(),
                system_program: solana_program::system_program::id(),
                rent: sysvar::rent::id(),
                clock: sysvar::clock::id(),
            }
            .to_account_metas(None),
            data: jito_steward::instruction::AutoRemoveValidatorFromPool {
                validator_list_index: validator_index_to_remove,
            }
            .data(),
        }],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        fixture.ctx.borrow().last_blockhash,
    )
}

async fn _setup_auto_remove_validator_test() -> (TestFixture, Pubkey) {
    let fixture = TestFixture::new().await;
    let _ctx = &fixture.ctx;
    fixture.advance_num_epochs(1, 10).await;
    fixture.initialize_stake_pool().await;
    fixture.initialize_steward(None).await;
    fixture.realloc_steward_state().await;

    crank_stake_pool(&fixture).await;
    crank_epoch_maintenance(&fixture, None).await;

    let vote_account = Pubkey::new_unique();

    _auto_add_validator_to_pool(&fixture, &vote_account).await;

    (fixture, vote_account)
}

#[tokio::test]
async fn test_auto_remove_validator_states() {
    /*
    This test requires specific setup of stake accounts to trigger different effects in spl_stake_pool::remove_validator_from_pool
    Setting up all conditions via regular instruction calls is very difficult, so we are just testing the logic works as expected for
    the different possible stake account states.

    - conditions of the stake accounts to pass `stake_is_usable_by_pool`:
        meta.authorized.staker == *expected_authority
        && meta.authorized.withdrawer == *expected_authority
        && meta.lockup == *expected_lockup
    - conditions of the stake accounts to pass `stake_is_inactive_without_history`:
        stake.delegation.deactivation_epoch < epoch
        || (stake.delegation.activation_epoch == epoch
            && stake.delegation.deactivation_epoch == epoch)
    */

    // Status in DeactivatingValidator -> Immediate Removal
    // Condition pt 1: get_stake_state on transient_stake_account retuns Err OR transient_stake_lamports == 0 (gets to DeactivatingValidator)
    // Condition pt 2: (stake_is_usable_by_pool && stake_is_inactive_without_history) is TRUE
    let (fixture, vote_account) = _setup_auto_remove_validator_test().await;
    let ctx = &fixture.ctx;
    let (stake_account_address, _transient_stake_account_address, withdraw_authority) =
        fixture.stake_accounts_for_validator(vote_account).await;

    let stake_pool: StakePool = fixture
        .load_and_deserialize(&fixture.stake_pool_meta.stake_pool)
        .await;

    let current_epoch = ctx
        .borrow_mut()
        .banks_client
        .get_sysvar::<Clock>()
        .await
        .unwrap()
        .epoch;

    // Manually set up stake account
    let configured_stake_account = StakeStateV2::Stake(
        Meta {
            rent_exempt_reserve: 0,
            authorized: Authorized {
                staker: withdraw_authority,
                withdrawer: withdraw_authority,
            },
            lockup: stake_pool.lockup,
        },
        Stake {
            delegation: Delegation {
                voter_pubkey: vote_account,
                stake: 1_000_000_000,
                activation_epoch: 0,
                deactivation_epoch: current_epoch - 1,
                ..Default::default()
            },
            credits_observed: 0,
        },
        StakeFlags::default(),
    );

    fixture.ctx.borrow_mut().set_account(
        &stake_account_address,
        &serialized_stake_account(configured_stake_account, 1_000_000_000).into(),
    );

    fixture
        .submit_transaction_assert_success(
            _auto_remove_validator_tx(&fixture, vote_account, 0).await,
        )
        .await;

    // Get validator list and assert state
    let validator_list: ValidatorList = fixture
        .load_and_deserialize(&fixture.stake_pool_meta.validator_list)
        .await;

    let steward_state_account: StewardStateAccount =
        fixture.load_and_deserialize(&fixture.steward_state).await;

    assert!(validator_list.validators[0].status == StakeStatus::DeactivatingValidator.into());
    assert!(
        steward_state_account
            .state
            .validators_for_immediate_removal
            .count()
            == 1
    );
    assert!(steward_state_account.state.validators_to_remove.count() == 0);

    // Status in DeactivatingValidator -> Regular Removal
    // Condition pt 1: get_stake_state on transient_stake_account retuns Err OR transient_stake_lamports == 0 (gets to DeactivatingValidator)
    // Condition pt 2: (stake_is_usable_by_pool && stake_is_inactive_without_history is FALSE
    let (fixture, vote_account) = _setup_auto_remove_validator_test().await;
    let ctx = &fixture.ctx;
    let (stake_account_address, _transient_stake_account_address, withdraw_authority) =
        fixture.stake_accounts_for_validator(vote_account).await;

    let stake_pool: StakePool = fixture
        .load_and_deserialize(&fixture.stake_pool_meta.stake_pool)
        .await;

    let current_epoch = ctx
        .borrow_mut()
        .banks_client
        .get_sysvar::<Clock>()
        .await
        .unwrap()
        .epoch;

    let mismatched_lockup = Lockup {
        epoch: stake_pool.lockup.epoch + 1,
        unix_timestamp: stake_pool.lockup.unix_timestamp + 1,
        custodian: Pubkey::default(),
    };

    // Manually set up stake account
    let configured_stake_account = StakeStateV2::Stake(
        Meta {
            rent_exempt_reserve: 0,
            authorized: Authorized {
                staker: withdraw_authority,
                withdrawer: withdraw_authority,
            },
            lockup: mismatched_lockup, // Not equal to stake pool lockup
        },
        Stake {
            delegation: Delegation {
                voter_pubkey: vote_account,
                stake: 1_000_000_000,
                activation_epoch: 0,
                deactivation_epoch: current_epoch - 1,
                ..Default::default()
            },
            credits_observed: 0,
        },
        StakeFlags::default(),
    );

    fixture.ctx.borrow_mut().set_account(
        &stake_account_address,
        &serialized_stake_account(configured_stake_account, 1_000_000_000).into(),
    );

    fixture
        .submit_transaction_assert_success(
            _auto_remove_validator_tx(&fixture, vote_account, 0).await,
        )
        .await;

    // Get validator list and assert state
    let validator_list: ValidatorList = fixture
        .load_and_deserialize(&fixture.stake_pool_meta.validator_list)
        .await;
    let steward_state_account: StewardStateAccount =
        fixture.load_and_deserialize(&fixture.steward_state).await;

    assert!(validator_list.validators[0].status == StakeStatus::DeactivatingValidator.into());
    assert!(
        steward_state_account
            .state
            .validators_for_immediate_removal
            .count()
            == 0
    );
    assert!(steward_state_account.state.validators_to_remove.count() == 1);

    // Status in DeactivatingAll -> Regular Removal
    // If transient_stake_lamports > 0 and transient stake stake_is_usable_by_pool is true -> DeactivatingAll
    let (fixture, vote_account) = _setup_auto_remove_validator_test().await;
    let ctx = &fixture.ctx;
    let (stake_account_address, transient_stake_account_address, withdraw_authority) =
        fixture.stake_accounts_for_validator(vote_account).await;

    let stake_pool: StakePool = fixture
        .load_and_deserialize(&fixture.stake_pool_meta.stake_pool)
        .await;

    let current_epoch = ctx
        .borrow_mut()
        .banks_client
        .get_sysvar::<Clock>()
        .await
        .unwrap()
        .epoch;

    // Manually set up stake account
    let configured_stake_account = StakeStateV2::Stake(
        Meta {
            rent_exempt_reserve: 0,
            authorized: Authorized {
                staker: withdraw_authority,
                withdrawer: withdraw_authority,
            },
            lockup: stake_pool.lockup,
        },
        Stake {
            delegation: Delegation {
                voter_pubkey: vote_account,
                stake: 1_000_000_000,
                activation_epoch: 0,
                deactivation_epoch: current_epoch - 1,
                ..Default::default()
            },
            credits_observed: 0,
        },
        StakeFlags::default(),
    );

    // Set custom transient stake account as well as validator list transient_stake_lamports
    let validator_list: ValidatorList = fixture
        .load_and_deserialize(&fixture.stake_pool_meta.validator_list)
        .await;
    let mut spl_validator_list = validator_list.as_ref().clone();
    spl_validator_list.validators[0].transient_stake_lamports = 1_000_000_000.into();
    fixture.ctx.borrow_mut().set_account(
        &fixture.stake_pool_meta.validator_list,
        &serialized_validator_list_account(spl_validator_list, None).into(),
    );
    fixture.ctx.borrow_mut().set_account(
        &stake_account_address,
        &serialized_stake_account(configured_stake_account, 1_000_000_000).into(),
    );
    fixture.ctx.borrow_mut().set_account(
        &transient_stake_account_address,
        &serialized_stake_account(configured_stake_account, 1_000_000_000).into(),
    );

    fixture
        .submit_transaction_assert_success(
            _auto_remove_validator_tx(&fixture, vote_account, 0).await,
        )
        .await;

    // Get validator list and assert state
    let validator_list: ValidatorList = fixture
        .load_and_deserialize(&fixture.stake_pool_meta.validator_list)
        .await;
    let steward_state_account: StewardStateAccount =
        fixture.load_and_deserialize(&fixture.steward_state).await;

    assert!(validator_list.validators[0].status == StakeStatus::DeactivatingAll.into());
    assert!(
        steward_state_account
            .state
            .validators_for_immediate_removal
            .count()
            == 0
    );
    assert!(steward_state_account.state.validators_to_remove.count() == 1);

    // Remaining states not tested:
    // Status in Active -> Error (not possible to get into this state from the instruction)
    // Status in ReadyForRemoval -> Immediate Removal (not possible to get into this state from the instruction)
    // Status in DeactivatingTransient -> Regular Removal (not possible to get into this state from the instruction)
}

fn _instant_remove_validator_tx(
    fixture: &TestFixture,
    validator_index_to_remove: u64,
) -> Transaction {
    Transaction::new_signed_with_payer(
        &[Instruction {
            program_id: jito_steward::id(),
            accounts: jito_steward::accounts::InstantRemoveValidator {
                config: fixture.steward_config.pubkey(),
                state_account: fixture.steward_state,
                validator_list: fixture.stake_pool_meta.validator_list,
                stake_pool: fixture.stake_pool_meta.stake_pool,
            }
            .to_account_metas(None),
            data: jito_steward::instruction::InstantRemoveValidator {
                validator_index_to_remove,
            }
            .data(),
        }],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        fixture.ctx.borrow().last_blockhash,
    )
}

#[tokio::test]
async fn test_instant_remove_validator() {
    // Setup + auto add validator to pool
    let fixture = TestFixture::new().await;
    let _ctx = &fixture.ctx;
    fixture.initialize_stake_pool().await;
    fixture.initialize_steward(None).await;
    fixture.realloc_steward_state().await;

    let vote_account = Pubkey::new_unique();

    let (_validator_history_account, _) = Pubkey::find_program_address(
        &[ValidatorHistory::SEED, vote_account.as_ref()],
        &validator_history::id(),
    );

    let (_stake_account_address, _transient_stake_account_address, _withdraw_authority) =
        fixture.stake_accounts_for_validator(vote_account).await;

    _auto_add_validator_to_pool(&fixture, &vote_account).await;

    //// Test checks ////

    // Default state

    // Test not marked for immediate removal (ValidatorNotInList)
    let tx = _instant_remove_validator_tx(&fixture, 0);
    fixture
        .submit_transaction_assert_error(tx, "ValidatorNotInList")
        .await;

    // Manually mark for removal and Force list ValidatorStakeInfo for removal - Ready for removal
    manual_remove_validator(&fixture, 0, true, true).await;
    let validator_list: ValidatorList = fixture
        .load_and_deserialize(&fixture.stake_pool_meta.validator_list)
        .await;
    let mut spl_validator_list = validator_list.as_ref().clone();
    spl_validator_list.validators[0].status = StakeStatus::ReadyForRemoval.into();
    fixture.ctx.borrow_mut().set_account(
        &fixture.stake_pool_meta.validator_list,
        &serialized_validator_list_account(spl_validator_list, None).into(),
    );

    // Test Validators have not been removed (ValidatorsHaveNotBeenRemoved)
    let tx = _instant_remove_validator_tx(&fixture, 0);
    fixture
        .submit_transaction_assert_error(tx, "ValidatorsHaveNotBeenRemoved")
        .await;

    // Actually remove validator
    crank_stake_pool(&fixture).await;

    // Test passes and removes validator
    let tx = _instant_remove_validator_tx(&fixture, 0);
    fixture.submit_transaction_assert_success(tx).await;

    // Check that validator is removed
    let steward_state_account: StewardStateAccount =
        fixture.load_and_deserialize(&fixture.steward_state).await;
    assert!(
        steward_state_account
            .state
            .validators_for_immediate_removal
            .count()
            == 0
    );
    assert!(
        steward_state_account.state.num_pool_validators
            + steward_state_account.state.validators_added as u64
            == 0
    );

    drop(fixture);
}

#[tokio::test]
async fn test_pause() {
    let fixture = TestFixture::new().await;
    let ctx = &fixture.ctx;
    fixture.initialize_stake_pool().await;
    fixture.initialize_steward(None).await;

    let ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::PauseSteward {
            config: fixture.steward_config.pubkey(),
            authority: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::PauseSteward {}.data(),
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        ctx.borrow().last_blockhash,
    );

    fixture.submit_transaction_assert_success(tx).await;

    let config: Config = fixture
        .load_and_deserialize(&fixture.steward_config.pubkey())
        .await;

    assert!(config.is_paused());

    let ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::ResumeSteward {
            config: fixture.steward_config.pubkey(),
            authority: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::ResumeSteward {}.data(),
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        ctx.borrow().last_blockhash,
    );

    fixture.submit_transaction_assert_success(tx).await;

    let config: Config = fixture
        .load_and_deserialize(&fixture.steward_config.pubkey())
        .await;
    assert!(!config.is_paused());

    drop(fixture);
}

#[tokio::test]
async fn test_blacklist() {
    let fixture = TestFixture::new().await;
    let ctx = &fixture.ctx;
    fixture.initialize_stake_pool().await;
    fixture.initialize_steward(None).await;

    let ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::AddValidatorsToBlacklist {
            config: fixture.steward_config.pubkey(),
            authority: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::AddValidatorsToBlacklist {
            validator_history_blacklist: vec![0, 4, 8],
        }
        .data(),
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        ctx.borrow().last_blockhash,
    );

    fixture.submit_transaction_assert_success(tx).await;

    let config: Config = fixture
        .load_and_deserialize(&fixture.steward_config.pubkey())
        .await;
    assert!(config.validator_history_blacklist.get(0).unwrap());
    assert!(config.validator_history_blacklist.get(4).unwrap());
    assert!(config.validator_history_blacklist.get(8).unwrap());

    let ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::RemoveValidatorsFromBlacklist {
            config: fixture.steward_config.pubkey(),
            authority: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::RemoveValidatorsFromBlacklist {
            validator_history_blacklist: vec![4, 0],
        }
        .data(),
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        ctx.borrow().last_blockhash,
    );
    fixture.submit_transaction_assert_success(tx).await;

    let config: Config = fixture
        .load_and_deserialize(&fixture.steward_config.pubkey())
        .await;
    assert!(!config.validator_history_blacklist.get(0).unwrap());
    assert!(!config.validator_history_blacklist.get(4).unwrap());
    assert!(config.validator_history_blacklist.get(8).unwrap());
}

#[tokio::test]
async fn test_blacklist_edge_cases() {
    let fixture = TestFixture::new().await;
    let ctx = &fixture.ctx;
    fixture.initialize_stake_pool().await;
    fixture.initialize_steward(None).await;

    // Test empty blacklist should not change anything
    let ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::RemoveValidatorsFromBlacklist {
            config: fixture.steward_config.pubkey(),
            authority: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::RemoveValidatorsFromBlacklist {
            validator_history_blacklist: vec![],
        }
        .data(),
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        ctx.borrow().last_blockhash,
    );

    fixture.submit_transaction_assert_success(tx).await;

    let config: Config = fixture
        .load_and_deserialize(&fixture.steward_config.pubkey())
        .await;
    assert!(config.validator_history_blacklist.is_empty());

    // Test deactivating a validator that is not in the blacklist shouldn't break anything
    let ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::RemoveValidatorsFromBlacklist {
            config: fixture.steward_config.pubkey(),
            authority: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::RemoveValidatorsFromBlacklist {
            validator_history_blacklist: vec![1],
        }
        .data(),
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        ctx.borrow().last_blockhash,
    );
    fixture.submit_transaction_assert_success(tx).await;

    // assert nothing changed
    let config: Config = fixture
        .load_and_deserialize(&fixture.steward_config.pubkey())
        .await;
    assert!(config.validator_history_blacklist.is_empty());

    drop(fixture);
}

#[tokio::test]
async fn test_set_new_authority() {
    let fixture = TestFixture::new().await;
    let ctx = &fixture.ctx;
    fixture.initialize_stake_pool().await;
    fixture.initialize_steward(None).await;

    // Regular test
    let new_authority = Keypair::new();
    fixture
        .ctx
        .borrow_mut()
        .set_account(&new_authority.pubkey(), &system_account(1_000_000).into());

    let ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::SetNewAuthority {
            config: fixture.steward_config.pubkey(),
            new_authority: new_authority.pubkey(),
            admin: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::SetNewAuthority {
            authority_type: AuthorityType::SetAdmin,
        }
        .data(),
    };
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        ctx.borrow().last_blockhash,
    );

    fixture.submit_transaction_assert_success(tx).await;

    let config: Config = fixture
        .load_and_deserialize(&fixture.steward_config.pubkey())
        .await;
    assert!(config.admin == new_authority.pubkey());

    let ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::SetNewAuthority {
            config: fixture.steward_config.pubkey(),
            new_authority: new_authority.pubkey(),
            admin: new_authority.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::SetNewAuthority {
            authority_type: AuthorityType::SetBlacklistAuthority,
        }
        .data(),
    };
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&new_authority.pubkey()),
        &[&new_authority],
        ctx.borrow().last_blockhash,
    );

    fixture.submit_transaction_assert_success(tx).await;

    let ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::SetNewAuthority {
            config: fixture.steward_config.pubkey(),
            new_authority: new_authority.pubkey(),
            admin: new_authority.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::SetNewAuthority {
            authority_type: AuthorityType::SetParametersAuthority,
        }
        .data(),
    };
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&new_authority.pubkey()),
        &[&new_authority],
        ctx.borrow().last_blockhash,
    );

    fixture.submit_transaction_assert_success(tx).await;

    let ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::SetNewAuthority {
            config: fixture.steward_config.pubkey(),
            new_authority: new_authority.pubkey(),
            admin: new_authority.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::SetNewAuthority {
            authority_type: AuthorityType::SetPriorityFeeParameterAuthority,
        }
        .data(),
    };
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&new_authority.pubkey()),
        &[&new_authority],
        ctx.borrow().last_blockhash,
    );

    fixture.submit_transaction_assert_success(tx).await;

    let config: Config = fixture
        .load_and_deserialize(&fixture.steward_config.pubkey())
        .await;
    assert!(config.admin == new_authority.pubkey());
    assert!(config.blacklist_authority == new_authority.pubkey());
    assert!(config.parameters_authority == new_authority.pubkey());
    assert!(config.priority_fee_setting_authority == new_authority.pubkey());

    // Try to transfer back with original authority
    let ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::SetNewAuthority {
            config: fixture.steward_config.pubkey(),
            new_authority: fixture.keypair.pubkey(),
            admin: new_authority.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::SetNewAuthority {
            authority_type: AuthorityType::SetAdmin,
        }
        .data(),
    };
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&new_authority.pubkey()),
        &[&new_authority],
        ctx.borrow().last_blockhash,
    );

    fixture.submit_transaction_assert_success(tx).await;

    let config: Config = fixture
        .load_and_deserialize(&fixture.steward_config.pubkey())
        .await;
    assert!(config.admin == fixture.keypair.pubkey());
    assert!(config.blacklist_authority == new_authority.pubkey());
    assert!(config.parameters_authority == new_authority.pubkey());
    assert!(config.priority_fee_setting_authority == new_authority.pubkey());

    drop(fixture);
}
