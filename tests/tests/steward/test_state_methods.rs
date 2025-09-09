/*
- General operation of each method
    - Cover all code paths (error cases, other ifs)
- Test wrong state errors


*/
use crate::steward::serialize_validator_list;
use anchor_lang::error::Error;
use jito_steward::{
    constants::{LAMPORT_BALANCE_DEFAULT, MAX_VALIDATORS, SORTED_INDEX_DEFAULT},
    delegation::RebalanceType,
    errors::StewardError,
    Delegation, StewardStateEnum, StewardStateV2,
};
use solana_sdk::native_token::LAMPORTS_PER_SOL;
use spl_stake_pool::big_vec::BigVec;
use tests::steward_fixtures::StateMachineFixtures;
use validator_history::ValidatorHistoryEntry;

#[test]
fn test_compute_scores() {
    /*
    - [ ]  `compute_scores`
        - [X]  InvalidState error
        - [X]  ValidatorHistoryNotRecentEnough error
        - [X]  ClusterHistoryNotRecentEnough error
        - [X]  Blacklist validator
        - [X]  Empty progress (is this checked with the state transition? actually no… need to call the method)
            -
    */
    let mut fixtures = Box::<StateMachineFixtures>::default();

    let current_epoch = fixtures.current_epoch;
    let clock = &mut fixtures.clock;
    let epoch_schedule = &fixtures.epoch_schedule;
    let validators = &mut fixtures.validators;
    let cluster_history = &mut fixtures.cluster_history;
    let config = &mut fixtures.config;
    let parameters = config.parameters;
    let state = &mut fixtures.state;

    // Test normal run
    let cloned_validators = Box::new(validators.clone());

    for validator in cloned_validators.iter() {
        let res = state.compute_score(
            clock,
            epoch_schedule,
            validator,
            validator.index as usize,
            cluster_history,
            config,
            state.num_pool_validators,
        );
        assert!(res.is_ok());
        assert!(matches!(state.state_tag, StewardStateEnum::ComputeScores));
    }
    assert!(state
        .progress
        .is_complete(state.num_pool_validators)
        .unwrap());
    // Updated assertions for new bit-packed scoring system
    // The scores are now u64 values with bit-packed fields
    assert!(state.scores[0..3] == [7249739868903849600, 0, 6887252875458657920]);
    assert!(state.sorted_score_indices[0..3] == [0, 2, 1]);
    assert!(state.sorted_score_indices[3..] == [SORTED_INDEX_DEFAULT; MAX_VALIDATORS - 3]);
    assert!(
        state.raw_scores[0..3] == [7249739868903849600, 72057594037931136, 6887252875458657920]
    );
    assert!(state.sorted_raw_score_indices[0..3] == [0, 2, 1]);
    assert!(state.sorted_raw_score_indices[3..] == [SORTED_INDEX_DEFAULT; MAX_VALIDATORS - 3]);
    assert!(state.start_computing_scores_slot == clock.slot);
    assert!(state.next_cycle_epoch == current_epoch + parameters.num_epochs_between_scoring);
    assert!(state.current_epoch == current_epoch);

    // Test invalid state
    state.progress.reset();
    state.state_tag = StewardStateEnum::Idle;
    let res = state.compute_score(
        clock,
        epoch_schedule,
        &validators[0],
        validators[0].index as usize,
        cluster_history,
        config,
        state.num_pool_validators,
    );
    assert_eq!(res, Err(Error::from(StewardError::InvalidState)));

    // Test ValidatorHistoryNotRecentEnough error
    state.state_tag = StewardStateEnum::ComputeScores;
    let mut validator = Box::new(validators[0]);
    validator
        .history
        .last_mut()
        .unwrap()
        .vote_account_last_update_slot = epoch_schedule.get_last_slot_in_epoch(current_epoch - 1);

    let res = state.compute_score(
        clock,
        epoch_schedule,
        &validator,
        validator.index as usize,
        cluster_history,
        config,
        state.num_pool_validators,
    );
    assert_eq!(
        res,
        Err(Error::from(StewardError::VoteHistoryNotRecentEnough))
    );

    let mut validator = Box::new(validators[0]);

    // TODO expose default for CircBuf
    validator.history.is_empty = 1;
    validator.history.idx = 0;
    validator.history.arr = [ValidatorHistoryEntry::default(); 512];
    let res = state.compute_score(
        clock,
        epoch_schedule,
        &validator,
        validator.index as usize,
        cluster_history,
        config,
        state.num_pool_validators,
    );
    assert_eq!(
        res,
        Err(Error::from(StewardError::VoteHistoryNotRecentEnough))
    );

    // Test ClusterHistoryNotRecentEnough error
    cluster_history.cluster_history_last_update_slot =
        epoch_schedule.get_last_slot_in_epoch(current_epoch - 1);

    let res = state.compute_score(
        clock,
        epoch_schedule,
        &validators[0],
        validators[0].index as usize,
        cluster_history,
        config,
        state.num_pool_validators,
    );
    assert_eq!(
        res,
        Err(Error::from(StewardError::ClusterHistoryNotRecentEnough))
    );
    cluster_history.cluster_history_last_update_slot =
        epoch_schedule.get_first_slot_in_epoch(current_epoch);

    state.progress.reset();

    // Test blacklist validator
    config
        .validator_history_blacklist
        .set(validators[0].index as usize, true)
        .unwrap();
    let res = state.compute_score(
        clock,
        epoch_schedule,
        &validators[0],
        validators[0].index as usize,
        cluster_history,
        config,
        state.num_pool_validators,
    );
    assert!(res.is_ok());
    // validator would not have a score of 0 if it was not blacklisted
    assert!(state.scores[validators[0].index as usize] == 0);
    assert!(state.sorted_score_indices[0] == 0);
    assert!(state.raw_scores[0] == 7249739868903849600); // Updated for new scoring system
    assert!(state.sorted_raw_score_indices[0] == 0);

    // Test reset scoring: 3 cases

    // 1) Progress empty case
    state.progress.reset();
    state.num_pool_validators = 4;
    clock.slot = epoch_schedule.get_last_slot_in_epoch(current_epoch);
    let res = state.compute_score(
        clock,
        epoch_schedule,
        &validators[0],
        validators[0].index as usize,
        cluster_history,
        config,
        state.num_pool_validators,
    );
    assert!(res.is_ok());
    assert!(state.start_computing_scores_slot == clock.slot);
    assert!(state.next_cycle_epoch == current_epoch + parameters.num_epochs_between_scoring);
    assert!(state.num_pool_validators == 4);

    // 2) Progress stalled and time moved into next epoch
    // Conditions: clock.epoch > state.current_epoch and !state.progress.is_empty()
    // REDACTED: The epoch is now updated in the epoch_maintenance method

    // state.current_epoch = current_epoch - 1;
    // assert!(!state.progress.is_empty());
    // assert!(state.current_epoch < clock.epoch);
    // let res = state.compute_score(
    //     clock,
    //     epoch_schedule,
    //     &validators[0],
    //     validators[0].index as usize,
    //     cluster_history,
    //     config,
    //     state.num_pool_validators,
    // );
    // assert!(res.is_ok());
    // assert!(state.current_epoch == current_epoch);

    // 3) Progress started, but took >1000 slots to complete
    // Conditions: start_computing_scores_slot > 1000 slots ago, !progress.is_empty(), and clock.epoch == state.current_epoch
    assert!(
        state.start_computing_scores_slot == epoch_schedule.get_last_slot_in_epoch(current_epoch)
    );
    assert!(!state.progress.is_empty());
    assert!(state.current_epoch == clock.epoch);
    state.start_computing_scores_slot = epoch_schedule.get_first_slot_in_epoch(current_epoch);
    let res = state.compute_score(
        clock,
        epoch_schedule,
        &validators[0],
        validators[0].index as usize,
        cluster_history,
        config,
        state.num_pool_validators,
    );
    assert!(res.is_ok());
    assert!(state.start_computing_scores_slot == clock.slot);
}

