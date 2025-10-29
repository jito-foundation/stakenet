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
use solana_sdk::pubkey::Pubkey;
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
    state.state_tag = StewardStateEnum::ComputeScores;

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
            3,
        );
        assert!(res.is_ok());
        assert!(matches!(state.state_tag, StewardStateEnum::ComputeScores));
    }
    assert!(state
        .progress
        .is_complete(state.num_pool_validators)
        .unwrap());
    assert!(state.scores[0..3] == [7249739868913833600, 0, 6887252875468641920]);
    assert!(state.sorted_score_indices[0..3] == [0, 2, 1]);
    assert!(state.sorted_raw_score_indices[3..] == [SORTED_INDEX_DEFAULT; MAX_VALIDATORS - 3]);
    assert!(state.raw_scores[0..3] == [1_000_000_000, 2_000_000, 950_000_000]);
    assert!(state.sorted_raw_score_indices[0..3] == [0, 2, 1]);
    assert!(state.sorted_raw_score_indices[3..] == [SORTED_INDEX_DEFAULT; MAX_VALIDATORS - 3]);
    assert!(state.start_computing_scores_slot == clock.slot);
    assert!(
        state.raw_scores[0..3] == [7249739868913833600, 72057594039927936, 6887252875468641920]
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
    assert!(state.raw_scores[0] == 7249739868913833600);
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
fn test_remove_validator_at_max_validators() {
    // Test case where num_pool_validators == MAX_VALIDATORS
    let fixtures = Box::<StateMachineFixtures>::default();
    let mut state = fixtures.state;
    state.num_pool_validators = MAX_VALIDATORS as u64;
    state.validators_added = 0;

    // Remove second-to-last validator
    let index = MAX_VALIDATORS - 2;
    state.validators_to_remove.set(index, true).unwrap();

    // Set test values
    state.validator_lamport_balances[index] = 998;
    state.validator_lamport_balances[index + 1] = 999;
    state.scores[index] = 998;
    state.scores[index + 1] = 999;

    let res = state.remove_validator(index);
    assert!(res.is_ok());

    // Verify shifting occurred - value at index should now be what was at index+1
    assert_eq!(state.validator_lamport_balances[index], 999);
    assert_eq!(state.scores[index], 999);

    // Verify the last position was cleared after shifting
    assert_eq!(
        state.validator_lamport_balances[index + 1],
        LAMPORT_BALANCE_DEFAULT
    );
    assert_eq!(state.scores[index + 1], 0);
}

#[test]
fn test_remove_validator_at_sum_equals_max() {
    // Test case where num_pool_validators + validators_added == MAX_VALIDATORS
    let fixtures = Box::<StateMachineFixtures>::default();
    let mut state = fixtures.state;

    // Set up state where sum equals MAX_VALIDATORS
    state.num_pool_validators = (MAX_VALIDATORS - 10) as u64;
    state.validators_added = 10;

    // Test removing from existing pool
    let index = MAX_VALIDATORS - 100;
    state.validators_to_remove.set(index, true).unwrap();

    // Set values to verify shifting
    state.validator_lamport_balances[index] = 100;
    state.validator_lamport_balances[index + 1] = 101;
    state.scores[index] = 100;
    state.scores[index + 1] = 101;

    let res = state.remove_validator(index);
    assert!(res.is_ok());
    assert_eq!(state.num_pool_validators, (MAX_VALIDATORS - 11) as u64);
    assert_eq!(state.validators_added, 10); // unchanged

    // Verify shifting occurred
    assert_eq!(state.validator_lamport_balances[index], 101);
    assert_eq!(state.scores[index], 101);

    // Test removing from added pool
    let mut state = fixtures.state;
    state.num_pool_validators = (MAX_VALIDATORS - 10) as u64;
    state.validators_added = 10;

    let index = MAX_VALIDATORS - 5;
    state
        .validators_for_immediate_removal
        .set(index, true)
        .unwrap();

    let res = state.remove_validator(index);
    assert!(res.is_ok());
    assert_eq!(state.num_pool_validators, (MAX_VALIDATORS - 10) as u64); // unchanged
    assert_eq!(state.validators_added, 9); // decremented
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

#[test]
fn test_directed_stake_preferences_valid() {
    use jito_steward::utils::U8Bool;
    use jito_steward::{DirectedStakePreference, DirectedStakeTicket, MAX_PREFERENCES_PER_TICKET};

    // Test case 1: Valid preferences under 10000 bps
    let mut ticket = DirectedStakeTicket {
        num_preferences: 3,
        staker_preferences: [DirectedStakePreference {
            vote_pubkey: Pubkey::default(),
            stake_share_bps: 4000,
            _padding0: [0; 94],
        }; MAX_PREFERENCES_PER_TICKET],
        ticket_update_authority: Pubkey::default(),
        ticket_holder_is_protocol: U8Bool::from(false),
        _padding0: [0; 125],
    };
    ticket.staker_preferences[1].stake_share_bps = 3000;
    ticket.staker_preferences[2].stake_share_bps = 3000;
    assert!(ticket.preferences_valid());

    // Test case 2: Invalid preferences over 10000 bps
    ticket.staker_preferences[2].stake_share_bps = 4000;
    assert!(!ticket.preferences_valid());

    // Test case 3: Edge case - exactly 10000 bps
    ticket.staker_preferences[0].stake_share_bps = 5000;
    ticket.staker_preferences[1].stake_share_bps = 3000;
    ticket.staker_preferences[2].stake_share_bps = 2000;
    assert!(ticket.preferences_valid());

    // Test case 4: Single preference
    ticket.num_preferences = 1;
    ticket.staker_preferences[0].stake_share_bps = 5000;
    assert!(ticket.preferences_valid());

    // Test case 5: Zero preferences
    ticket.num_preferences = 0;
    assert!(ticket.preferences_valid());

    // Test case 6: Multiple preferences with zero bps
    ticket.num_preferences = 3;
    ticket.staker_preferences[0].stake_share_bps = 0;
    ticket.staker_preferences[1].stake_share_bps = 0;
    ticket.staker_preferences[2].stake_share_bps = 0;
    assert!(ticket.preferences_valid());
}

#[test]
fn test_directed_stake_get_allocations() {
    use jito_steward::utils::U8Bool;
    use jito_steward::{DirectedStakePreference, DirectedStakeTicket, MAX_PREFERENCES_PER_TICKET};

    // Test case 1: Basic allocation with 3 validators
    let mut ticket = DirectedStakeTicket {
        num_preferences: 3,
        staker_preferences: [DirectedStakePreference {
            vote_pubkey: Pubkey::default(),
            stake_share_bps: 4000,
            _padding0: [0; 94],
        }; MAX_PREFERENCES_PER_TICKET],
        ticket_update_authority: Pubkey::default(),
        ticket_holder_is_protocol: U8Bool::from(false),
        _padding0: [0; 125],
    };
    let pk1 = Pubkey::new_unique();
    let pk2 = Pubkey::new_unique();
    let pk3 = Pubkey::new_unique();
    ticket.staker_preferences[0].vote_pubkey = pk1;
    ticket.staker_preferences[1].vote_pubkey = pk2;
    ticket.staker_preferences[1].stake_share_bps = 3000;
    ticket.staker_preferences[2].vote_pubkey = pk3;
    ticket.staker_preferences[2].stake_share_bps = 3000;

    let allocations = ticket.get_allocations(10_000);
    assert_eq!(allocations.len(), 3);
    assert_eq!(allocations[0], (pk1, 4000));
    assert_eq!(allocations[1], (pk2, 3000));
    assert_eq!(allocations[2], (pk3, 3000));

    // Test case 2: Allocation with zero bps preference
    ticket.staker_preferences[2].stake_share_bps = 0;
    let allocations = ticket.get_allocations(10_000);
    assert_eq!(allocations.len(), 2);
    assert_eq!(allocations[0], (pk1, 4000));
    assert_eq!(allocations[1], (pk2, 3000));

    // Test case 3: Large allocation amounts
    ticket.staker_preferences[0].stake_share_bps = 5000;
    ticket.staker_preferences[1].stake_share_bps = 5000;
    ticket.staker_preferences[2].stake_share_bps = 0;
    let allocations = ticket.get_allocations(1_000_000_000);
    assert_eq!(allocations.len(), 2);
    assert_eq!(allocations[0], (pk1, 500_000_000));
    assert_eq!(allocations[1], (pk2, 500_000_000));

    // Test case 4: Single preference allocation
    ticket.num_preferences = 1;
    let allocations = ticket.get_allocations(10_000);
    assert_eq!(allocations.len(), 1);
    assert_eq!(allocations[0], (pk1, 5000));

    // Test case 5: Zero total lamports
    let allocations = ticket.get_allocations(0);
    assert_eq!(allocations.len(), 0);

    // Test case 6: Rounding behavior with small amounts
    ticket.num_preferences = 3;
    ticket.staker_preferences[0].stake_share_bps = 3333;
    ticket.staker_preferences[1].stake_share_bps = 3333;
    ticket.staker_preferences[2].stake_share_bps = 3334;
    let allocations = ticket.get_allocations(100);
    assert_eq!(allocations.len(), 3);
    // Should handle rounding appropriately
    assert!(allocations[0].1 <= 34);
    assert!(allocations[1].1 <= 34);
    assert!(allocations[2].1 <= 34);

    // Test case 7: Under 100% allocation (less than 10,000 bps total)
    ticket.num_preferences = 2;
    ticket.staker_preferences[0].stake_share_bps = 3000; // 30%
    ticket.staker_preferences[1].stake_share_bps = 2000; // 20%
                                                         // Total: 50% (5000 bps), leaving 50% unallocated
    let allocations = ticket.get_allocations(10_000);
    assert_eq!(allocations.len(), 2);
    assert_eq!(allocations[0], (pk1, 3000)); // 30% of 10,000 = 3,000
    assert_eq!(allocations[1], (pk2, 2000)); // 20% of 10,000 = 2,000
                                             // Total allocated: 5,000 out of 10,000 (50%)
}

#[test]
fn test_directed_stake_whitelist_operations() {
    use jito_steward::{
        DirectedStakeWhitelist, MAX_PERMISSIONED_DIRECTED_STAKERS,
        MAX_PERMISSIONED_DIRECTED_VALIDATORS,
    };

    let mut whitelist = DirectedStakeWhitelist {
        permissioned_user_stakers: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_STAKERS],
        permissioned_protocol_stakers: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_STAKERS],
        permissioned_validators: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_VALIDATORS],
        total_permissioned_user_stakers: 0,
        total_permissioned_protocol_stakers: 0,
        total_permissioned_validators: 0,
        _padding0: [0; 250],
    };

    // Test case 1: Add staker successfully
    let staker1 = Pubkey::new_unique();
    let result = whitelist.add_user_staker(staker1);
    assert!(result.is_ok());
    assert_eq!(whitelist.total_permissioned_user_stakers, 1);
    assert!(whitelist.is_staker_permissioned(&staker1));

    // Test case 2: Add validator successfully
    let validator1 = Pubkey::new_unique();
    let result = whitelist.add_validator(validator1);
    assert!(result.is_ok());
    assert_eq!(whitelist.total_permissioned_validators, 1);
    assert!(whitelist.is_validator_permissioned(&validator1));

    // Test case 3: Add duplicate staker should fail
    let result = whitelist.add_user_staker(staker1);
    assert!(result.is_err());

    // Test case 4: Add duplicate validator should fail
    let result = whitelist.add_validator(validator1);
    assert!(result.is_err());

    // Test case 5: Check can_add methods
    assert!(whitelist.can_add_staker());
    assert!(whitelist.can_add_validator());

    // Test case 6: Add multiple stakers and validators
    let staker2 = Pubkey::new_unique();
    let validator2 = Pubkey::new_unique();
    assert!(whitelist.add_user_staker(staker2).is_ok());
    assert!(whitelist.add_validator(validator2).is_ok());
    assert_eq!(whitelist.total_permissioned_user_stakers, 2);
    assert_eq!(whitelist.total_permissioned_validators, 2);
    assert!(whitelist.is_staker_permissioned(&staker2));
    assert!(whitelist.is_validator_permissioned(&validator2));

    // Test case 7: Check non-permissioned entities
    let non_permissioned = Pubkey::new_unique();
    assert!(!whitelist.is_staker_permissioned(&non_permissioned));
    assert!(!whitelist.is_validator_permissioned(&non_permissioned));
}

