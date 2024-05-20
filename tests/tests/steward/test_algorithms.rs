// Unit tests for scoring, instant unstake, and delegation methods
use anchor_lang::AnchorSerialize;
use jito_steward::{
    constants::SORTED_INDEX_DEFAULT,
    delegation::{
        decrease_stake_calculation, increase_stake_calculation, DecreaseComponents, RebalanceType,
        UnstakeState,
    },
    errors::StewardError,
    insert_sorted_index,
    score::{
        instant_unstake_validator, validator_score, InstantUnstakeComponents, ScoreComponents,
    },
    select_validators_to_delegate, Delegation,
};
use solana_sdk::native_token::LAMPORTS_PER_SOL;
use spl_stake_pool::big_vec::BigVec;
use tests::steward_fixtures::StateMachineFixtures;
use validator_history::{ClusterHistoryEntry, ValidatorHistoryEntry};

#[test]
fn test_compute_score() {
    let default_fixture = StateMachineFixtures::default();

    let current_epoch = default_fixture.current_epoch;
    let mut config = default_fixture.config;

    let validators = default_fixture.validators;

    // 1000 blocks per epoch
    let cluster_history = default_fixture.cluster_history;

    // 1000 credits per epoch
    let good_validator = validators[0];

    // Regular run
    let components = validator_score(
        &good_validator,
        good_validator.index as usize,
        &cluster_history,
        &config,
        current_epoch as u16,
    )
    .unwrap();
    assert_eq!(
        components,
        ScoreComponents {
            score: 1.0,
            yield_score: 1.0,
            mev_commission_score: 1.0,
            blacklisted_score: 1.0,
            superminority_score: 1.0,
            delinquency_score: 1.0,
            running_jito_score: 1.0,
            vote_credits_ratio: 1.0,
            commission_score: 1.0,
            vote_account: good_validator.vote_account,
            epoch: current_epoch as u16
        }
    );

    // mev commission score
    let mut validator = good_validator;
    validator.history.last_mut().unwrap().mev_commission = 1001;

    let components = validator_score(
        &validator,
        validator.index as usize,
        &cluster_history,
        &config,
        current_epoch as u16,
    )
    .unwrap();
    assert_eq!(
        components,
        ScoreComponents {
            score: 0.0,
            yield_score: 1.0,
            mev_commission_score: 0.0,
            blacklisted_score: 1.0,
            superminority_score: 1.0,
            delinquency_score: 1.0,
            running_jito_score: 1.0,
            vote_credits_ratio: 1.0,
            commission_score: 1.0,
            vote_account: validator.vote_account,
            epoch: current_epoch as u16
        }
    );

    let mut validator = good_validator;
    validator.history.arr[11].mev_commission = 1001;
    let components = validator_score(
        &validator,
        validator.index as usize,
        &cluster_history,
        &config,
        current_epoch as u16,
    )
    .unwrap();
    assert_eq!(
        components,
        ScoreComponents {
            score: 0.0,
            yield_score: 1.0,
            mev_commission_score: 0.0,
            blacklisted_score: 1.0,
            superminority_score: 1.0,
            delinquency_score: 1.0,
            running_jito_score: 1.0,
            vote_credits_ratio: 1.0,
            commission_score: 1.0,
            vote_account: validator.vote_account,
            epoch: current_epoch as u16
        }
    );
    let mut validator = good_validator;
    validator.history.arr[9].mev_commission = 1001;
    let components = validator_score(
        &validator,
        validator.index as usize,
        &cluster_history,
        &config,
        current_epoch as u16,
    )
    .unwrap();
    assert_eq!(
        components,
        ScoreComponents {
            score: 1.0,
            yield_score: 1.0,
            mev_commission_score: 1.0,
            blacklisted_score: 1.0,
            superminority_score: 1.0,
            delinquency_score: 1.0,
            running_jito_score: 1.0,
            vote_credits_ratio: 1.0,
            commission_score: 1.0,
            vote_account: validator.vote_account,
            epoch: current_epoch as u16
        }
    );

    // blacklist
    let validator = good_validator;
    config
        .blacklist
        .set(validator.index as usize, true)
        .unwrap();
    let components = validator_score(
        &validator,
        validator.index as usize,
        &cluster_history,
        &config,
        current_epoch as u16,
    )
    .unwrap();
    assert_eq!(
        components,
        ScoreComponents {
            score: 0.0,
            yield_score: 1.0,
            mev_commission_score: 1.0,
            blacklisted_score: 0.0,
            superminority_score: 1.0,
            delinquency_score: 1.0,
            running_jito_score: 1.0,
            vote_credits_ratio: 1.0,
            commission_score: 1.0,
            vote_account: validator.vote_account,
            epoch: current_epoch as u16
        }
    );
    config.blacklist.reset();

    // superminority score
    let mut validator = good_validator;
    validator.history.last_mut().unwrap().is_superminority = 1;
    let components = validator_score(
        &validator,
        validator.index as usize,
        &cluster_history,
        &config,
        current_epoch as u16,
    )
    .unwrap();
    assert_eq!(
        components,
        ScoreComponents {
            score: 0.0,
            yield_score: 1.0,
            mev_commission_score: 1.0,
            blacklisted_score: 1.0,
            superminority_score: 0.0,
            delinquency_score: 1.0,
            running_jito_score: 1.0,
            vote_credits_ratio: 1.0,
            commission_score: 1.0,
            vote_account: validator.vote_account,
            epoch: current_epoch as u16
        }
    );

    // Every previous epoch in superminority except last
    let mut validator = good_validator;
    for i in 0..19 {
        validator.history.arr_mut()[i].is_superminority = 1;
    }
    let components = validator_score(
        &validator,
        validator.index as usize,
        &cluster_history,
        &config,
        current_epoch as u16,
    )
    .unwrap();
    assert_eq!(
        components,
        ScoreComponents {
            score: 1.0,
            yield_score: 1.0,
            mev_commission_score: 1.0,
            blacklisted_score: 1.0,
            superminority_score: 1.0,
            delinquency_score: 1.0,
            running_jito_score: 1.0,
            vote_credits_ratio: 1.0,
            commission_score: 1.0,
            vote_account: validator.vote_account,
            epoch: current_epoch as u16
        }
    );

    // running jito score
    let mut validator = good_validator;
    for i in 10..=20 {
        validator.history.arr_mut()[i].mev_commission =
            ValidatorHistoryEntry::default().mev_commission;
    }
    let components = validator_score(
        &validator,
        validator.index as usize,
        &cluster_history,
        &config,
        current_epoch as u16,
    )
    .unwrap();
    assert_eq!(
        components,
        ScoreComponents {
            score: 0.0,
            yield_score: 1.0,
            mev_commission_score: 0.0,
            blacklisted_score: 1.0,
            superminority_score: 1.0,
            delinquency_score: 1.0,
            running_jito_score: 0.0,
            vote_credits_ratio: 1.0,
            commission_score: 1.0,
            vote_account: validator.vote_account,
            epoch: current_epoch as u16
        }
    );

    // commission
    let mut validator = good_validator;
    validator.history.last_mut().unwrap().commission = 11;
    let components = validator_score(
        &validator,
        validator.index as usize,
        &cluster_history,
        &config,
        current_epoch as u16,
    )
    .unwrap();
    assert_eq!(
        components,
        ScoreComponents {
            score: 0.0,
            yield_score: 0.89,
            mev_commission_score: 1.0,
            blacklisted_score: 1.0,
            superminority_score: 1.0,
            delinquency_score: 1.0,
            running_jito_score: 1.0,
            vote_credits_ratio: 1.0,
            commission_score: 0.0,
            vote_account: validator.vote_account,
            epoch: current_epoch as u16
        }
    );

    let mut validator = good_validator;
    let mut cluster_history = default_fixture.cluster_history;

    // average vote credits + average blocks
    for i in 0..=20 {
        validator.history.arr_mut()[i].epoch_credits = 880;
        cluster_history.history.arr_mut()[i].total_blocks = 1000;
    }
    let components = validator_score(
        &validator,
        validator.index as usize,
        &cluster_history,
        &config,
        current_epoch as u16,
    )
    .unwrap();
    assert_eq!(
        components,
        ScoreComponents {
            score: 0.88,
            yield_score: 0.88,
            mev_commission_score: 1.0,
            blacklisted_score: 1.0,
            superminority_score: 1.0,
            delinquency_score: 1.0,
            running_jito_score: 1.0,
            vote_credits_ratio: 0.88,
            commission_score: 1.0,
            vote_account: validator.vote_account,
            epoch: current_epoch as u16
        }
    );

    // delinquency
    let mut validator = good_validator;
    validator.history.arr[10].epoch_credits = 0;
    let components = validator_score(
        &validator,
        validator.index as usize,
        &cluster_history,
        &config,
        current_epoch as u16,
    )
    .unwrap();
    assert_eq!(
        components,
        ScoreComponents {
            score: 0.0,
            yield_score: 0.95,
            mev_commission_score: 1.0,
            blacklisted_score: 1.0,
            superminority_score: 1.0,
            delinquency_score: 0.0,
            running_jito_score: 1.0,
            vote_credits_ratio: 0.95,
            commission_score: 1.0,
            vote_account: validator.vote_account,
            epoch: current_epoch as u16
        }
    );

    // Test cluster history not updated won't punish validators for delinquency
    let mut validator = good_validator;
    let mut cluster_history = default_fixture.cluster_history;
    validator.history.arr[10].epoch_credits = ValidatorHistoryEntry::default().epoch_credits;
    validator.history.arr[11].epoch_credits = 0;
    cluster_history.history.arr[10].total_blocks = ClusterHistoryEntry::default().total_blocks;
    cluster_history.history.arr[11].total_blocks = ClusterHistoryEntry::default().total_blocks;

    let components = validator_score(
        &validator,
        validator.index as usize,
        &cluster_history,
        &config,
        current_epoch as u16,
    )
    .unwrap();
    assert_eq!(
        components,
        ScoreComponents {
            score: 0.9,
            yield_score: 0.9,
            mev_commission_score: 1.0,
            blacklisted_score: 1.0,
            superminority_score: 1.0,
            delinquency_score: 1.0,
            running_jito_score: 1.0,
            vote_credits_ratio: 0.9,
            commission_score: 1.0,
            vote_account: validator.vote_account,
            epoch: current_epoch as u16
        }
    );

    // Test current epoch vote credits and blocks don't affect score
    let mut validator = good_validator;
    let mut cluster_history = default_fixture.cluster_history;
    assert_eq!(current_epoch, 20);
    validator.history.arr[current_epoch as usize].epoch_credits = 0;
    cluster_history.history.arr[current_epoch as usize].total_blocks = 0;
    let components = validator_score(
        &validator,
        validator.index as usize,
        &cluster_history,
        &config,
        current_epoch as u16,
    )
    .unwrap();
    assert_eq!(
        components,
        ScoreComponents {
            score: 1.0,
            yield_score: 1.0,
            mev_commission_score: 1.0,
            blacklisted_score: 1.0,
            superminority_score: 1.0,
            delinquency_score: 1.0,
            running_jito_score: 1.0,
            vote_credits_ratio: 1.0,
            commission_score: 1.0,
            vote_account: validator.vote_account,
            epoch: current_epoch as u16
        }
    );

    // Test superminority missing from current
    // conditions: no epoch credits for this epoch
    // iterate through several previous epochs until we find populated superminority
    let mut validator = good_validator;
    validator.history.arr[current_epoch as usize].epoch_credits =
        ValidatorHistoryEntry::default().epoch_credits;
    validator.history.arr[current_epoch as usize].is_superminority =
        ValidatorHistoryEntry::default().is_superminority;
    validator.history.arr[current_epoch as usize - 1].is_superminority =
        ValidatorHistoryEntry::default().is_superminority;
    validator.history.arr[current_epoch as usize - 2].is_superminority =
        ValidatorHistoryEntry::default().is_superminority;
    validator.history.arr[current_epoch as usize - 3].is_superminority = 1;
    let components = validator_score(
        &validator,
        validator.index as usize,
        &cluster_history,
        &config,
        current_epoch as u16,
    )
    .unwrap();
    assert!(components.superminority_score == 0.0);

    // Test error: superminority should exist if epoch credits exist
    let mut validator = good_validator;
    validator.history.arr[current_epoch as usize].is_superminority =
        ValidatorHistoryEntry::default().is_superminority;
    let res = validator_score(
        &validator,
        validator.index as usize,
        &cluster_history,
        &config,
        current_epoch as u16,
    );
    assert!(res == Err(StewardError::StakeHistoryNotRecentEnough.into()));
}