#[test]
fn test_compute_delegations() {
    // - [ ]  `compute_delegations`
    // - [ ]  Regular run
    // - [ ]  InvalidState error

    let mut fixtures = Box::<StateMachineFixtures>::default();
    let state = &mut fixtures.state;
    let clock = &mut fixtures.clock;
    let config = &fixtures.config;

    // Regular run
    state.scores[0..3].copy_from_slice(&[1_000_000_000, 1_000_000_000, 1_000_000_000]);
    state.sorted_score_indices[0..3].copy_from_slice(&[0, 1, 2]);
    state.sorted_raw_score_indices[0..3].copy_from_slice(&[0, 1, 2]);
    state.state_tag = StewardStateEnum::ComputeDelegations;
    assert!(config.parameters.num_delegation_validators == 3);
    let res = state.compute_delegations(clock.epoch, config);
    assert!(res.is_ok());
    assert!(matches!(
        state.state_tag,
        StewardStateEnum::ComputeDelegations
    ));
    assert!(
        state.delegations[0..3]
            == [
                Delegation::new(1, 3),
                Delegation::new(1, 3),
                Delegation::new(1, 3)
            ]
    );

    // Delegate with fewer non-zero score validators than the number of pool validators
    state.delegations = [Delegation::default(); MAX_VALIDATORS];
    state.scores[0..3].copy_from_slice(&[1_000_000_000, 0, 1_000_000_000]);
    state.sorted_score_indices[0..3].copy_from_slice(&[0, 2, 1]);
    state.sorted_raw_score_indices[0..3].copy_from_slice(&[0, 2, 1]);
    let res = state.compute_delegations(clock.epoch, config);
    assert!(res.is_ok());
    assert!(
        state.delegations[0..3]
            == [
                Delegation::new(1, 2),
                Delegation::new(0, 1),
                Delegation::new(1, 2)
            ]
    );

    // Test invalid state
    state.state_tag = StewardStateEnum::Idle;
    let res = state.compute_delegations(clock.epoch, config);
    assert!(res == Err(Error::from(StewardError::InvalidState)));

    // Next compute scores epoch
    state.state_tag = StewardStateEnum::ComputeDelegations;
    clock.epoch += config.parameters.num_epochs_between_scoring;
    let res = state.compute_delegations(clock.epoch, config);
    assert!(res == Err(Error::from(StewardError::InvalidState)));
}