#[test]
fn test_directed_stake_whitelist_remove_operations() {
    use jito_steward::{
        DirectedStakeWhitelist, MAX_PERMISSIONED_DIRECTED_STAKERS,
        MAX_PERMISSIONED_DIRECTED_VALIDATORS,
    };

    let mut whitelist = DirectedStakeWhitelist {
        permissioned_user_stakers: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_STAKERS],
        permissioned_protocol_stakers: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_STAKERS],
        permissioned_validators: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_VALIDATORS],
        total_permissioned_user_stakers: 0,
        total_permissioned_protocol_stakers: 0,
        total_permissioned_validators: 0,
        _padding0: [0; 250],
    };

    // Test case 1: Remove staker successfully
    let staker1 = Pubkey::new_unique();
    let staker2 = Pubkey::new_unique();
    let staker3 = Pubkey::new_unique();

    assert!(whitelist.add_user_staker(staker1).is_ok());
    assert!(whitelist.add_user_staker(staker2).is_ok());
    assert!(whitelist.add_user_staker(staker3).is_ok());
    assert_eq!(whitelist.total_permissioned_user_stakers, 3);

    // Remove middle staker
    let result = whitelist.remove_user_staker(&staker2);
    assert!(result.is_ok());
    assert_eq!(whitelist.total_permissioned_user_stakers, 2);
    assert!(whitelist.is_staker_permissioned(&staker1));
    assert!(!whitelist.is_staker_permissioned(&staker2));
    assert!(whitelist.is_staker_permissioned(&staker3));

    // Test case 2: Remove validator successfully
    let validator1 = Pubkey::new_unique();
    let validator2 = Pubkey::new_unique();
    let validator3 = Pubkey::new_unique();

    assert!(whitelist.add_validator(validator1).is_ok());
    assert!(whitelist.add_validator(validator2).is_ok());
    assert!(whitelist.add_validator(validator3).is_ok());
    assert_eq!(whitelist.total_permissioned_validators, 3);

    // Remove first validator
    let result = whitelist.remove_validator(&validator1);
    assert!(result.is_ok());
    assert_eq!(whitelist.total_permissioned_validators, 2);
    assert!(!whitelist.is_validator_permissioned(&validator1));
    assert!(whitelist.is_validator_permissioned(&validator2));
    assert!(whitelist.is_validator_permissioned(&validator3));

    // Test case 3: Remove non-existent staker should fail
    let non_existent_staker = Pubkey::new_unique();
    let result = whitelist.remove_user_staker(&non_existent_staker);
    assert!(result.is_err());
    assert!(result == Err(Error::from(StewardError::StakerNotInWhitelist)));

    // Test case 4: Remove non-existent validator should fail
    let non_existent_validator = Pubkey::new_unique();
    let result = whitelist.remove_validator(&non_existent_validator);
    assert!(result.is_err());
    assert!(result == Err(Error::from(StewardError::ValidatorNotInWhitelist)));

    // Test case 5: Remove from empty staker list should fail
    let mut empty_whitelist = DirectedStakeWhitelist {
        permissioned_user_stakers: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_STAKERS],
        permissioned_protocol_stakers: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_STAKERS],
        permissioned_validators: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_VALIDATORS],
        total_permissioned_user_stakers: 0,
        total_permissioned_protocol_stakers: 0,
        total_permissioned_validators: 0,
        _padding0: [0; 250],
    };
    let result = empty_whitelist.remove_user_staker(&Pubkey::new_unique());
    assert!(result.is_err());
    assert!(result == Err(Error::from(StewardError::StakerNotInWhitelist)));

    // Test case 6: Remove from empty validator list should fail
    let result = empty_whitelist.remove_validator(&Pubkey::new_unique());
    assert!(result.is_err());
    assert!(result == Err(Error::from(StewardError::ValidatorNotInWhitelist)));

    // Test case 7: Remove last staker and verify can add again
    assert!(whitelist.remove_user_staker(&staker1).is_ok());
    assert!(whitelist.remove_user_staker(&staker3).is_ok());
    assert_eq!(whitelist.total_permissioned_user_stakers, 0);
    assert!(whitelist.can_add_staker());

    let new_staker = Pubkey::new_unique();
    assert!(whitelist.add_user_staker(new_staker).is_ok());
    assert_eq!(whitelist.total_permissioned_user_stakers, 1);

    // Test case 8: Remove last validator and verify can add again
    assert!(whitelist.remove_validator(&validator2).is_ok());
    assert!(whitelist.remove_validator(&validator3).is_ok());
    assert_eq!(whitelist.total_permissioned_validators, 0);
    assert!(whitelist.can_add_validator());

    let new_validator = Pubkey::new_unique();
    assert!(whitelist.add_validator(new_validator).is_ok());
    assert_eq!(whitelist.total_permissioned_validators, 1);
}

