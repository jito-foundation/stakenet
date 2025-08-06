#![allow(clippy::await_holding_refcell_ref)]
use std::collections::HashMap;

#[allow(deprecated)]
use anchor_lang::{
    solana_program::{instruction::Instruction, pubkey::Pubkey, stake},
    InstructionData, ToAccountMetas,
};
use jito_steward::{stake_pool_utils::ValidatorList, StewardStateAccount, UpdateParametersArgs};
use solana_program_test::*;
#[allow(deprecated)]
use solana_sdk::{
    clock::Clock, epoch_schedule::EpochSchedule, signature::Keypair, signer::Signer,
    system_program, transaction::Transaction,
};
use tests::steward_fixtures::{
    auto_add_validator, crank_compute_delegations, crank_compute_instant_unstake,
    crank_compute_score, crank_epoch_maintenance, crank_idle, crank_rebalance, crank_stake_pool,
    crank_validator_history_accounts, instant_remove_validator, ExtraValidatorAccounts,
    FixtureDefaultAccounts, StateMachineFixtures, TestFixture, ValidatorEntry,
};
use validator_history::ValidatorHistory;

#[tokio::test]
async fn test_cycle() {
    let mut fixture_accounts = FixtureDefaultAccounts::default();

    let unit_test_fixtures = StateMachineFixtures::default();

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
    let ctx = &fixture.ctx;

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
            }),
            None,
        )
        .await;
    fixture.realloc_steward_state().await;

    let _steward: StewardStateAccount = fixture.load_and_deserialize(&fixture.steward_state).await;

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
    for extra_accounts in extra_validator_accounts.iter() {
        auto_add_validator(&fixture, extra_accounts).await;
    }

    crank_compute_score(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1, 2],
    )
    .await;

    crank_compute_delegations(&fixture).await;

    let epoch_schedule: EpochSchedule = ctx.borrow_mut().banks_client.get_sysvar().await.unwrap();
    let clock: Clock = ctx.borrow_mut().banks_client.get_sysvar().await.unwrap();

    crank_idle(&fixture).await;

    crank_compute_instant_unstake(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1, 2],
    )
    .await;

    crank_rebalance(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1, 2],
    )
    .await;

    fixture.advance_num_epochs(1, 10).await;

    crank_stake_pool(&fixture).await;

    crank_epoch_maintenance(&fixture, None).await;

    crank_idle(&fixture).await;

    // Advance to instant_unstake_inputs_epoch_progress
    fixture
        .advance_num_epochs(0, epoch_schedule.get_slots_in_epoch(clock.epoch) / 2 + 1)
        .await;

    // Update validator history values
    crank_validator_history_accounts(&fixture, &extra_validator_accounts, &[0, 1, 2]).await;

    crank_compute_instant_unstake(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1, 2],
    )
    .await;

    crank_rebalance(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1, 2],
    )
    .await;

    fixture.advance_num_epochs(1, 10).await;

    crank_stake_pool(&fixture).await;

    crank_epoch_maintenance(&fixture, None).await;

    // Update validator history values
    crank_validator_history_accounts(&fixture, &extra_validator_accounts, &[0, 1, 2]).await;

    // In new cycle
    crank_compute_score(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1, 2],
    )
    .await;

    let clock: Clock = ctx.borrow_mut().banks_client.get_sysvar().await.unwrap();
    let state_account: StewardStateAccount =
        fixture.load_and_deserialize(&fixture.steward_state).await;
    let state = state_account.state;

    assert!(matches!(
        state.state_tag,
        jito_steward::StewardStateEnum::ComputeDelegations
    ));
    assert_eq!(state.current_epoch, clock.epoch);
    assert_eq!(state.next_cycle_epoch, clock.epoch + 2);
    assert_eq!(state.instant_unstake_total, 0);
    assert_eq!(state.scoring_unstake_total, 0);
    assert_eq!(state.stake_deposit_unstake_total, 0);
    assert_eq!(state.validators_added, 0);
    assert!(state.validators_to_remove.is_empty());
    // assert_eq!(state.status_flags, 3); // TODO

    // All other values are reset

    drop(fixture);
}