#[test]
fn test_instant_unstake() {
    let default_fixture = StateMachineFixtures::default();

    let epoch_schedule = default_fixture.epoch_schedule;
    let current_epoch = default_fixture.current_epoch;
    let mut config = default_fixture.config;

    let validators = default_fixture.validators;

    // 1000 blocks per epoch
    let cluster_history = default_fixture.cluster_history;

    // 200 credits per epoch
    let bad_validator = validators[1];

    // Setup state
    config
        .parameters
        .instant_unstake_delinquency_threshold_ratio = 0.25;
    let start_slot = epoch_schedule.get_first_slot_in_epoch(current_epoch);
    let current_epoch = current_epoch as u16;

    // Non-instant-unstake validator
    let good_validator = validators[0];

    let res = instant_unstake_validator(
        &good_validator,
        good_validator.index as usize,
        &cluster_history,
        &config,
        start_slot,
        current_epoch,
    );

    assert!(res.is_ok());
    assert!(
        res.unwrap()
            == InstantUnstakeComponents {
                instant_unstake: false,
                delinquency_check: false,
                commission_check: false,
                mev_commission_check: false,
                is_blacklisted: false,
                vote_account: good_validator.vote_account,
                epoch: current_epoch
            }
    );

    // Is blacklisted
    config
        .blacklist
        .set(good_validator.index as usize, true)
        .unwrap();
    let res = instant_unstake_validator(
        &good_validator,
        good_validator.index as usize,
        &cluster_history,
        &config,
        start_slot,
        current_epoch,
    );
    assert!(res.is_ok());
    assert!(
        res.unwrap()
            == InstantUnstakeComponents {
                instant_unstake: true,
                delinquency_check: false,
                commission_check: false,
                mev_commission_check: false,
                is_blacklisted: true,
                vote_account: good_validator.vote_account,
                epoch: current_epoch
            }
    );
    config.blacklist.reset();

    // Delinquency threshold + Commission
    let res = instant_unstake_validator(
        &bad_validator,
        bad_validator.index as usize,
        &cluster_history,
        &config,
        start_slot,
        current_epoch,
    );
    assert!(res.is_ok());
    assert!(
        res.unwrap()
            == InstantUnstakeComponents {
                instant_unstake: true,
                delinquency_check: true,
                commission_check: true,
                mev_commission_check: true,
                is_blacklisted: false,
                vote_account: bad_validator.vote_account,
                epoch: current_epoch
            }
    );

    // Errors
    let mut cluster_history = default_fixture.cluster_history;
    cluster_history.history.last_mut().unwrap().total_blocks =
        ClusterHistoryEntry::default().total_blocks;
    let res = instant_unstake_validator(
        &bad_validator,
        bad_validator.index as usize,
        &cluster_history,
        &config,
        start_slot,
        current_epoch,
    );
    assert!(res == Err(StewardError::ClusterHistoryNotRecentEnough.into()));

    let cluster_history = default_fixture.cluster_history;
    let mut validator = validators[0];
    validator.history.last_mut().unwrap().epoch_credits =
        ValidatorHistoryEntry::default().epoch_credits;

    let res = instant_unstake_validator(
        &validator,
        validator.index as usize,
        &cluster_history,
        &config,
        start_slot,
        current_epoch,
    );
    assert!(res == Err(StewardError::VoteHistoryNotRecentEnough.into()));

    let mut validator = validators[0];
    validator
        .history
        .last_mut()
        .unwrap()
        .vote_account_last_update_slot =
        ValidatorHistoryEntry::default().vote_account_last_update_slot;
    let res = instant_unstake_validator(
        &validator,
        validator.index as usize,
        &cluster_history,
        &config,
        start_slot,
        current_epoch,
    );
    assert!(res == Err(StewardError::VoteHistoryNotRecentEnough.into()));

    // Not sure how commission would be unset with epoch credits set but test anyway
    let mut validator = validators[0];
    validator.history.last_mut().unwrap().commission = ValidatorHistoryEntry::default().commission;
    let res = instant_unstake_validator(
        &validator,
        validator.index as usize,
        &cluster_history,
        &config,
        start_slot,
        current_epoch,
    );
    assert!(res.is_ok());
    assert!(
        res.unwrap()
            == InstantUnstakeComponents {
                instant_unstake: true,
                delinquency_check: false,
                commission_check: true,
                mev_commission_check: false,
                is_blacklisted: false,
                vote_account: validator.vote_account,
                epoch: current_epoch
            }
    );

    let mut validator = validators[0];
    validator.history.last_mut().unwrap().mev_commission =
        ValidatorHistoryEntry::default().mev_commission;
    let res = instant_unstake_validator(
        &validator,
        validator.index as usize,
        &cluster_history,
        &config,
        start_slot,
        current_epoch,
    );
    assert!(res.is_ok());
    assert!(
        res.unwrap()
            == InstantUnstakeComponents {
                instant_unstake: false,
                delinquency_check: false,
                commission_check: false,
                mev_commission_check: false,
                is_blacklisted: false,
                vote_account: validator.vote_account,
                epoch: current_epoch
            }
    );

    // Try to break it
    let mut cluster_history = default_fixture.cluster_history;
    cluster_history.history.last_mut().unwrap().total_blocks = 0;
    let res = instant_unstake_validator(
        &good_validator,
        good_validator.index as usize,
        &cluster_history,
        &config,
        start_slot,
        current_epoch,
    );
    assert!(res.is_ok());
    assert!(
        res.unwrap()
            == InstantUnstakeComponents {
                instant_unstake: false,
                delinquency_check: false,
                commission_check: false,
                mev_commission_check: false,
                is_blacklisted: false,
                vote_account: good_validator.vote_account,
                epoch: current_epoch
            }
    );
}