#[test]
fn test_directed_stake_whitelist_remove_array_shifting() {
    use jito_steward::{
        DirectedStakeWhitelist, MAX_PERMISSIONED_DIRECTED_STAKERS,
        MAX_PERMISSIONED_DIRECTED_VALIDATORS,
    };

    let mut whitelist = DirectedStakeWhitelist {
        permissioned_user_stakers: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_STAKERS],
        permissioned_protocol_stakers: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_STAKERS],
        permissioned_validators: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_VALIDATORS],
        total_permissioned_user_stakers: 0,
        total_permissioned_protocol_stakers: 0,
        total_permissioned_validators: 0,
        _padding0: [0; 250],
    };

    // Test case 1: Verify array shifting for stakers
    let stakers = [
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        Pubkey::new_unique(),
    ];

    // Add all stakers
    for staker in &stakers {
        assert!(whitelist.add_user_staker(*staker).is_ok());
    }
    assert_eq!(whitelist.total_permissioned_user_stakers, 4);

    // Remove staker at index 1 (middle)
    assert!(whitelist.remove_user_staker(&stakers[1]).is_ok());
    assert_eq!(whitelist.total_permissioned_user_stakers, 3);

    // Verify remaining stakers are in correct order
    assert!(whitelist.is_staker_permissioned(&stakers[0]));
    assert!(!whitelist.is_staker_permissioned(&stakers[1]));
    assert!(whitelist.is_staker_permissioned(&stakers[2]));
    assert!(whitelist.is_staker_permissioned(&stakers[3]));

    // Remove staker at index 0 (first)
    assert!(whitelist.remove_user_staker(&stakers[0]).is_ok());
    assert_eq!(whitelist.total_permissioned_user_stakers, 2);

    // Verify remaining stakers
    assert!(!whitelist.is_staker_permissioned(&stakers[0]));
    assert!(!whitelist.is_staker_permissioned(&stakers[1]));
    assert!(whitelist.is_staker_permissioned(&stakers[2]));
    assert!(whitelist.is_staker_permissioned(&stakers[3]));

    // Test case 2: Verify array shifting for validators
    let validators = [
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        Pubkey::new_unique(),
    ];

    // Add all validators
    for validator in &validators {
        assert!(whitelist.add_validator(*validator).is_ok());
    }
    assert_eq!(whitelist.total_permissioned_validators, 4);

    // Remove validator at index 2 (third position)
    assert!(whitelist.remove_validator(&validators[2]).is_ok());
    assert_eq!(whitelist.total_permissioned_validators, 3);

    // Verify remaining validators are in correct order
    assert!(whitelist.is_validator_permissioned(&validators[0]));
    assert!(whitelist.is_validator_permissioned(&validators[1]));
    assert!(!whitelist.is_validator_permissioned(&validators[2]));
    assert!(whitelist.is_validator_permissioned(&validators[3]));

    // Remove validator at index 3 (last)
    assert!(whitelist.remove_validator(&validators[3]).is_ok());
    assert_eq!(whitelist.total_permissioned_validators, 2);

    // Verify remaining validators
    assert!(whitelist.is_validator_permissioned(&validators[0]));
    assert!(whitelist.is_validator_permissioned(&validators[1]));
    assert!(!whitelist.is_validator_permissioned(&validators[2]));
    assert!(!whitelist.is_validator_permissioned(&validators[3]));
}

