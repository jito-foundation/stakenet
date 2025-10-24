#![allow(clippy::await_holding_refcell_ref)]
use std::collections::HashMap;

use anchor_lang::{
    solana_program::{instruction::Instruction, pubkey::Pubkey},
    InstructionData, ToAccountMetas,
};
use jito_steward::{
    stake_pool_utils::ValidatorList, StewardStateAccount, UpdateParametersArgs, EPOCH_MAINTENANCE,
};
use solana_program_test::*;
use solana_sdk::{clock::Clock, signature::Keypair, signer::Signer, transaction::Transaction};
use spl_stake_pool::state::{StakeStatus, ValidatorList as SPLValidatorList};
use tests::{
    stake_pool_utils::serialized_validator_list_account,
    steward_fixtures::{
        auto_add_validator, crank_epoch_maintenance, crank_stake_pool, manual_remove_validator,
        ExtraValidatorAccounts, FixtureDefaultAccounts, StateMachineFixtures, TestFixture,
        ValidatorEntry,
    },
};
use validator_history::ValidatorHistory;

async fn _epoch_maintenance_tx(
    fixture: &TestFixture,
    validator_index_to_remove: Option<u64>,
) -> Transaction {
    let ctx = &fixture.ctx;
    let ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::EpochMaintenance {
            config: fixture.steward_config.pubkey(),
            state_account: fixture.steward_state,
            validator_list: fixture.stake_pool_meta.validator_list,
            stake_pool: fixture.stake_pool_meta.stake_pool,
        }
        .to_account_metas(None),
        data: jito_steward::instruction::EpochMaintenance {
            validator_index_to_remove,
        }
        .data(),
    };
    let blockhash = ctx.borrow_mut().get_new_latest_blockhash().await.unwrap();
    Transaction::new_signed_with_payer(
        &[ix],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        blockhash,
    )
}

async fn _epoch_maintenance_setup() -> (
    Box<TestFixture>,
    Box<StateMachineFixtures>,
    Vec<ExtraValidatorAccounts>,
) {
    // Setup pool and steward
    let mut fixture_accounts = FixtureDefaultAccounts::default();

    let unit_test_fixtures = Box::<StateMachineFixtures>::default();

    // Note that these parameters are overriden in initialize_steward, just included here for completeness
    fixture_accounts.steward_config.parameters = unit_test_fixtures.config.parameters;

    fixture_accounts.validators = (0..3)
        .map(|i| ValidatorEntry {
            validator_history: unit_test_fixtures.validators[i],
            vote_account: unit_test_fixtures.vote_accounts[i].clone(),
            vote_address: unit_test_fixtures.validators[i].vote_account,
        })
        .collect();
    fixture_accounts.cluster_history = unit_test_fixtures.cluster_history;

    // Modify validator history account with desired values

    let mut fixture = TestFixture::new_from_accounts(fixture_accounts, HashMap::new()).await;

    fixture.steward_config = Keypair::new();
    fixture.steward_state = Pubkey::find_program_address(
        &[
            StewardStateAccount::SEED,
            fixture.steward_config.pubkey().as_ref(),
        ],
        &jito_steward::id(),
    )
    .0;

    fixture.advance_num_epochs(20, 10).await;
    fixture.initialize_stake_pool().await;
    fixture
        .initialize_steward(
            Some(UpdateParametersArgs {
                mev_commission_range: Some(10), // Set to pass validation, where epochs starts at 0
                epoch_credits_range: Some(20),  // Set to pass validation, where epochs starts at 0
                commission_range: Some(20),     // Set to pass validation, where epochs starts at 0
                scoring_delinquency_threshold_ratio: Some(0.85),
                instant_unstake_delinquency_threshold_ratio: Some(0.70),
                mev_commission_bps_threshold: Some(1000),
                commission_threshold: Some(5),
                historical_commission_threshold: Some(50),
                num_delegation_validators: Some(200),
                scoring_unstake_cap_bps: Some(750),
                instant_unstake_cap_bps: Some(10),
                stake_deposit_unstake_cap_bps: Some(10),
                instant_unstake_epoch_progress: Some(0.00),
                compute_score_slot_range: Some(1000),
                instant_unstake_inputs_epoch_progress: Some(0.50),
                num_epochs_between_scoring: Some(2), // 2 epoch cycle
                minimum_stake_lamports: Some(5_000_000_000),
                minimum_voting_epochs: Some(0), // Set to pass validation, where epochs starts at 0
                compute_score_epoch_progress: Some(0.50),
            }),
            None,
        )
        .await;
    fixture.realloc_steward_state().await;

    let mut extra_validator_accounts = vec![];
    for i in 0..unit_test_fixtures.validators.len() {
        let vote_account = unit_test_fixtures.validator_list[i].vote_account_address;
        let (validator_history_address, _) = Pubkey::find_program_address(
            &[ValidatorHistory::SEED, vote_account.as_ref()],
            &validator_history::id(),
        );

        let (stake_account_address, transient_stake_account_address, withdraw_authority) =
            fixture.stake_accounts_for_validator(vote_account).await;

        extra_validator_accounts.push(ExtraValidatorAccounts {
            vote_account,
            validator_history_address,
            stake_account_address,
            transient_stake_account_address,
            withdraw_authority,
        })
    }

    crank_epoch_maintenance(&fixture, None).await;
    // Auto add validator - adds to validator list
    for extra_accounts in extra_validator_accounts
        .iter()
        .take(unit_test_fixtures.validators.len())
    {
        auto_add_validator(&fixture, extra_accounts).await;
    }

    fixture.advance_num_epochs(1, 10).await;

    crank_stake_pool(&fixture).await;

    (
        Box::new(fixture),
        unit_test_fixtures,
        extra_validator_accounts,
    )
}