#[test]
fn test_compute_instant_unstake_errors() {
    /*
    - [ ]  `compute_instant_unstake`
        - [X]  InvalidState error
        - [X]  InstantUnstakeNotReady Error
        - [X]  ValidatorHistoryNotRecentEnough x2
        - [X]  ClusterHistoryNotRecentEnough Error
    */
    let mut fixtures = Box::<StateMachineFixtures>::default();
    let state = &mut fixtures.state;
    let clock = &mut fixtures.clock;
    let epoch_schedule = &fixtures.epoch_schedule;
    let current_epoch = &fixtures.current_epoch;
    let validators = &mut fixtures.validators;
    let cluster_history = &mut fixtures.cluster_history;
    let config = &mut fixtures.config;

    // InstantUnstakeNotReady: slot too low
    state.state_tag = StewardStateEnum::ComputeInstantUnstake;
    clock.slot = epoch_schedule.get_first_slot_in_epoch(*current_epoch);
    let res = state.compute_instant_unstake(
        clock,
        epoch_schedule,
        &validators[0],
        validators[0].index as usize,
        cluster_history,
        config,
    );
    assert!(res == Err(Error::from(StewardError::InstantUnstakeNotReady)));

    // InvalidState
    state.state_tag = StewardStateEnum::Idle;
    let res = state.compute_instant_unstake(
        clock,
        epoch_schedule,
        &validators[0],
        validators[0].index as usize,
        cluster_history,
        config,
    );
    assert!(res == Err(Error::from(StewardError::InvalidState)));

    state.state_tag = StewardStateEnum::ComputeInstantUnstake;
    clock.epoch += config.parameters.num_epochs_between_scoring;
    let res = state.compute_instant_unstake(
        clock,
        epoch_schedule,
        &validators[0],
        validators[0].index as usize,
        cluster_history,
        config,
    );
    assert!(res == Err(Error::from(StewardError::InvalidState)));

    // ValidatorHistoryNotRecentEnough
    clock.epoch = *current_epoch;
    clock.slot = epoch_schedule.get_last_slot_in_epoch(*current_epoch);
    let mut validator = Box::new(validators[0]);
    validator
        .history
        .last_mut()
        .unwrap()
        .vote_account_last_update_slot = epoch_schedule.get_last_slot_in_epoch(current_epoch - 1);
    let res = state.compute_instant_unstake(
        clock,
        epoch_schedule,
        &validator,
        validator.index as usize,
        cluster_history,
        config,
    );
    assert!(res == Err(Error::from(StewardError::VoteHistoryNotRecentEnough)));

    let mut validator = Box::new(validators[0]);

    // TODO expose default for CircBuf
    validator.history.is_empty = 1;
    validator.history.idx = 0;
    validator.history.arr = [ValidatorHistoryEntry::default(); 512];
    let res = state.compute_instant_unstake(
        clock,
        epoch_schedule,
        &validator,
        validator.index as usize,
        cluster_history,
        config,
    );
    assert!(res == Err(Error::from(StewardError::VoteHistoryNotRecentEnough)));

    // ClusterHistoryNotRecentEnough
    cluster_history.cluster_history_last_update_slot =
        epoch_schedule.get_last_slot_in_epoch(current_epoch - 1);

    let res = state.compute_instant_unstake(
        clock,
        epoch_schedule,
        &validators[0],
        validators[0].index as usize,
        cluster_history,
        config,
    );
    assert!(res == Err(Error::from(StewardError::ClusterHistoryNotRecentEnough)));
}