#[tokio::test]
async fn test_remove_validator_mid_epoch() {
    /*
      Tests that a validator removed at an arbitrary point in the cycle is not included in the current cycle's consideration,
      even though it is still in the validator list, and the next epoch, it is removed from the validator list.
    */

    let mut fixture_accounts = FixtureDefaultAccounts::default();

    let unit_test_fixtures = StateMachineFixtures::default();

    fixture_accounts.steward_config.parameters = unit_test_fixtures.config.parameters;

    fixture_accounts.validators = (0..3)
        .map(|i| ValidatorEntry {
            validator_history: unit_test_fixtures.validators[i],
            vote_account: unit_test_fixtures.vote_accounts[i].clone(),
            vote_address: unit_test_fixtures.validators[i].vote_account,
        })
        .collect();
    fixture_accounts.cluster_history = unit_test_fixtures.cluster_history;

    let mut fixture = TestFixture::new_from_accounts(fixture_accounts, HashMap::new()).await;
    let ctx = &fixture.ctx;

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
            }),
            None,
        )
        .await;
    fixture.realloc_steward_state().await;

    let mut extra_validator_accounts = vec![];
    for vote_account in unit_test_fixtures
        .validator_list
        .iter()
        .take(unit_test_fixtures.validators.len())
        .map(|v| v.vote_account_address)
    {
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
    // Auto add validator - adds validators 2 and 3
    for extra_accounts in extra_validator_accounts.iter().take(3) {
        auto_add_validator(&fixture, extra_accounts).await;
    }

    crank_compute_score(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1, 2],
    )
    .await;

    crank_compute_delegations(&fixture).await;

    crank_idle(&fixture).await;

    crank_compute_instant_unstake(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1],
    )
    .await;

    // Remove validator 2 in the middle of compute instant unstake
    let remove_validator_from_pool_ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::RemoveValidatorFromPool {
            config: fixture.steward_config.pubkey(),
            state_account: fixture.steward_state,
            stake_pool_program: spl_stake_pool::id(),
            stake_pool: fixture.stake_pool_meta.stake_pool,
            withdraw_authority: extra_validator_accounts[2].withdraw_authority,
            validator_list: fixture.stake_pool_meta.validator_list,
            stake_account: extra_validator_accounts[2].stake_account_address,
            transient_stake_account: extra_validator_accounts[2].transient_stake_account_address,
            clock: solana_sdk::sysvar::clock::id(),
            system_program: system_program::id(),
            stake_program: stake::program::id(),
            admin: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::RemoveValidatorFromPool {
            validator_list_index: 2,
        }
        .data(),
    };
    let blockhash = ctx.borrow_mut().get_new_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[remove_validator_from_pool_ix],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        blockhash,
    );
    fixture.submit_transaction_assert_success(tx).await;

    let state_account: StewardStateAccount =
        fixture.load_and_deserialize(&fixture.steward_state).await;
    let state = state_account.state;
    assert!(matches!(
        state.state_tag,
        jito_steward::StewardStateEnum::ComputeInstantUnstake
    ));
    assert_eq!(state.validators_for_immediate_removal.count(), 1);
    assert!(state.validators_for_immediate_removal.get(2).unwrap());
    assert_eq!(state.num_pool_validators, 3);

    let validator_list: ValidatorList = fixture
        .load_and_deserialize(&fixture.stake_pool_meta.validator_list)
        .await;
    assert!(validator_list
        .validators
        .iter()
        .any(|v| v.vote_account_address == extra_validator_accounts[2].vote_account));
    assert!(validator_list.validators.len() == 3);
    println!("Stake Status: {:?}", validator_list.validators[2].status);

    // crank stake pool to remove validator from list
    crank_stake_pool(&fixture).await;

    let validator_list: ValidatorList = fixture
        .load_and_deserialize(&fixture.stake_pool_meta.validator_list)
        .await;
    assert!(!validator_list
        .validators
        .iter()
        .any(|v| v.vote_account_address == extra_validator_accounts[2].vote_account));
    assert!(validator_list.validators.len() == 2);

    instant_remove_validator(&fixture, 2).await;
    let state_account: StewardStateAccount =
        fixture.load_and_deserialize(&fixture.steward_state).await;
    let state = state_account.state;
    assert!(matches!(
        state.state_tag,
        jito_steward::StewardStateEnum::ComputeInstantUnstake
    ));
    assert_eq!(state.validators_to_remove.count(), 0);
    assert_eq!(state.validators_for_immediate_removal.count(), 0);
    assert_eq!(state.num_pool_validators, 2);

    // Compute instant unstake transitions to Rebalance
    crank_compute_instant_unstake(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[2],
    )
    .await;

    crank_rebalance(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1],
    )
    .await;

    fixture.advance_num_epochs(1, 10).await;
    crank_stake_pool(&fixture).await;
    let validator_list: ValidatorList = fixture
        .load_and_deserialize(&fixture.stake_pool_meta.validator_list)
        .await;
    assert!(!validator_list
        .validators
        .iter()
        .any(|v| v.vote_account_address == extra_validator_accounts[2].vote_account));
    assert!(validator_list.validators.len() == 2);

    crank_epoch_maintenance(&fixture, None).await;
    let state_account: StewardStateAccount =
        fixture.load_and_deserialize(&fixture.steward_state).await;
    let state = state_account.state;
    assert!(matches!(
        state.state_tag,
        jito_steward::StewardStateEnum::Idle
    ));
    assert_eq!(state.validators_to_remove.count(), 0);
    assert_eq!(state.validators_for_immediate_removal.count(), 0);
    assert_eq!(state.num_pool_validators, 2);

    drop(fixture);
}