#[tokio::test]
async fn test_epoch_maintenance_fails_status_check() {
    // Setup pool and steward
    let (fixture, _unit_test_fixtures, _extra_validator_accounts) =
        _epoch_maintenance_setup().await;

    let validator_list: ValidatorList = fixture
        .load_and_deserialize(&fixture.stake_pool_meta.validator_list)
        .await;
    let mut spl_validator_list: SPLValidatorList = validator_list.as_ref().clone();

    // Force validator list into deactivating state (overriding account)
    spl_validator_list.validators[0].status = StakeStatus::ReadyForRemoval.into();
    fixture.ctx.borrow_mut().set_account(
        &fixture.stake_pool_meta.validator_list,
        &serialized_validator_list_account(spl_validator_list, None).into(),
    );

    // Tests fails state invariant checks

    let tx = _epoch_maintenance_tx(&fixture, Some(0)).await;
    fixture
        .submit_transaction_assert_error(tx, "ValidatorsHaveNotBeenRemoved")
        .await;
}

#[tokio::test]
async fn test_epoch_maintenance_fails_invariant_check() {
    // Setup pool and steward
    let (fixture, _unit_test_fixtures, _extra_validator_accounts) =
        _epoch_maintenance_setup().await;

    // Mark validator to remove without actually removing it from list
    manual_remove_validator(&fixture, 0, true, false).await;

    // Try to remove validator 0 but it's not removed from spl ValidatorList
    let tx = _epoch_maintenance_tx(&fixture, Some(0)).await;
    fixture
        .submit_transaction_assert_error(tx, "ListStateMismatch")
        .await;
}

#[tokio::test]
async fn test_epoch_maintenance_removes_validators() {
    // Setup pool and steward
    let (fixture, _unit_test_fixtures, _extra_validator_accounts) =
        _epoch_maintenance_setup().await;

    // Mark validator to remove with admin fn (delayed removal)
    manual_remove_validator(&fixture, 0, true, false).await;
    let validator_list: ValidatorList = fixture
        .load_and_deserialize(&fixture.stake_pool_meta.validator_list)
        .await;
    // Override validator list to actually remove validator
    let mut spl_validator_list: SPLValidatorList = validator_list.as_ref().clone();
    spl_validator_list.validators.remove(0);
    fixture.ctx.borrow_mut().set_account(
        &fixture.stake_pool_meta.validator_list,
        &serialized_validator_list_account(spl_validator_list, None).into(),
    );

    // Test removes validator_to_remove
    crank_epoch_maintenance(&fixture, Some(&[0])).await;

    // Checks new validators_added and epoch maintenance state updated
    let clock: Clock = fixture
        .ctx
        .borrow_mut()
        .banks_client
        .get_sysvar()
        .await
        .unwrap();
    let state_account: Box<StewardStateAccount> =
        Box::new(fixture.load_and_deserialize(&fixture.steward_state).await);
    let state = &state_account.state;
    assert_eq!(state.validators_added, 2);
    assert_eq!(state.current_epoch, clock.epoch);
    assert!(state.validators_to_remove.is_empty());
    assert!(state.validators_for_immediate_removal.is_empty());
    assert!(state.has_flag(EPOCH_MAINTENANCE));
}