#[test]
fn test_compute_instant_unstake_success() {
    let mut fixtures = Box::<StateMachineFixtures>::default();
    let state = &mut fixtures.state;
    let clock = &mut fixtures.clock;
    let epoch_schedule = &fixtures.epoch_schedule;
    let current_epoch = &fixtures.current_epoch;
    let validators = &fixtures.validators;
    let cluster_history = &fixtures.cluster_history;
    let config = &mut fixtures.config;

    state.state_tag = StewardStateEnum::ComputeInstantUnstake;
    clock.slot = epoch_schedule.get_last_slot_in_epoch(*current_epoch);
    state.delegations[validators[0].index as usize] = Delegation::new(1, 1);

    // Non instant-unstakeable validator
    let res = state.compute_instant_unstake(
        clock,
        epoch_schedule,
        &validators[0],
        validators[0].index as usize,
        cluster_history,
        config,
    );
    assert!(res.is_ok());
    assert!(matches!(
        state.state_tag,
        StewardStateEnum::ComputeInstantUnstake
    ));
    assert!(!state
        .instant_unstake
        .get(validators[0].index as usize)
        .unwrap());

    // Should skip validator since it's already been computed
    let res = state.compute_instant_unstake(
        clock,
        epoch_schedule,
        &validators[0],
        validators[0].index as usize,
        cluster_history,
        config,
    );
    assert!(res.is_ok());

    // Instant unstakeable validator
    state.progress.reset();
    state.instant_unstake.reset();
    config
        .validator_history_blacklist
        .set(validators[0].index as usize, true)
        .unwrap();

    let res = state.compute_instant_unstake(
        clock,
        epoch_schedule,
        &validators[0],
        validators[0].index as usize,
        cluster_history,
        config,
    );
    assert!(res.is_ok());
    assert!(state
        .instant_unstake
        .get(validators[0].index as usize)
        .unwrap());

    // Instant unstakeable validator with no delegation amount
    state.delegations[validators[0].index as usize] = Delegation::new(0, 1);
    state.progress.reset();
    state.instant_unstake.reset();
    let res = state.compute_instant_unstake(
        clock,
        epoch_schedule,
        &validators[0],
        validators[0].index as usize,
        cluster_history,
        config,
    );
    assert!(res.is_ok());
    assert!(state
        .instant_unstake
        .get(validators[0].index as usize)
        .unwrap());
}

