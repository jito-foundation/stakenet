/*
    TODO link picture of state machine
    These tests cover all possible state transitions when calling the `transition` method on the `StewardState` struct.
*/

use jito_steward::{
    constants::MAX_VALIDATORS, Delegation, StewardStateEnum, COMPUTE_DELEGATIONS, REBALANCE,
    REBALANCE_DIRECTED_COMPLETE, RESET_TO_IDLE,
};
use tests::steward_fixtures::StateMachineFixtures;

#[test]
pub fn test_compute_scores_to_compute_delegations() {
    let mut fixtures = Box::<StateMachineFixtures>::default();

    let clock = &fixtures.clock;
    let epoch_schedule = &fixtures.epoch_schedule;
    let validators = &fixtures.validators;
    let cluster_history = &fixtures.cluster_history;
    let config = &fixtures.config;
    let parameters = &fixtures.config.parameters;
    let state = &mut fixtures.state;
    state.state_tag = StewardStateEnum::ComputeScores;
    state.set_flag(REBALANCE_DIRECTED_COMPLETE);

    for validator in validators {
        state
            .compute_score(
                clock,
                epoch_schedule,
                validator,
                validator.index as usize,
                cluster_history,
                config,
            )
            .unwrap();
        assert!(matches!(state.state_tag, StewardStateEnum::ComputeScores));
    }

    assert!(state
        .progress
        .is_complete(state.num_pool_validators)
        .unwrap());

    let res = state.transition(clock, parameters, epoch_schedule);
    assert!(res.is_ok());
    assert!(matches!(
        state.state_tag,
        StewardStateEnum::ComputeDelegations
    ));
    assert!(state.progress.is_empty());
    assert!(state.delegations == [Delegation::default(); MAX_VALIDATORS]);
}

/* TODO: Is this test applicable anymore with the new initial state machine state?
#[test]
pub fn test_compute_scores_to_new_compute_scores() {
    let mut fixtures = Box::<StateMachineFixtures>::default();

    let clock = &mut fixtures.clock;
    let epoch_schedule = &fixtures.epoch_schedule;
    let validators = &mut fixtures.validators;
    let cluster_history = &fixtures.cluster_history;
    let config = &fixtures.config;
    let parameters = &fixtures.config.parameters;
    let state = &mut fixtures.state;
    state.state_tag = StewardStateEnum::ComputeScores;
    state.set_flag(REBALANCE_DIRECTED_COMPLETE);
    clock.slot += 250_000;

    // Case 1: Make some progress but then progress halts until past next_compute_epoch
    state
        .compute_score(
            clock,
            epoch_schedule,
            &validators[0],
            validators[0].index as usize,
            cluster_history,
            config,
            state.num_pool_validators,
        )
        .unwrap();
    assert!(matches!(state.state_tag, StewardStateEnum::ComputeScores));

    let res = state.transition(clock, parameters, epoch_schedule);
    assert!(res.is_ok());
    assert!(matches!(state.state_tag, StewardStateEnum::ComputeScores));
    assert!(state.progress.is_empty());
    assert!(state.scores == [0; MAX_VALIDATORS]);

    // Case 2: Make some progress but then progress halts for 1000 slots
}*/

#[test]
pub fn test_compute_scores_noop() {
    let mut fixtures = Box::<StateMachineFixtures>::default();

    let clock = &mut fixtures.clock;
    let epoch_schedule = &fixtures.epoch_schedule;
    let parameters = &fixtures.config.parameters;
    let state = &mut fixtures.state;
    state.state_tag = StewardStateEnum::ComputeScores;
    state.set_flag(REBALANCE_DIRECTED_COMPLETE);
    clock.slot += 250_000;

    let res = state.transition(clock, parameters, epoch_schedule);
    assert!(res.is_ok());
    assert!(matches!(state.state_tag, StewardStateEnum::ComputeScores));
}

#[test]
pub fn test_rebalance_directed_noop() {
    let mut fixtures = Box::<StateMachineFixtures>::default();

    let clock = &mut fixtures.clock;
    let epoch_schedule = &fixtures.epoch_schedule;
    let parameters = &fixtures.config.parameters;
    let state = &mut fixtures.state;
    state.state_tag = StewardStateEnum::RebalanceDirected;
    clock.slot += 10_000;

    let res = state.transition(clock, parameters, epoch_schedule);
    assert!(res.is_ok());
    assert!(matches!(
        state.state_tag,
        StewardStateEnum::RebalanceDirected
    ));
}

#[test]
pub fn test_compute_delegations_to_idle() {
    let mut fixtures = Box::<StateMachineFixtures>::default();

    let current_epoch = fixtures.clock.epoch;
    let clock = &fixtures.clock;
    let epoch_schedule = &fixtures.epoch_schedule;
    let config = &fixtures.config;
    let parameters = &fixtures.config.parameters;
    let state = &mut fixtures.state;
    state.sorted_score_indices[0..3].copy_from_slice(&[0, 1, 2]);

    state.state_tag = StewardStateEnum::ComputeDelegations;
    state.compute_delegations(current_epoch, config).unwrap();

    let res = state.transition(clock, parameters, epoch_schedule);
    assert!(res.is_ok());
    assert!(matches!(state.state_tag, StewardStateEnum::Idle));
}