#[tokio::test]
async fn test_add_validator_next_cycle() {
    /*
      Tests that a validator added at an arbitrary point during the cycle does not get included in the
      current cycle's consideration, but is included in the next cycle's scoring after ComputeScores is run.
    */

    let mut fixture_accounts = FixtureDefaultAccounts::default();

    let unit_test_fixtures = StateMachineFixtures::default();

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
                num_epochs_between_scoring: Some(1), // 1 epoch cycle
                minimum_stake_lamports: Some(5_000_000_000),
                minimum_voting_epochs: Some(0), // Set to pass validation, where epochs starts at 0
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
    // Auto add validator - adds validators 2 and 3
    for extra_accounts in extra_validator_accounts.iter().take(2) {
        auto_add_validator(&fixture, extra_accounts).await;
    }

    crank_compute_score(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1],
    )
    .await;

    // Add in validator 2 at random time
    auto_add_validator(&fixture, &extra_validator_accounts[2]).await;

    let validator_list: ValidatorList = fixture
        .load_and_deserialize(&fixture.stake_pool_meta.validator_list)
        .await;
    assert!(validator_list
        .validators
        .iter()
        .any(|v| v.vote_account_address == extra_validator_accounts[2].vote_account));
    assert!(validator_list.validators.len() == 3);

    // Ensure that num_pool_validators isn't updated but validators_added is
    let state_account: StewardStateAccount =
        fixture.load_and_deserialize(&fixture.steward_state).await;
    let state = state_account.state;

    assert!(matches!(
        state.state_tag,
        jito_steward::StewardStateEnum::ComputeDelegations
    ));
    assert_eq!(state.validators_added, 1);
    assert_eq!(state.num_pool_validators, 2);

    crank_compute_delegations(&fixture).await;
    crank_idle(&fixture).await;
    crank_compute_instant_unstake(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1],
    )
    .await;
    crank_rebalance(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1],
    )
    .await;

    fixture.advance_num_epochs(1, 10).await;

    crank_stake_pool(&fixture).await;
    crank_epoch_maintenance(&fixture, None).await;

    let state_account: StewardStateAccount =
        fixture.load_and_deserialize(&fixture.steward_state).await;
    let state = state_account.state;

    assert!(matches!(
        state.state_tag,
        jito_steward::StewardStateEnum::Idle
    ));
    assert_eq!(state.validators_added, 1);
    assert_eq!(state.num_pool_validators, 2);

    crank_validator_history_accounts(&fixture, &extra_validator_accounts, &[0, 1, 2]).await;

    // Ensure we're in the next cycle
    crank_compute_score(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0],
    )
    .await;

    // Ensure that num_pool_validators is updated and validators_added is reset
    let state_account: StewardStateAccount =
        fixture.load_and_deserialize(&fixture.steward_state).await;
    let state = state_account.state;

    assert!(matches!(
        state.state_tag,
        jito_steward::StewardStateEnum::ComputeScores
    ));

    assert_eq!(state.validators_added, 0);
    assert!(state.validators_to_remove.is_empty());
    assert_eq!(state.num_pool_validators, 3);

    // Ensure we can crank the new validator
    crank_compute_score(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[2],
    )
    .await;

    drop(fixture);
}