#[test]
fn test_rebalance() {
    /*
    - [ ]  `rebalance`
        - [X]  InvalidState error
        - [X]  positive rebalance nonzero
        - [X]  negative rebalance + instant unstake: test rebalancing of `self.delegations`
        - [X]  negative rebalance zero
    */
    let mut fixtures = Box::<StateMachineFixtures>::default();
    fixtures.config.parameters.scoring_unstake_cap_bps = 10000;
    fixtures.config.parameters.instant_unstake_cap_bps = 10000;
    fixtures.config.parameters.stake_deposit_unstake_cap_bps = 10000;

    let state = &mut fixtures.state;

    // Increase stake
    // validator_list: all validators have 1000 SOL
    // reserve_stake: 1000 SOL
    // give all possible SOL to validator 1
    state.state_tag = StewardStateEnum::Rebalance;
    state.delegations[0..3].copy_from_slice(&[
        Delegation::new(1, 1),
        Delegation::default(),
        Delegation::default(),
    ]);
    state.scores[0..3].copy_from_slice(&[1_000_000_000, 0, 0]);
    state.sorted_score_indices[0..3].copy_from_slice(&[0, 1, 2]);

    let mut serialized_data = serialize_validator_list(&fixtures.validator_list);
    let validator_list_bigvec = BigVec {
        data: &mut serialized_data,
    };

    let res = state.rebalance(
        fixtures.current_epoch,
        0,
        &validator_list_bigvec,
        4000 * LAMPORTS_PER_SOL,
        1000 * LAMPORTS_PER_SOL,
        u64::from(fixtures.validator_list[0].active_stake_lamports),
        0,
        0,
        &fixtures.config.parameters,
    );
    assert!(res.is_ok());
    match res.unwrap() {
        RebalanceType::Increase(lamports) => {
            assert!(lamports == 1000 * LAMPORTS_PER_SOL);
        }
        _ => panic!("Expected RebalanceType::Increase"),
    }

    // Decrease stake with instant unstake

    state.delegations[0..3].copy_from_slice(&[
        Delegation::new(1, 2),
        Delegation::new(1, 2),
        Delegation::new(0, 1),
    ]);
    state.scores[0..3].copy_from_slice(&[1_000_000_000, 500_000_000, 0]);
    state.sorted_score_indices[0..3].copy_from_slice(&[0, 1, 2]);
    state.sorted_raw_score_indices[0..3].copy_from_slice(&[0, 1, 2]);
    // Second validator is instant unstakeable
    state.instant_unstake.set(1, true).unwrap();

    // Validator index 0: 1000 SOL, 1 score, 1 delegation -> Keeps its stake
    // Validator index 1: 1000 SOL, 0.5 score, 0 delegation, -> Decrease stake, from "instant unstake" category, and set delegation to 0
    // Validator index 2: 1000 SOL, 0 score, 0 delegation -> Decrease stake, from "regular unstake" category

    let res = state.rebalance(
        fixtures.current_epoch,
        1,
        &validator_list_bigvec,
        4000 * LAMPORTS_PER_SOL,
        1000 * LAMPORTS_PER_SOL,
        u64::from(fixtures.validator_list[1].active_stake_lamports),
        0,
        0,
        &fixtures.config.parameters,
    );

    assert!(res.is_ok());
    match res.unwrap() {
        RebalanceType::Decrease(decrease_components) => {
            assert_eq!(
                decrease_components.total_unstake_lamports,
                1000 * LAMPORTS_PER_SOL
            );
            assert_eq!(
                decrease_components.instant_unstake_lamports,
                1000 * LAMPORTS_PER_SOL
            );
            assert_eq!(decrease_components.scoring_unstake_lamports, 0);
            assert_eq!(decrease_components.stake_deposit_unstake_lamports, 0);

            assert!(
                state.delegations[0..3]
                    == [
                        Delegation::new(1, 1),
                        Delegation::new(0, 1),
                        Delegation::new(0, 1)
                    ]
            );
        }
        _ => panic!("Expected RebalanceType::Decrease"),
    }

    // Test that rebalance will be skipped if validator has already been run
    let res = state.rebalance(
        fixtures.current_epoch,
        1,
        &validator_list_bigvec,
        4000 * LAMPORTS_PER_SOL,
        1000 * LAMPORTS_PER_SOL,
        u64::from(fixtures.validator_list[1].active_stake_lamports),
        0,
        0,
        &fixtures.config.parameters,
    );

    assert!(res.is_ok());
    match res.unwrap() {
        RebalanceType::None => {}
        _ => panic!("Expected RebalanceType::None"),
    }

    // Instant unstake validator, but no delegation, so other delegations are not affected
    // Same scenario as above but out-of-band validator
    state.delegations[0..3].copy_from_slice(&[
        Delegation::new(1, 2),
        Delegation::new(0, 1),
        Delegation::new(1, 2),
    ]);
    state.scores[0..3].copy_from_slice(&[1_000_000_000, 500_000_000, 0]);
    state.sorted_score_indices[0..3].copy_from_slice(&[0, 1, 2]);
    state.sorted_raw_score_indices[0..3].copy_from_slice(&[0, 1, 2]);
    // Second validator is instant unstakeable
    state.instant_unstake.set(1, true).unwrap();
    state.validator_lamport_balances[1] = 1000 * LAMPORTS_PER_SOL;

    // Validator index 0: 1000 SOL, 1 score, 1 delegation -> Keeps its stake
    // Validator index 1: 1000 SOL, 0.5 score, 0 delegation, -> Decrease stake, from "instant unstake" category, and set delegation to 0
    // Validator index 2: 1000 SOL, 0 score, 0 delegation -> Decrease stake, from "regular unstake" category

    state.progress.reset();
    let res = state.rebalance(
        fixtures.current_epoch,
        1,
        &validator_list_bigvec,
        4000 * LAMPORTS_PER_SOL,
        1000 * LAMPORTS_PER_SOL,
        u64::from(fixtures.validator_list[1].active_stake_lamports),
        0,
        0,
        &fixtures.config.parameters,
    );
    assert!(res.is_ok());
    match res.unwrap() {
        RebalanceType::Decrease(decrease_components) => {
            assert_eq!(
                decrease_components.total_unstake_lamports,
                1000 * LAMPORTS_PER_SOL
            );
            assert_eq!(
                decrease_components.instant_unstake_lamports,
                1000 * LAMPORTS_PER_SOL
            );
            assert_eq!(decrease_components.scoring_unstake_lamports, 0);
            assert_eq!(decrease_components.stake_deposit_unstake_lamports, 0);

            assert!(
                state.delegations[0..3]
                    == [
                        Delegation::new(1, 2),
                        Delegation::new(0, 1),
                        Delegation::new(1, 2)
                    ]
            );
        }
        _ => panic!("Expected RebalanceType::Decrease"),
    }

    // Decrease, instant unstake on the last eligible validator
    state.instant_unstake_total = 0;
    state.scoring_unstake_total = 0;
    state.stake_deposit_unstake_total = 0;
    state.delegations[0..3].copy_from_slice(&[
        Delegation::new(0, 1),
        Delegation::default(),
        Delegation::default(),
    ]);
    state.scores[0..3].copy_from_slice(&[1_000_000_000, 0, 0]);
    state.sorted_score_indices[0..3].copy_from_slice(&[0, 1, 2]);
    state.sorted_raw_score_indices[0..3].copy_from_slice(&[0, 1, 2]);
    state.instant_unstake.reset();
    state.instant_unstake.set(0, true).unwrap();

    state.progress.reset();
    let res = state.rebalance(
        fixtures.current_epoch,
        0,
        &validator_list_bigvec,
        4000 * LAMPORTS_PER_SOL,
        1000 * LAMPORTS_PER_SOL,
        u64::from(fixtures.validator_list[0].active_stake_lamports),
        0,
        0,
        &fixtures.config.parameters,
    );
    assert!(res.is_ok());
    match res.unwrap() {
        RebalanceType::Decrease(decrease_components) => {
            assert!(decrease_components.total_unstake_lamports == 1000 * LAMPORTS_PER_SOL);
            assert!(decrease_components.scoring_unstake_lamports == 0);
            assert!(decrease_components.instant_unstake_lamports == 1000 * LAMPORTS_PER_SOL);
            assert!(decrease_components.stake_deposit_unstake_lamports == 0);
            assert!(state.delegations[0..3] == [Delegation::default(); 3]);
        }
        _ => panic!("Expected RebalanceType::Decrease"),
    }

    // No rebalance
    state.instant_unstake_total = 0;
    state.scoring_unstake_total = 0;
    state.stake_deposit_unstake_total = 0;
    state.instant_unstake.reset();
    state.delegations[0..3].copy_from_slice(&[Delegation::new(1, 3); 3]);
    state.scores[0..3].copy_from_slice(&[1_000_000_000, 1_000_000_000, 1_000_000_000]);
    state.sorted_score_indices[0..3].copy_from_slice(&[0, 1, 2]);
    state.sorted_raw_score_indices[0..3].copy_from_slice(&[0, 1, 2]);
    state.progress.reset();
    let res = state.rebalance(
        fixtures.current_epoch,
        0,
        &validator_list_bigvec,
        3000 * LAMPORTS_PER_SOL,
        0,
        u64::from(fixtures.validator_list[0].active_stake_lamports),
        0,
        0,
        &fixtures.config.parameters,
    );
    assert!(res.is_ok());
    match res.unwrap() {
        RebalanceType::None => {}
        _ => panic!("Expected RebalanceType::None"),
    }

    // Invalid State
    state.state_tag = StewardStateEnum::Idle;
    let res = state.rebalance(
        fixtures.current_epoch,
        0,
        &validator_list_bigvec,
        3000 * LAMPORTS_PER_SOL,
        0,
        u64::from(fixtures.validator_list[0].active_stake_lamports),
        0,
        0,
        &fixtures.config.parameters,
    );
    match res {
        Ok(_) => panic!("Expected StewardError::InvalidState"),
        Err(e) => {
            assert_eq!(e, Error::from(StewardError::InvalidState));
        }
    }
}