#[test]
pub fn test_compute_delegations_noop() {
    let mut fixtures = Box::<StateMachineFixtures>::default();

    let clock = &fixtures.clock;
    let epoch_schedule = &fixtures.epoch_schedule;
    let parameters = &fixtures.config.parameters;
    let state = &mut fixtures.state;

    state.state_tag = StewardStateEnum::ComputeDelegations;
    let res = state.transition(clock, parameters, epoch_schedule);
    assert!(res.is_ok());
    // TODO need to add some logic to make sure it stays in the same state. Failing right now
    assert!(matches!(
        state.state_tag,
        StewardStateEnum::ComputeDelegations
    ));
}

#[test]
pub fn test_idle_to_compute_instant_unstake() {
    let mut fixtures = Box::<StateMachineFixtures>::default();

    let clock = &mut fixtures.clock;
    let epoch_schedule = &fixtures.epoch_schedule;
    let parameters = &fixtures.config.parameters;
    let state = &mut fixtures.state;

    state.state_tag = StewardStateEnum::Idle;
    state.set_flag(REBALANCE_DIRECTED_COMPLETE);
    state.set_flag(COMPUTE_DELEGATIONS);
    clock.slot +=
        (epoch_schedule.slots_per_epoch as f64 * parameters.instant_unstake_epoch_progress) as u64;
    let res = state.transition(clock, parameters, epoch_schedule);
    assert!(res.is_ok());
    assert!(matches!(
        state.state_tag,
        StewardStateEnum::ComputeInstantUnstake
    ));
    assert!(state.progress.is_empty());
    assert!(state.instant_unstake.is_empty());
}

#[test]
pub fn test_idle_to_compute_scores() {
    let mut fixtures = Box::<StateMachineFixtures>::default();

    let clock = &mut fixtures.clock;
    let epoch_schedule = &fixtures.epoch_schedule;
    let parameters = &fixtures.config.parameters;
    let state = &mut fixtures.state;

    assert!(matches!(
        state.state_tag,
        StewardStateEnum::RebalanceDirected
    ));

    state.state_tag = StewardStateEnum::Idle;
    state.set_flag(REBALANCE_DIRECTED_COMPLETE);

    //clock.epoch += parameters.num_epochs_between_scoring;
    clock.slot = epoch_schedule.get_first_slot_in_epoch(clock.epoch);
    clock.slot += 250_000;
    let res = state.transition(clock, parameters, epoch_schedule);
    assert!(res.is_ok());
    println!("state.state_tag: {}", state.state_tag);
    assert!(matches!(state.state_tag, StewardStateEnum::ComputeScores));
}

#[test]
pub fn test_idle_noop() {
    let mut fixtures = Box::<StateMachineFixtures>::default();

    let clock = &mut fixtures.clock;
    let epoch_schedule = &fixtures.epoch_schedule;
    let parameters = &fixtures.config.parameters;
    let state = &mut fixtures.state;

    // Case 1: before we've hit instant_unstake_epoch_progress
    clock.slot = epoch_schedule.get_first_slot_in_epoch(clock.epoch);
    state.state_tag = StewardStateEnum::Idle;
    state.set_flag(REBALANCE_DIRECTED_COMPLETE);
    let res = state.transition(clock, parameters, epoch_schedule);
    assert!(res.is_ok());
    assert!(matches!(state.state_tag, StewardStateEnum::Idle));

    // Case 2: still after instant_unstake_epoch_progress but after rebalance is completed
    clock.slot = epoch_schedule.get_last_slot_in_epoch(clock.epoch);
    state.set_flag(COMPUTE_DELEGATIONS);
    state.set_flag(REBALANCE);
    let res = state.transition(clock, parameters, epoch_schedule);
    assert!(res.is_ok());
    assert!(matches!(state.state_tag, StewardStateEnum::Idle));
}

#[test]
pub fn test_compute_instant_unstake_to_rebalance() {
    let mut fixtures = Box::<StateMachineFixtures>::default();

    let clock = &fixtures.clock;
    let epoch_schedule = &fixtures.epoch_schedule;
    let validators = &fixtures.validators;
    let cluster_history = &fixtures.cluster_history;
    let config = &fixtures.config;
    let parameters = &fixtures.config.parameters;
    let state = &mut fixtures.state;

    state.state_tag = StewardStateEnum::ComputeInstantUnstake;
    for validator in validators {
        state
            .compute_instant_unstake(
                clock,
                epoch_schedule,
                validator,
                validator.index as usize,
                cluster_history,
                config,
            )
            .unwrap();
        assert!(matches!(
            state.state_tag,
            StewardStateEnum::ComputeInstantUnstake
        ));
    }
    state
        .compute_instant_unstake(
            clock,
            epoch_schedule,
            &validators[0],
            validators[0].index as usize,
            cluster_history,
            config,
        )
        .unwrap();
    assert!(state
        .progress
        .is_complete(state.num_pool_validators)
        .unwrap());

    let res = state.transition(clock, parameters, epoch_schedule);
    assert!(res.is_ok());
    assert!(matches!(state.state_tag, StewardStateEnum::Rebalance));
    assert!(state.progress.is_empty());
}