#[test]
fn test_insert_sorted_index() {
    let mut scores = vec![10];
    let mut sorted_indices: [u16; 5] = [SORTED_INDEX_DEFAULT; 5];
    let index = 0;
    let score = scores[index as usize];
    insert_sorted_index(&mut sorted_indices, &scores, index, score, 0).unwrap();
    assert_eq!(sorted_indices[0], index);

    scores = vec![10, 20];
    sorted_indices = [SORTED_INDEX_DEFAULT; 5];
    sorted_indices[0] = 0;
    let index = 1;
    let score = scores[index as usize];
    insert_sorted_index(&mut sorted_indices, &scores, index, score, 1).unwrap();
    assert_eq!(sorted_indices[0], index);
    assert_eq!(sorted_indices[1], 0);

    scores = vec![20, 10];
    sorted_indices = [SORTED_INDEX_DEFAULT; 5];
    sorted_indices[0] = 0;
    let index = 1;
    let score = scores[index as usize];
    insert_sorted_index(&mut sorted_indices, &scores, index, score, 1).unwrap();
    assert_eq!(sorted_indices[0], 0);
    assert_eq!(sorted_indices[1], index);

    scores = vec![30, 10, 20];
    sorted_indices = [SORTED_INDEX_DEFAULT; 5];
    sorted_indices[0] = 0;
    sorted_indices[1] = 1;
    let index = 2;
    let score = scores[index as usize];
    insert_sorted_index(&mut sorted_indices, &scores, index, score, 2).unwrap();
    assert_eq!(sorted_indices[0], 0);
    assert_eq!(sorted_indices[1], index);
    assert_eq!(sorted_indices[2], 1);

    scores = vec![30, 20, 10, 40, 25];
    sorted_indices = [3, 0, 1, 2, SORTED_INDEX_DEFAULT];
    let index = 4;
    let score = scores[index as usize];
    insert_sorted_index(&mut sorted_indices, &scores, index, score, 4).unwrap();
    assert_eq!(sorted_indices, [3, 0, 4, 1, 2]);
}