#[test]
fn test_rebalance_default_lamports() {
    let fixtures = Box::<StateMachineFixtures>::default();
    let mut state = fixtures.state;
    let mut validator_list = fixtures.validator_list.clone();

    // Case 1: Lamports default, has transient stake
    state.validator_lamport_balances[0] = LAMPORT_BALANCE_DEFAULT;
    state.state_tag = StewardStateEnum::Rebalance;
    state.delegations[0..3].copy_from_slice(&[
        Delegation::new(1, 1),
        Delegation::default(),
        Delegation::default(),
    ]);
    state.scores[0..3].copy_from_slice(&[1_000_000_000, 0, 0]);
    state.sorted_score_indices[0..3].copy_from_slice(&[0, 1, 2]);

    validator_list[0].transient_stake_lamports = 1000.into();

    let mut serialized_data = serialize_validator_list(&validator_list);
    let validator_list_bigvec = BigVec {
        data: &mut serialized_data,
    };

    let res = state.rebalance(
        fixtures.current_epoch,
        0,
        &validator_list_bigvec,
        3000 * LAMPORTS_PER_SOL,
        0,
        u64::from(validator_list[0].active_stake_lamports),
        0,
        0,
        &fixtures.config.parameters,
    );

    assert!(res.is_ok());
    match res.unwrap() {
        RebalanceType::None => {}
        _ => panic!("Expected RebalanceType::Increase"),
    }
    assert_eq!(state.validator_lamport_balances[0], LAMPORT_BALANCE_DEFAULT);

    // Case 2: Lamports not default, no transient stake
    let mut state = fixtures.state;
    state.state_tag = StewardStateEnum::Rebalance;
    state.delegations[0..3].copy_from_slice(&[
        Delegation::new(1, 1),
        Delegation::default(),
        Delegation::default(),
    ]);
    state.scores[0..3].copy_from_slice(&[1_000_000_000, 0, 0]);
    state.sorted_score_indices[0..3].copy_from_slice(&[0, 1, 2]);

    state.validator_lamport_balances[0] = LAMPORT_BALANCE_DEFAULT;
    validator_list[0].transient_stake_lamports = 0.into();

    let mut serialized_data = serialize_validator_list(&validator_list);
    let validator_list_bigvec = BigVec {
        data: &mut serialized_data,
    };

    let res = state.rebalance(
        fixtures.current_epoch,
        0,
        &validator_list_bigvec,
        4000 * LAMPORTS_PER_SOL,
        1000 * LAMPORTS_PER_SOL,
        u64::from(validator_list[0].active_stake_lamports),
        0,
        0,
        &fixtures.config.parameters,
    );

    assert!(res.is_ok());
    if let RebalanceType::Increase(increase_amount) = res.unwrap() {
        assert_eq!(
            state.validator_lamport_balances[0],
            1000 * LAMPORTS_PER_SOL + increase_amount
        );
    } else {
        panic!("Expected RebalanceType::Increase");
    }
}