#[test]
pub fn test_compute_instant_unstake_to_idle() {
    let mut fixtures = Box::<StateMachineFixtures>::default();

    let clock = &mut fixtures.clock;
    let epoch_schedule = &fixtures.epoch_schedule;
    let parameters = &fixtures.config.parameters;
    let state = &mut fixtures.state;

    state.state_tag = StewardStateEnum::ComputeInstantUnstake;
    state.set_flag(RESET_TO_IDLE);

    let res = state.transition(clock, parameters, epoch_schedule);
    assert!(res.is_ok());
    assert!(matches!(state.state_tag, StewardStateEnum::Idle));
}

#[test]
pub fn test_compute_instant_unstake_transition_noop() {
    let mut fixtures = Box::<StateMachineFixtures>::default();

    let clock = &mut fixtures.clock;
    let epoch_schedule = &fixtures.epoch_schedule;
    let parameters = &fixtures.config.parameters;
    let state = &mut fixtures.state;

    state.state_tag = StewardStateEnum::ComputeInstantUnstake;
    assert!(state.progress.is_empty());
    let res = state.transition(clock, parameters, epoch_schedule);
    assert!(res.is_ok());
    assert!(matches!(
        state.state_tag,
        StewardStateEnum::ComputeInstantUnstake
    ));
    assert!(state.progress.is_empty());
}

#[test]
pub fn test_rebalance_to_idle() {
    let mut fixtures = Box::<StateMachineFixtures>::default();

    let clock = &mut fixtures.clock;
    let epoch_schedule = &fixtures.epoch_schedule;
    let parameters = &fixtures.config.parameters;
    let state = &mut fixtures.state;

    state.state_tag = StewardStateEnum::Rebalance;

    for i in 0..state.num_pool_validators as usize {
        let _ = state.progress.set(i, true);
        assert!(matches!(state.state_tag, StewardStateEnum::Rebalance));
    }

    let res = state.transition(clock, parameters, epoch_schedule);
    assert!(res.is_ok());
    assert!(matches!(state.state_tag, StewardStateEnum::Idle));

    // Test didn't finish rebalance case
    state.state_tag = StewardStateEnum::Rebalance;
    state.progress.reset();
    state.set_flag(RESET_TO_IDLE);

    let res = state.transition(clock, parameters, epoch_schedule);
    assert!(res.is_ok());
    assert!(matches!(state.state_tag, StewardStateEnum::Idle));
}

#[test]
pub fn test_directed_rebalance_to_idle() {
    let mut fixtures = Box::<StateMachineFixtures>::default();

    let clock = &mut fixtures.clock;
    let epoch_schedule = &fixtures.epoch_schedule;
    let parameters = &fixtures.config.parameters;
    let state = &mut fixtures.state;

    state.state_tag = StewardStateEnum::RebalanceDirected;
    state.set_flag(REBALANCE_DIRECTED_COMPLETE);
    clock.slot = epoch_schedule.get_first_slot_in_epoch(clock.epoch) + 100_000; // ~ 25% epoch progress

    let res = state.transition(clock, parameters, epoch_schedule);
    assert!(res.is_ok());
    println!("state.state_tag: {}", state.state_tag);
    assert!(matches!(state.state_tag, StewardStateEnum::Idle));
}

#[test]
pub fn idle_to_new_epoch_rebalance_directed() {
    let mut fixtures = Box::<StateMachineFixtures>::default();

    let clock = &mut fixtures.clock;
    let epoch_schedule = &fixtures.epoch_schedule;
    let parameters = &fixtures.config.parameters;
    let state = &mut fixtures.state;

    state.state_tag = StewardStateEnum::Idle;
    clock.slot = epoch_schedule.get_last_slot_in_epoch(clock.epoch);
    state.set_flag(REBALANCE_DIRECTED_COMPLETE);
    state.set_flag(COMPUTE_DELEGATIONS);

    let _ = state.transition(clock, parameters, epoch_schedule);

    clock.epoch += parameters.num_epochs_between_scoring;
    clock.slot = epoch_schedule.get_first_slot_in_epoch(clock.epoch);

    let res = state.transition(clock, parameters, epoch_schedule);
    assert!(res.is_ok());
    assert!(matches!(
        state.state_tag,
        StewardStateEnum::RebalanceDirected
    ));
}