#[test]
fn test_select_validators_to_delegate() {
    let scores: [u32; 10] = [10, 0, 9, 0, 8, 0, 7, 0, 6, 0];
    let sorted_score_indices = [0, 2, 4, 6, 8, 1, 3, 5, 7, 9];

    let mut validators: Vec<u16> = select_validators_to_delegate(&scores, &sorted_score_indices, 4);
    assert!(validators == vec![0, 2, 4, 6]);

    validators = select_validators_to_delegate(&scores, &sorted_score_indices, 10);
    assert!(validators.iter().all(|x| scores[*x as usize] > 0));
    assert!(validators == vec![0, 2, 4, 6, 8]);

    validators = select_validators_to_delegate(&scores, &sorted_score_indices, 0);
    assert!(validators.is_empty());

    let scores = [0; 10];
    validators = select_validators_to_delegate(&scores, &sorted_score_indices, 10);
    assert!(validators.is_empty());
}

#[test]
fn test_increase_stake_calculation() {
    /*

    State:
    Tests:
    * Couple scenarios covering all the code paths
        [X] Top up some validators and then get to current, still have stake left over
        [X] Top up validators then get to current, running out at current
        [X] Top up all validators running out before current
        [X] Skipping over instant unstake validators
    * Test errors
        [X] Invalid State (validator doesn't deserve stake)
        [X] Index out of bounds (validator not found)

    */

    let default_fixture = StateMachineFixtures::default();

    // Scores of fixture validators = [1.0, 0.0, 0.95]

    let target_validator = default_fixture.validators[2];

    let mut state = default_fixture.state;
    state.scores[0] = 100;
    state.scores[1] = 0;
    state.scores[2] = 95;
    state.sorted_score_indices[0] = 0;
    state.sorted_score_indices[1] = 2;
    state.sorted_score_indices[2] = 1;
    state.delegations[0] = Delegation::new(1, 2);
    state.delegations[1] = Delegation::new(0, 2);
    state.delegations[2] = Delegation::new(1, 2);

    let mut validator_list = default_fixture.validator_list.clone();
    validator_list[0].active_stake_lamports = (500 * LAMPORTS_PER_SOL).into();
    let validator_list_bigvec = BigVec {
        data: &mut validator_list.try_to_vec().unwrap(),
    };

    // 500 SOL in reserve, 500 SOL on validator[0], 1000 SOL on validator[1], 1000
    // validator[0] and validator[2] have a target of 2000 SOL when 1000 SOL is added to reserve
    // All reserve SOL should go to validator[0] since its score is 1.0 > 0.95
    let result = increase_stake_calculation(
        &state,
        target_validator.index as usize,
        u64::from(validator_list[2].active_stake_lamports),
        4000 * LAMPORTS_PER_SOL,
        &validator_list_bigvec,
        1500 * LAMPORTS_PER_SOL,
        0,
        0,
    );
    assert!(result.is_ok());
    assert!(matches!(result.unwrap(), RebalanceType::None));

    // Same scenario but 2500 SOL in reserve, 2000 goes to validator[0] and 500 goes to validator[2]
    let result = increase_stake_calculation(
        &state,
        target_validator.index as usize,
        u64::from(validator_list[2].active_stake_lamports),
        5000 * LAMPORTS_PER_SOL,
        &validator_list_bigvec,
        2500 * LAMPORTS_PER_SOL,
        0,
        0,
    );
    assert!(result.is_ok());
    assert!(match result.unwrap() {
        RebalanceType::Increase(lamports) => lamports == 500 * LAMPORTS_PER_SOL,
        _ => false,
    });

    // Same scenario but targeting first validator
    let target_validator = default_fixture.validators[0];
    let result = increase_stake_calculation(
        &state,
        target_validator.index as usize,
        u64::from(validator_list[0].active_stake_lamports),
        5000 * LAMPORTS_PER_SOL,
        &validator_list_bigvec,
        2500 * LAMPORTS_PER_SOL,
        0,
        0,
    );
    assert!(result.is_ok());
    assert!(match result.unwrap() {
        RebalanceType::Increase(lamports) => lamports == 2000 * LAMPORTS_PER_SOL,
        _ => false,
    });

    // Test errors

    // Target validator over allocated on stake
    let mut validator_list = default_fixture.validator_list.clone();
    validator_list[0].active_stake_lamports = (3000 * LAMPORTS_PER_SOL).into();
    validator_list[1].active_stake_lamports = 0.into();
    validator_list[2].active_stake_lamports = 0.into();
    let validator_list_bigvec = BigVec {
        data: &mut validator_list.try_to_vec().unwrap(),
    };

    let result = increase_stake_calculation(
        &state,
        0,
        u64::from(validator_list[0].active_stake_lamports),
        4000 * LAMPORTS_PER_SOL,
        &validator_list_bigvec,
        1000 * LAMPORTS_PER_SOL,
        0,
        0,
    );
    assert!(match result {
        Err(e) => e == StewardError::InvalidState.into(),
        _ => false,
    });

    // Bad index validator error
    let result = increase_stake_calculation(
        &state,
        3,
        u64::from(validator_list[0].active_stake_lamports),
        4000 * LAMPORTS_PER_SOL,
        &validator_list_bigvec,
        1000 * LAMPORTS_PER_SOL,
        0,
        0,
    );
    assert!(match result {
        Err(e) => e == StewardError::ValidatorIndexOutOfBounds.into(),
        _ => false,
    });

    // instant unstake cases
    // validator before target validator is instant unstake
    let mut validator_list = default_fixture.validator_list.clone();
    validator_list[0].active_stake_lamports = (500 * LAMPORTS_PER_SOL).into();
    let validator_list_bigvec = BigVec {
        data: &mut validator_list.try_to_vec().unwrap(),
    };

    state.instant_unstake.set(0, true).unwrap();
    let result = increase_stake_calculation(
        &state,
        2,
        u64::from(validator_list[2].active_stake_lamports),
        4000 * LAMPORTS_PER_SOL,
        &validator_list_bigvec,
        1500 * LAMPORTS_PER_SOL,
        0,
        0,
    );
    assert!(result.is_ok());
    assert!(match result.unwrap() {
        RebalanceType::Increase(lamports) => lamports == 1000 * LAMPORTS_PER_SOL,
        _ => false,
    });

    // target validator is instant unstake
    let result = increase_stake_calculation(
        &state,
        0,
        u64::from(validator_list[0].active_stake_lamports),
        4000 * LAMPORTS_PER_SOL,
        &validator_list_bigvec,
        1500 * LAMPORTS_PER_SOL,
        0,
        0,
    );
    assert!(result.is_ok());
    assert!(matches!(result.unwrap(), RebalanceType::None));

    state.instant_unstake.reset();

    // Test skips over validators with transient
    validator_list[0].active_stake_lamports = 0.into();
    validator_list[0].transient_stake_lamports = (1000 * LAMPORTS_PER_SOL).into();
    validator_list[1].active_stake_lamports = 0.into();
    validator_list[2].active_stake_lamports = (500 * LAMPORTS_PER_SOL).into();
    let validator_list_bigvec = BigVec {
        data: &mut validator_list.try_to_vec().unwrap(),
    };
    let result = increase_stake_calculation(
        &state,
        2,
        u64::from(validator_list[2].active_stake_lamports),
        2000 * LAMPORTS_PER_SOL,
        &validator_list_bigvec,
        500 * LAMPORTS_PER_SOL,
        0,
        0,
    );
    assert!(result.is_ok());
    assert!(match result.unwrap() {
        RebalanceType::Increase(lamports) => lamports == 500 * LAMPORTS_PER_SOL,
        _ => false,
    });

    // Test don't delegate less than minimum
    validator_list[0].active_stake_lamports = (998 * LAMPORTS_PER_SOL).into();
    validator_list[0].transient_stake_lamports = 0.into();
    validator_list[1].active_stake_lamports = 0.into();
    validator_list[2].active_stake_lamports = 0.into();
    let validator_list_bigvec = BigVec {
        data: &mut validator_list.try_to_vec().unwrap(),
    };
    let minimum_delegation = 2 * LAMPORTS_PER_SOL;
    let result = increase_stake_calculation(
        &state,
        0,
        u64::from(validator_list[0].active_stake_lamports) - minimum_delegation,
        2000 * LAMPORTS_PER_SOL - (state.num_pool_validators as u64 * minimum_delegation),
        &validator_list_bigvec,
        1002 * LAMPORTS_PER_SOL,
        minimum_delegation,
        0,
    );
    assert!(result.is_ok());
    assert!(matches!(result.unwrap(), RebalanceType::None));
}