fn _test_remove_validator_setup(fixtures: &StateMachineFixtures) -> StewardStateV2 {
    let mut state = fixtures.state;
    // Set values for all of the values that are gonna get shifted
    state.validator_lamport_balances[0..3].copy_from_slice(&[0, 1, 2]);
    state.scores[0..3].copy_from_slice(&[0, 1, 2]);
    state.raw_scores[0..3].copy_from_slice(&[0, 1, 2]);
    state.delegations[0..3].copy_from_slice(&[
        Delegation::new(0, 1),
        Delegation::new(1, 1),
        Delegation::new(2, 1),
    ]);
    state.instant_unstake.reset();
    state.instant_unstake.set(0, true).unwrap();
    state.instant_unstake.set(1, false).unwrap();
    state.instant_unstake.set(2, true).unwrap();

    state
}
#[test]
fn test_remove_validator() {
    // Setup: create steward state based off StewardStateFixtures
    // mark index 1 to removal
    let fixtures = Box::<StateMachineFixtures>::default();
    let mut state = _test_remove_validator_setup(&fixtures);

    // test basic case - remove validator_to_remove
    state.validators_to_remove.set(1, true).unwrap();
    let res = state.remove_validator(1);
    assert!(res.is_ok());
    assert_eq!(state.num_pool_validators, 2);
    // Assert that values were shifted left
    assert_eq!(state.raw_scores[1], 2);
    assert_eq!(state.scores[1], 2);
    assert!(state.delegations[1] == Delegation::new(2, 1));

    // test basic case - remove immediate_removal validator
    let mut state = _test_remove_validator_setup(&fixtures);

    state.validators_for_immediate_removal.set(1, true).unwrap();
    let res = state.remove_validator(1);
    assert!(res.is_ok());
    assert_eq!(state.num_pool_validators, 2);
    // Assert that values were shifted left
    assert_eq!(state.raw_scores[1], 2);
    assert_eq!(state.scores[1], 2);
    assert!(state.delegations[1] == Delegation::new(2, 1));

    // Setup: mark an index for removal that's higher than num_pool_validators
    // Remember this is always gonna be run after actual removals have taken place, so could validator_list_len be kind of a red herring? do we need to go further?

    let mut state = _test_remove_validator_setup(&fixtures);

    state.validators_for_immediate_removal.set(3, true).unwrap();
    state.validators_for_immediate_removal.set(4, true).unwrap();
    state.validators_added = 2;
    // both validators were removed from pool and now the validator list is down to 3
    let res = state.remove_validator(3);
    assert!(res.is_ok());

    assert_eq!(state.num_pool_validators, 3);
    assert!(state.validators_for_immediate_removal.get(3).unwrap());
    assert!(!state.validators_for_immediate_removal.get(4).unwrap());
}