#[test]
fn test_directed_stake_whitelist_capacity_limits() {
    use jito_steward::{
        DirectedStakeWhitelist, MAX_PERMISSIONED_DIRECTED_STAKERS,
        MAX_PERMISSIONED_DIRECTED_VALIDATORS,
    };

    let mut whitelist = DirectedStakeWhitelist {
        permissioned_user_stakers: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_STAKERS],
        permissioned_protocol_stakers: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_STAKERS],
        permissioned_validators: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_VALIDATORS],
        total_permissioned_user_stakers: 0,
        total_permissioned_protocol_stakers: 0,
        total_permissioned_validators: 0,
        _padding0: [0; 250],
    };

    // Test case 1: Fill staker list to capacity
    for i in 0..MAX_PERMISSIONED_DIRECTED_STAKERS {
        let staker = Pubkey::new_unique();
        let result = whitelist.add_user_staker(staker);
        assert!(result.is_ok());
        assert_eq!(whitelist.total_permissioned_user_stakers as usize, i + 1);
    }
    // Note: can_add_staker() returns true if either user or protocol stakers can be added
    // Since we only filled user stakers, protocol stakers can still be added
    assert!(whitelist.can_add_staker());

    // Test case 2: Try to add staker when full
    let extra_staker = Pubkey::new_unique();
    let result = whitelist.add_user_staker(extra_staker);
    assert!(result.is_err());

    // Test case 3: Fill validator list to capacity
    for i in 0..MAX_PERMISSIONED_DIRECTED_VALIDATORS {
        let validator = Pubkey::new_unique();
        let result = whitelist.add_validator(validator);
        assert!(result.is_ok());
        assert_eq!(whitelist.total_permissioned_validators as usize, i + 1);
    }
    assert!(!whitelist.can_add_validator());

    // Test case 4: Try to add validator when full
    let extra_validator = Pubkey::new_unique();
    let result = whitelist.add_validator(extra_validator);
    assert!(result.is_err());
}