#[test]
fn test_decrease_stake_calculation() {
    /* Tests:
        [X] unstake cap hit
        [X] unstake cap partially hit
        [X] unstake cap not hit
        [X] instant unstake cap hit
        [X] instant unstake cap partially hit
        [X] instant unstake cap not hit
        [X] stake deposit cap hit
        [X] stake deposit cap partially hit
        [X] stake deposit not hit
        [X] errors
    */
    let default_fixture = StateMachineFixtures::default();

    // Yield scores of fixture validators = [1.0, 0.0, 0.95]
    // Active stake lamports: 1000 SOL on each
    // Validator list balances: 1000 SOL on each
    // Delegations: 0: 100%, 1: 0%, 2: 0%

    let mut state = default_fixture.state;
    state.yield_scores[0] = 100;
    state.yield_scores[1] = 0;
    state.yield_scores[2] = 95;
    state.sorted_yield_score_indices[0] = 0;
    state.sorted_yield_score_indices[1] = 2;
    state.sorted_yield_score_indices[2] = 1;
    state.delegations[0] = Delegation::new(1, 1);
    state.delegations[1] = Delegation::new(0, 1);
    state.delegations[2] = Delegation::new(0, 1);

    let validator_list = default_fixture.validator_list.clone();
    let validator_list_bigvec = BigVec {
        data: &mut validator_list.try_to_vec().unwrap(),
    };

    // Test: unstake cap reached before target validator
    // 1000 SOL unstaked from validator 1, cap hit
    let unstake_state = UnstakeState {
        scoring_unstake_cap: 1000 * LAMPORTS_PER_SOL,
        ..Default::default()
    };
    let result = decrease_stake_calculation(
        &state,
        2,
        unstake_state,
        3000 * LAMPORTS_PER_SOL,
        &validator_list_bigvec,
        0,
        0,
    );
    assert!(result.is_ok());
    assert!(matches!(result.unwrap(), RebalanceType::None));

    // Test: unstake cap reached on target validator
    // 1000 SOL unstaked from validator 1, cap partially reached
    // 500 SOL unstaked from validator 2
    let unstake_state = UnstakeState {
        scoring_unstake_cap: 1500 * LAMPORTS_PER_SOL,
        ..Default::default()
    };

    let result = decrease_stake_calculation(
        &state,
        2,
        unstake_state,
        3000 * LAMPORTS_PER_SOL,
        &validator_list_bigvec,
        0,
        0,
    );
    assert!(result.is_ok());
    assert!(match result.unwrap() {
        RebalanceType::Decrease(components) =>
            components
                == DecreaseComponents {
                    scoring_unstake_lamports: 500 * LAMPORTS_PER_SOL,
                    instant_unstake_lamports: 0,
                    stake_deposit_unstake_lamports: 0,
                    total_unstake_lamports: 500 * LAMPORTS_PER_SOL,
                },
        _ => false,
    });

    // Test: Unstake cap not reached on target validator
    // 1000 SOL unstaked from validator 1
    // 1000 SOL unstaked from validator 2
    // 500 SOL left in unstake_cap
    let unstake_state = UnstakeState {
        scoring_unstake_cap: 2500 * LAMPORTS_PER_SOL,
        ..Default::default()
    };

    let result = decrease_stake_calculation(
        &state,
        2,
        unstake_state,
        3000 * LAMPORTS_PER_SOL,
        &validator_list_bigvec,
        0,
        0,
    );
    assert!(result.is_ok());
    assert!(match result.unwrap() {
        RebalanceType::Decrease(components) =>
            components
                == DecreaseComponents {
                    scoring_unstake_lamports: 1000 * LAMPORTS_PER_SOL,
                    instant_unstake_lamports: 0,
                    stake_deposit_unstake_lamports: 0,
                    total_unstake_lamports: 1000 * LAMPORTS_PER_SOL,
                },
        _ => false,
    });

    // Test: Instant unstake cap reached before target validator
    // 1000 SOL instant-unstaked from validator 1, instant unstake cap reached
    // 1000 SOL scoring-unstaked from validator 2
    state.instant_unstake.set(0, true).unwrap();
    state.instant_unstake.set(1, true).unwrap();
    state.instant_unstake.set(2, true).unwrap();
    let unstake_state = UnstakeState {
        scoring_unstake_cap: 1000 * LAMPORTS_PER_SOL,
        instant_unstake_cap: 1000 * LAMPORTS_PER_SOL,
        ..Default::default()
    };

    let result = decrease_stake_calculation(
        &state,
        2,
        unstake_state,
        3000 * LAMPORTS_PER_SOL,
        &validator_list_bigvec,
        0,
        0,
    );
    assert!(result.is_ok());
    assert!(match result.unwrap() {
        RebalanceType::Decrease(components) =>
            components
                == DecreaseComponents {
                    scoring_unstake_lamports: 1000 * LAMPORTS_PER_SOL,
                    instant_unstake_lamports: 0,
                    stake_deposit_unstake_lamports: 0,
                    total_unstake_lamports: 1000 * LAMPORTS_PER_SOL,
                },
        _ => false,
    });

    // Test: Instant unstake cap reached on target validator
    // 1000 SOL instant-unstaked from validator 1
    // 500 SOL instant=unstaked on validator 2, cap hit
    // 500 SOL scoring-unstaked on validator 2
    let unstake_state = UnstakeState {
        scoring_unstake_cap: 1000 * LAMPORTS_PER_SOL,
        instant_unstake_cap: 1500 * LAMPORTS_PER_SOL,
        ..Default::default()
    };

    let result = decrease_stake_calculation(
        &state,
        2,
        unstake_state,
        3000 * LAMPORTS_PER_SOL,
        &validator_list_bigvec,
        0,
        0,
    );
    assert!(match result.unwrap() {
        RebalanceType::Decrease(components) =>
            components
                == DecreaseComponents {
                    scoring_unstake_lamports: 500 * LAMPORTS_PER_SOL,
                    instant_unstake_lamports: 500 * LAMPORTS_PER_SOL,
                    stake_deposit_unstake_lamports: 0,
                    total_unstake_lamports: 1000 * LAMPORTS_PER_SOL,
                },
        _ => false,
    });

    // Instant unstake cap not reached
    // 1000 SOL unstaked on validator 1
    // 1000 SOL unstaked on validator 2
    let unstake_state = UnstakeState {
        instant_unstake_cap: 2500 * LAMPORTS_PER_SOL,
        ..Default::default()
    };

    let result = decrease_stake_calculation(
        &state,
        2,
        unstake_state,
        3000 * LAMPORTS_PER_SOL,
        &validator_list_bigvec,
        0,
        0,
    );
    assert!(match result.unwrap() {
        RebalanceType::Decrease(components) =>
            components
                == DecreaseComponents {
                    scoring_unstake_lamports: 0,
                    instant_unstake_lamports: 1000 * LAMPORTS_PER_SOL,
                    stake_deposit_unstake_lamports: 0,
                    total_unstake_lamports: 1000 * LAMPORTS_PER_SOL,
                },
        _ => false,
    });
    state.instant_unstake.reset();

    // Stake deposit: more lamports in the account than we expected. This has priority in unstaking
    let mut validator_list = default_fixture.validator_list.clone();
    validator_list[1].active_stake_lamports = (2000 * LAMPORTS_PER_SOL).into();
    validator_list[2].active_stake_lamports = (2000 * LAMPORTS_PER_SOL).into();
    let validator_list_bigvec = BigVec {
        data: &mut validator_list.try_to_vec().unwrap(),
    };

    // Test: Stake deposit cap reached before target validator
    // 1000 SOL Stake-deposit-unstaked from validator 1, cap hit
    // 1000 SOL scoring-unstaked from validator 1, cap hit
    // None unstaked from validator 2
    let unstake_state = UnstakeState {
        stake_deposit_unstake_cap: 1000 * LAMPORTS_PER_SOL,
        scoring_unstake_cap: 1000 * LAMPORTS_PER_SOL,
        instant_unstake_cap: 1000 * LAMPORTS_PER_SOL,
        ..Default::default()
    };

    let result = decrease_stake_calculation(
        &state,
        2,
        unstake_state,
        5000 * LAMPORTS_PER_SOL,
        &validator_list_bigvec,
        0,
        0,
    );
    assert!(matches!(result.unwrap(), RebalanceType::None));

    // Test: Stake deposit cap reached on target validator
    // 1000 SOL stake-deposit-unstaked from validator 1 (2000 -> 1000, internal balance level)
    // 1000 SOL scoring-unstaked from validator 1
    // 1000 SOL stake-deposit-unstaked from validator 2 (2000 -> 1000, internal balance level)
    // 1000 SOL scoring-unstaked from validator 2
    let unstake_state = UnstakeState {
        stake_deposit_unstake_cap: 2500 * LAMPORTS_PER_SOL,
        scoring_unstake_cap: 2500 * LAMPORTS_PER_SOL,
        ..Default::default()
    };

    let result = decrease_stake_calculation(
        &state,
        2,
        unstake_state,
        5000 * LAMPORTS_PER_SOL,
        &validator_list_bigvec,
        0,
        0,
    );
    assert!(match result.unwrap() {
        RebalanceType::Decrease(components) =>
            components
                == DecreaseComponents {
                    scoring_unstake_lamports: 1000 * LAMPORTS_PER_SOL,
                    instant_unstake_lamports: 0,
                    stake_deposit_unstake_lamports: 1000 * LAMPORTS_PER_SOL,
                    total_unstake_lamports: 2000 * LAMPORTS_PER_SOL,
                },
        _ => false,
    });

    // Test: stake deposit cap not reached
    // 1000 SOL stake-deposit-unstaked from validator 1
    // 1000 SOL stake-deposit-unstaked from validator 2
    let unstake_state = UnstakeState {
        stake_deposit_unstake_cap: 2500 * LAMPORTS_PER_SOL,
        ..Default::default()
    };

    let result = decrease_stake_calculation(
        &state,
        2,
        unstake_state,
        5000 * LAMPORTS_PER_SOL,
        &validator_list_bigvec,
        0,
        0,
    );
    assert!(match result.unwrap() {
        RebalanceType::Decrease(components) =>
            components
                == DecreaseComponents {
                    scoring_unstake_lamports: 0,
                    instant_unstake_lamports: 0,
                    stake_deposit_unstake_lamports: 1000 * LAMPORTS_PER_SOL,
                    total_unstake_lamports: 1000 * LAMPORTS_PER_SOL,
                },
        _ => false,
    });

    // Test: minimum delegation and stake rent exempt from total lamports unstaked
    // Same scenario as above, just leaves minimum_delegation + stake_rent on the validator
    let unstake_state = UnstakeState {
        stake_deposit_unstake_cap: 2500 * LAMPORTS_PER_SOL,
        ..Default::default()
    };

    let result = decrease_stake_calculation(
        &state,
        2,
        unstake_state,
        5000 * LAMPORTS_PER_SOL,
        &validator_list_bigvec,
        100 * LAMPORTS_PER_SOL,
        50 * LAMPORTS_PER_SOL,
    );
    assert!(match result.unwrap() {
        RebalanceType::Decrease(components) =>
            components
                == DecreaseComponents {
                    scoring_unstake_lamports: 0,
                    instant_unstake_lamports: 0,
                    stake_deposit_unstake_lamports: 850 * LAMPORTS_PER_SOL,
                    total_unstake_lamports: 850 * LAMPORTS_PER_SOL,
                },
        _ => false,
    });

    // Test: skips unstaking from transient
    // Valiator 1: nothing instant unstaked because transient exists
    let mut validator_list = default_fixture.validator_list.clone();
    validator_list[0].transient_stake_lamports = (1000 * LAMPORTS_PER_SOL).into();
    validator_list[1].transient_stake_lamports = (1000 * LAMPORTS_PER_SOL).into();
    let validator_list_bigvec = BigVec {
        data: &mut validator_list.try_to_vec().unwrap(),
    };
    state.instant_unstake.set(0, true).unwrap();
    state.instant_unstake.set(1, true).unwrap();
    state.instant_unstake.set(2, true).unwrap();
    let unstake_state = UnstakeState {
        instant_unstake_cap: 1000 * LAMPORTS_PER_SOL,
        ..Default::default()
    };

    let result = decrease_stake_calculation(
        &state,
        2,
        unstake_state,
        5000 * LAMPORTS_PER_SOL,
        &validator_list_bigvec,
        0,
        0,
    );
    assert!(result.is_ok());
    assert!(match result.unwrap() {
        RebalanceType::Decrease(components) =>
            components
                == DecreaseComponents {
                    scoring_unstake_lamports: 0,
                    instant_unstake_lamports: 1000 * LAMPORTS_PER_SOL,
                    stake_deposit_unstake_lamports: 0,
                    total_unstake_lamports: 1000 * LAMPORTS_PER_SOL,
                },
        _ => false,
    });

    // Test unstake amount is less than minimum delegation
    let validator_list = default_fixture.validator_list.clone();
    let validator_list_bigvec = BigVec {
        data: &mut validator_list.try_to_vec().unwrap(),
    };
    // 900 SOL unstaked from first two then 50 SOL left to unstake from the third, less than minimum delegation
    let unstake_state = UnstakeState {
        instant_unstake_cap: 1850 * LAMPORTS_PER_SOL,
        ..Default::default()
    };

    let result = decrease_stake_calculation(
        &state,
        0,
        unstake_state,
        5000 * LAMPORTS_PER_SOL,
        &validator_list_bigvec,
        100 * LAMPORTS_PER_SOL,
        0,
    );
    assert!(result.is_ok());
    assert!(matches!(result.unwrap(), RebalanceType::None));

    // Test errors
    let result = decrease_stake_calculation(
        &state,
        3,
        UnstakeState::default(),
        5000 * LAMPORTS_PER_SOL,
        &validator_list_bigvec,
        0,
        0,
    );
    assert!(match result {
        Err(e) => e == StewardError::ValidatorIndexOutOfBounds.into(),
        _ => false,
    });
}