#[test]
fn test_remove_validator_fails() {
    let fixtures = Box::<StateMachineFixtures>::default();
    let mut state = fixtures.state;

    // Test fails if validator not marked to remove
    state.validators_for_immediate_removal.reset();
    let res = state.remove_validator(0);
    assert!(res.is_err());
    assert!(res == Err(Error::from(StewardError::ValidatorNotMarkedForRemoval)));

    // Test fails out of bounds
    state
        .validators_for_immediate_removal
        .set(state.num_pool_validators as usize, true)
        .unwrap();
    let res = state.remove_validator(state.num_pool_validators as usize);
    assert!(res.is_err());
    assert!(res == Err(Error::from(StewardError::ValidatorIndexOutOfBounds)));
}

#[test]
fn test_rebalance_max_lamports() {
    let mut fixtures = Box::<StateMachineFixtures>::default();
    fixtures.config.parameters.scoring_unstake_cap_bps = 10000;
    fixtures.config.parameters.instant_unstake_cap_bps = 10000;
    fixtures.config.parameters.stake_deposit_unstake_cap_bps = 10000;

    const MAX_SOLANA_LAMPORTS: u64 = 600_000_000 * LAMPORTS_PER_SOL;
    let state = &mut fixtures.state;

    state.state_tag = StewardStateEnum::Rebalance;
    let mut validator_list_bytes = borsh1::to_vec(&fixtures.validator_list).unwrap();
    let validator_list_bigvec = BigVec {
        data: validator_list_bytes.as_mut_slice(),
    };

    state.delegations[0..3].copy_from_slice(&[
        Delegation::new(1, 2),
        Delegation::new(1, 2),
        Delegation::new(0, 1),
    ]);
    state.scores[0..3].copy_from_slice(&[1_000_000_000, 500_000_000, 0]);
    state.sorted_score_indices[0..3].copy_from_slice(&[0, 1, 2]);
    state.sorted_raw_score_indices[0..3].copy_from_slice(&[0, 1, 2]);
    // Second validator is instant unstakeable
    state.instant_unstake.set(1, true).unwrap();

    let res = state.rebalance(
        fixtures.current_epoch,
        1,
        &validator_list_bigvec,
        MAX_SOLANA_LAMPORTS,
        1000 * LAMPORTS_PER_SOL,
        u64::from(fixtures.validator_list[1].active_stake_lamports),
        0,
        0,
        &fixtures.config.parameters,
    );

    assert!(res.is_ok());
}
