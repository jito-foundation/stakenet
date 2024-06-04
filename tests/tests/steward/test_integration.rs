use rand::{rngs::StdRng, seq::SliceRandom, Rng, SeedableRng};

/// Basic integration test
use anchor_lang::{
    solana_program::{instruction::Instruction, pubkey::Pubkey, stake, sysvar},
    AnchorDeserialize, InstructionData, ToAccountMetas,
};
use jito_steward::{
    constants::{MAX_VALIDATORS, SORTED_INDEX_DEFAULT},
    utils::{StakePool, ValidatorList},
    Config, Delegation, StewardStateAccount, StewardStateEnum, UpdateParametersArgs,
};
use solana_program_test::*;
use solana_sdk::{
    clock::Clock, compute_budget::ComputeBudgetInstruction, epoch_schedule::EpochSchedule,
    signer::Signer, stake::state::StakeStateV2, transaction::Transaction,
};
use spl_stake_pool::{
    minimum_delegation,
    state::{AccountType, ValidatorListHeader, ValidatorStakeInfo},
};
use tests::steward_fixtures::{
    cluster_history_default, new_vote_account, serialized_cluster_history_account,
    serialized_config, serialized_stake_account, serialized_stake_pool_account,
    serialized_steward_state_account, serialized_validator_history_account,
    serialized_validator_history_config, serialized_validator_list_account,
    validator_history_default, TestFixture,
};
use validator_history::{
    ClusterHistory, ClusterHistoryEntry, Config as ValidatorHistoryConfig, ValidatorHistory,
    ValidatorHistoryEntry,
};

#[tokio::test]
async fn test_compute_delegations() {
    let fixture = TestFixture::new().await;
    let ctx = &fixture.ctx;
    fixture.initialize_stake_pool().await;
    fixture.initialize_config(None).await;
    fixture.initialize_steward_state().await;

    let clock: Clock = fixture.get_sysvar().await;

    // Basic run: test compute and memory limits with max validators

    let mut steward_config: Config = fixture
        .load_and_deserialize(&fixture.steward_config.pubkey())
        .await;

    steward_config.parameters.num_delegation_validators = MAX_VALIDATORS as u32;
    steward_config.parameters.num_epochs_between_scoring = 10;

    let mut steward_state_account: StewardStateAccount =
        fixture.load_and_deserialize(&fixture.steward_state).await;

    // sets state to compute delegations for 10,000 validators
    let mut rng = StdRng::from_seed([42; 32]);
    for i in 0..MAX_VALIDATORS {
        steward_state_account.state.scores[i] = rng.gen_range(1000, 1_000_000_000);
        steward_state_account.state.yield_scores[i] = rng.gen_range(1000, 1_000_000_000);
    }

    let mut score_vec = steward_state_account
        .state
        .scores
        .iter()
        .enumerate()
        .collect::<Vec<_>>();
    let mut yield_score_vec = steward_state_account
        .state
        .yield_scores
        .iter()
        .enumerate()
        .collect::<Vec<_>>();
    score_vec.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap());
    yield_score_vec.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap());

    for i in 0..MAX_VALIDATORS {
        steward_state_account.state.sorted_score_indices[i] = score_vec[i].0 as u16;
        steward_state_account.state.sorted_yield_score_indices[i] = yield_score_vec[i].0 as u16;
    }

    steward_state_account.state.num_pool_validators = MAX_VALIDATORS;
    steward_state_account.state.state_tag =
        jito_steward::state::StewardStateEnum::ComputeDelegations;
    steward_state_account.state.current_epoch = clock.epoch;
    steward_state_account.state.next_cycle_epoch =
        clock.epoch + steward_config.parameters.num_epochs_between_scoring;

    fixture.ctx.borrow_mut().set_account(
        &fixture.steward_state,
        &serialized_steward_state_account(steward_state_account).into(),
    );
    fixture.ctx.borrow_mut().set_account(
        &fixture.steward_config.pubkey(),
        &serialized_config(steward_config).into(),
    );

    let compute_delegations_ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::ComputeDelegations {
            config: fixture.steward_config.pubkey(),
            state_account: fixture.steward_state,
            signer: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::ComputeDelegations {}.data(),
    };
    let tx = Transaction::new_signed_with_payer(
        &[
            ComputeBudgetInstruction::set_compute_unit_limit(500_000),
            compute_delegations_ix,
        ],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        ctx.borrow().last_blockhash,
    );
    fixture.submit_transaction_assert_success(tx).await;

    let steward_state_account: StewardStateAccount =
        fixture.load_and_deserialize(&fixture.steward_state).await;

    assert!(steward_state_account.state.delegations.iter().all(|&x| x
        == Delegation {
            numerator: 1,
            denominator: MAX_VALIDATORS as u32
        }));

    assert!(matches!(
        steward_state_account.state.state_tag,
        StewardStateEnum::Idle
    ));

    // Test pause
    let pause_ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::PauseSteward {
            config: fixture.steward_config.pubkey(),
            authority: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::PauseSteward {}.data(),
    };
    let compute_delegations_ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::ComputeDelegations {
            config: fixture.steward_config.pubkey(),
            state_account: fixture.steward_state,
            signer: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::ComputeDelegations {}.data(),
    };
    let tx = Transaction::new_signed_with_payer(
        &[
            pause_ix,
            ComputeBudgetInstruction::set_compute_unit_limit(500_000),
            compute_delegations_ix,
        ],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        ctx.borrow().last_blockhash,
    );
    fixture
        .submit_transaction_assert_error(tx, "StateMachinePaused")
        .await;

    drop(fixture);
}

#[tokio::test]
async fn test_compute_scores() {
    let fixture = TestFixture::new().await;
    let ctx = &fixture.ctx;
    fixture.initialize_stake_pool().await;
    fixture.initialize_config(None).await;
    fixture.initialize_steward_state().await;

    let epoch_credits = vec![(0, 1, 0), (1, 2, 1), (2, 3, 2), (3, 4, 3), (4, 5, 4)];
    let vote_account = Pubkey::new_unique();
    let validator_history_account = Pubkey::find_program_address(
        &[ValidatorHistory::SEED, vote_account.as_ref()],
        &validator_history::id(),
    )
    .0;

    let clock: Clock = fixture.get_sysvar().await;
    fixture.advance_num_epochs(512 - clock.epoch, 0).await;
    let epoch_schedule: EpochSchedule = fixture.get_sysvar().await;
    fixture.ctx.borrow_mut().set_account(
        &vote_account,
        &new_vote_account(Pubkey::new_unique(), vote_account, 1, Some(epoch_credits)).into(),
    );

    let mut validator_history = validator_history_default(vote_account, 0);
    for i in 0..=512 {
        validator_history.history.push(ValidatorHistoryEntry {
            epoch: i,
            epoch_credits: 1000,
            commission: 0,
            mev_commission: 0,
            is_superminority: 0,
            vote_account_last_update_slot: epoch_schedule.get_last_slot_in_epoch(i.into()),
            ..ValidatorHistoryEntry::default()
        });
    }

    // Setup ClusterHistory
    let cluster_history_account =
        Pubkey::find_program_address(&[ClusterHistory::SEED], &validator_history::id()).0;
    let mut cluster_history = cluster_history_default();
    cluster_history.cluster_history_last_update_slot = epoch_schedule.get_last_slot_in_epoch(512);
    for i in 0..=512 {
        cluster_history.history.push(ClusterHistoryEntry {
            epoch: i,
            total_blocks: 1000,
            ..ClusterHistoryEntry::default()
        });
    }

    // Basic run: test compute and memory limits with max validators

    let mut steward_config: Config = fixture
        .load_and_deserialize(&fixture.steward_config.pubkey())
        .await;

    steward_config.parameters.num_delegation_validators = MAX_VALIDATORS as u32;
    steward_config.parameters.num_epochs_between_scoring = 10;
    steward_config.parameters.epoch_credits_range = 511;
    steward_config.parameters.mev_commission_range = 511;
    steward_config.parameters.commission_range = 511;
    steward_config
        .parameters
        .scoring_delinquency_threshold_ratio = 0.1;
    steward_config.parameters.commission_threshold = 10;
    steward_config.parameters.mev_commission_bps_threshold = 1000;

    let mut steward_state_account: StewardStateAccount =
        fixture.load_and_deserialize(&fixture.steward_state).await;

    // Basic state setup
    steward_state_account.state.num_pool_validators = MAX_VALIDATORS;
    steward_state_account.state.state_tag = jito_steward::state::StewardStateEnum::ComputeScores;
    steward_state_account.state.current_epoch = clock.epoch;
    steward_state_account.state.next_cycle_epoch =
        clock.epoch + steward_config.parameters.num_epochs_between_scoring;

    // Setup validator list
    let mut validator_list_validators = (0..MAX_VALIDATORS)
        .map(|_| ValidatorStakeInfo {
            vote_account_address: Pubkey::new_unique(),
            ..ValidatorStakeInfo::default()
        })
        .collect::<Vec<_>>();
    validator_list_validators[0].vote_account_address = vote_account;
    let validator_list = spl_stake_pool::state::ValidatorList {
        header: ValidatorListHeader {
            account_type: AccountType::ValidatorList,
            max_validators: MAX_VALIDATORS as u32,
        },
        validators: validator_list_validators,
    };

    fixture.ctx.borrow_mut().set_account(
        &fixture.stake_pool_meta.validator_list,
        &serialized_validator_list_account(validator_list, None).into(),
    );

    fixture.ctx.borrow_mut().set_account(
        &validator_history_account,
        &serialized_validator_history_account(validator_history).into(),
    );
    fixture.ctx.borrow_mut().set_account(
        &cluster_history_account,
        &serialized_cluster_history_account(cluster_history).into(),
    );
    fixture.ctx.borrow_mut().set_account(
        &fixture.steward_state,
        &serialized_steward_state_account(steward_state_account).into(),
    );
    fixture.ctx.borrow_mut().set_account(
        &fixture.steward_config.pubkey(),
        &serialized_config(steward_config).into(),
    );

    // Basic test - test score computation that requires most compute
    let compute_scores_ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::ComputeScore {
            config: fixture.steward_config.pubkey(),
            state_account: fixture.steward_state,
            validator_history: validator_history_account,
            validator_list: fixture.stake_pool_meta.validator_list,
            cluster_history: cluster_history_account,
            signer: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::ComputeScore {
            validator_list_index: validator_history.index as usize,
        }
        .data(),
    };

    let tx = Transaction::new_signed_with_payer(
        &[
            // Only high because we are averaging 512 epochs
            ComputeBudgetInstruction::set_compute_unit_limit(600_000),
            ComputeBudgetInstruction::request_heap_frame(128 * 1024),
            compute_scores_ix.clone(),
        ],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        ctx.borrow().last_blockhash,
    );

    fixture.submit_transaction_assert_success(tx).await;

    let mut steward_state_account: StewardStateAccount =
        fixture.load_and_deserialize(&fixture.steward_state).await;

    assert!(matches!(
        steward_state_account.state.state_tag,
        StewardStateEnum::ComputeScores
    ));
    assert!(steward_state_account.state.scores[0] == 1_000_000_000);
    assert!(steward_state_account.state.yield_scores[0] == 1_000_000_000);
    assert!(steward_state_account.state.sorted_score_indices[0] == 0);
    assert!(steward_state_account.state.sorted_yield_score_indices[0] == 0);
    assert!(steward_state_account.state.progress.get(0).unwrap());
    assert!(!steward_state_account.state.progress.get(1).unwrap());

    // Transition out of this state
    // Reset current state, set progress[1] to true, progress[0] to false
    steward_state_account.state.num_pool_validators = 2;
    steward_state_account.state.scores[..2].copy_from_slice(&[0, 0]);
    steward_state_account.state.yield_scores[..2].copy_from_slice(&[0, 0]);
    steward_state_account.state.sorted_score_indices[..2]
        .copy_from_slice(&[1, SORTED_INDEX_DEFAULT]);
    steward_state_account.state.sorted_yield_score_indices[..2]
        .copy_from_slice(&[1, SORTED_INDEX_DEFAULT]);
    steward_state_account.state.progress.set(0, false).unwrap();
    steward_state_account.state.progress.set(1, true).unwrap();

    fixture.ctx.borrow_mut().set_account(
        &fixture.steward_state,
        &serialized_steward_state_account(steward_state_account).into(),
    );

    let blockhash = fixture.get_latest_blockhash().await;

    let tx = Transaction::new_signed_with_payer(
        &[
            ComputeBudgetInstruction::set_compute_unit_limit(600_000),
            ComputeBudgetInstruction::request_heap_frame(128 * 1024),
            compute_scores_ix.clone(),
        ],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        blockhash,
    );
    fixture.submit_transaction_assert_success(tx).await;

    steward_state_account = fixture.load_and_deserialize(&fixture.steward_state).await;

    assert!(matches!(
        steward_state_account.state.state_tag,
        StewardStateEnum::ComputeDelegations
    ));
    assert!(steward_state_account.state.progress.is_empty());

    // Test transitions _into_ this epoch when next_compute_epoch comes back around
    steward_state_account.state.state_tag = StewardStateEnum::ComputeDelegations;
    steward_state_account.state.next_cycle_epoch = steward_state_account.state.current_epoch;
    fixture.ctx.borrow_mut().set_account(
        &fixture.steward_state,
        &serialized_steward_state_account(steward_state_account).into(),
    );

    let blockhash = fixture.get_latest_blockhash().await;
    let tx = Transaction::new_signed_with_payer(
        &[
            ComputeBudgetInstruction::set_compute_unit_limit(600_000),
            ComputeBudgetInstruction::request_heap_frame(128 * 1024),
            compute_scores_ix.clone(),
        ],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        blockhash,
    );
    fixture.submit_transaction_assert_success(tx).await;
    steward_state_account = fixture.load_and_deserialize(&fixture.steward_state).await;
    assert!(matches!(
        steward_state_account.state.state_tag,
        StewardStateEnum::ComputeScores
    ));

    // Test pause
    let pause_ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::PauseSteward {
            config: fixture.steward_config.pubkey(),
            authority: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::PauseSteward {}.data(),
    };
    let tx = Transaction::new_signed_with_payer(
        &[pause_ix, compute_scores_ix],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        blockhash,
    );
    fixture
        .submit_transaction_assert_error(tx, "StateMachinePaused")
        .await;

    drop(fixture);
}

#[tokio::test]
async fn test_compute_instant_unstake() {
    let fixture = TestFixture::new().await;
    let ctx = &fixture.ctx;
    fixture.initialize_stake_pool().await;
    fixture
        .initialize_config(Some(UpdateParametersArgs {
            mev_commission_range: Some(0), // Set to pass validation, where epochs starts at 0
            epoch_credits_range: Some(0),  // Set to pass validation, where epochs starts at 0
            commission_range: Some(0),     // Set to pass validation, where epochs starts at 0
            scoring_delinquency_threshold_ratio: Some(0.85),
            instant_unstake_delinquency_threshold_ratio: Some(0.70),
            mev_commission_bps_threshold: Some(1000),
            commission_threshold: Some(5),
            historical_commission_threshold: Some(50),
            num_delegation_validators: Some(200),
            scoring_unstake_cap_bps: Some(750),
            instant_unstake_cap_bps: Some(10),
            stake_deposit_unstake_cap_bps: Some(10),
            instant_unstake_epoch_progress: Some(0.0), // So that we don't have to increase the slots
            compute_score_slot_range: Some(1000),
            instant_unstake_inputs_epoch_progress: Some(0.50),
            num_epochs_between_scoring: Some(10),
            minimum_stake_lamports: Some(5_000_000_000),
            minimum_voting_epochs: Some(0), // Set to pass validation, where epochs starts at 0
        }))
        .await;
    fixture.initialize_steward_state().await;

    let epoch_credits = vec![(0, 1, 0), (1, 2, 1), (2, 3, 2), (3, 4, 3), (4, 5, 4)];
    let vote_account = Pubkey::new_unique();
    let validator_history_account = Pubkey::find_program_address(
        &[ValidatorHistory::SEED, vote_account.as_ref()],
        &validator_history::id(),
    )
    .0;

    let clock: Clock = fixture.get_sysvar().await;
    let epoch_schedule: EpochSchedule = fixture.get_sysvar().await;

    fixture.ctx.borrow_mut().set_account(
        &vote_account,
        &new_vote_account(Pubkey::new_unique(), vote_account, 100, Some(epoch_credits)).into(),
    );

    let mut validator_history_config: ValidatorHistoryConfig = fixture
        .load_and_deserialize(&fixture.validator_history_config)
        .await;

    validator_history_config.counter = 2;

    let mut validator_history = validator_history_default(vote_account, 0);
    validator_history.history.push(ValidatorHistoryEntry {
        epoch: clock.epoch as u16,
        epoch_credits: 1000,
        commission: 100, // This is the condition causing instant unstake
        mev_commission: 0,
        is_superminority: 0,
        vote_account_last_update_slot: epoch_schedule.get_last_slot_in_epoch(clock.epoch),
        ..ValidatorHistoryEntry::default()
    });

    // Setup ClusterHistory
    let cluster_history_account =
        Pubkey::find_program_address(&[ClusterHistory::SEED], &validator_history::id()).0;
    let mut cluster_history = cluster_history_default();
    cluster_history.cluster_history_last_update_slot =
        epoch_schedule.get_last_slot_in_epoch(clock.epoch);
    cluster_history.history.push(ClusterHistoryEntry {
        epoch: clock.epoch as u16,
        total_blocks: 1000,
        ..ClusterHistoryEntry::default()
    });

    // Test basic run - first epoch
    // want to produce an instant unstake condition

    let mut steward_config: Config = fixture
        .load_and_deserialize(&fixture.steward_config.pubkey())
        .await;

    steward_config.parameters.num_delegation_validators = 2;
    steward_config.parameters.num_epochs_between_scoring = 10;
    steward_config.parameters.epoch_credits_range = 512;
    steward_config.parameters.mev_commission_range = 512;
    steward_config.parameters.commission_range = 512;
    steward_config
        .parameters
        .scoring_delinquency_threshold_ratio = 0.1;
    steward_config.parameters.commission_threshold = 10;
    steward_config.parameters.mev_commission_bps_threshold = 1000;

    let mut steward_state_account: StewardStateAccount =
        fixture.load_and_deserialize(&fixture.steward_state).await;

    // Basic state setup
    steward_state_account.state.num_pool_validators = 2;
    steward_state_account.state.state_tag =
        jito_steward::state::StewardStateEnum::ComputeInstantUnstake;
    steward_state_account.state.current_epoch = clock.epoch;
    steward_state_account.state.next_cycle_epoch =
        clock.epoch + steward_config.parameters.num_epochs_between_scoring;
    steward_state_account.state.delegations[0] = Delegation {
        numerator: 1,
        denominator: 2,
    };
    steward_state_account.state.delegations[1] = Delegation {
        numerator: 1,
        denominator: 2,
    };

    // Setup validator list
    let validator_list = spl_stake_pool::state::ValidatorList {
        header: ValidatorListHeader {
            account_type: AccountType::ValidatorList,
            max_validators: MAX_VALIDATORS as u32,
        },
        validators: vec![ValidatorStakeInfo {
            vote_account_address: vote_account,
            ..ValidatorStakeInfo::default()
        }],
    };

    fixture.ctx.borrow_mut().set_account(
        &fixture.stake_pool_meta.validator_list,
        &serialized_validator_list_account(validator_list, None).into(),
    );

    fixture.ctx.borrow_mut().set_account(
        &fixture.validator_history_config,
        &serialized_validator_history_config(validator_history_config).into(),
    );
    fixture.ctx.borrow_mut().set_account(
        &validator_history_account,
        &serialized_validator_history_account(validator_history).into(),
    );
    fixture.ctx.borrow_mut().set_account(
        &cluster_history_account,
        &serialized_cluster_history_account(cluster_history).into(),
    );
    fixture.ctx.borrow_mut().set_account(
        &fixture.steward_state,
        &serialized_steward_state_account(steward_state_account).into(),
    );
    fixture.ctx.borrow_mut().set_account(
        &fixture.steward_config.pubkey(),
        &serialized_config(steward_config).into(),
    );

    let compute_instant_unstake_ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::ComputeInstantUnstake {
            config: fixture.steward_config.pubkey(),
            state_account: fixture.steward_state,
            validator_history: validator_history_account,
            validator_list: fixture.stake_pool_meta.validator_list,
            cluster_history: cluster_history_account,
            signer: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::ComputeInstantUnstake {
            validator_list_index: validator_history.index as usize,
        }
        .data(),
    };

    let tx = Transaction::new_signed_with_payer(
        &[compute_instant_unstake_ix.clone()],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        ctx.borrow().last_blockhash,
    );

    fixture.submit_transaction_assert_success(tx).await;
    steward_state_account = fixture.load_and_deserialize(&fixture.steward_state).await;
    assert!(matches!(
        steward_state_account.state.state_tag,
        StewardStateEnum::ComputeInstantUnstake
    ));
    assert!(steward_state_account.state.progress.get(0).unwrap());
    assert!(steward_state_account.state.instant_unstake.get(0).unwrap());

    // Test pause
    let pause_ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::PauseSteward {
            config: fixture.steward_config.pubkey(),
            authority: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::PauseSteward {}.data(),
    };
    let blockhash = fixture.get_latest_blockhash().await;
    let tx = Transaction::new_signed_with_payer(
        &[pause_ix, compute_instant_unstake_ix.clone()],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        blockhash,
    );
    fixture
        .submit_transaction_assert_error(tx, "StateMachinePaused")
        .await;

    // Test transitions out
    steward_state_account = fixture.load_and_deserialize(&fixture.steward_state).await;
    steward_state_account.state.instant_unstake.reset();
    steward_state_account.state.progress.reset();
    steward_state_account.state.progress.set(1, true).unwrap();

    fixture.ctx.borrow_mut().set_account(
        &fixture.steward_state,
        &serialized_steward_state_account(steward_state_account).into(),
    );

    let blockhash = fixture.get_latest_blockhash().await;
    let tx = Transaction::new_signed_with_payer(
        &[compute_instant_unstake_ix.clone()],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        blockhash,
    );

    fixture.submit_transaction_assert_success(tx).await;

    steward_state_account = fixture.load_and_deserialize(&fixture.steward_state).await;
    assert!(matches!(
        steward_state_account.state.state_tag,
        StewardStateEnum::Rebalance
    ));
    assert!(steward_state_account.state.progress.is_empty());
    assert!(steward_state_account.state.instant_unstake.get(0).unwrap());

    drop(fixture);
}

#[tokio::test]
async fn test_idle() {
    let fixture = TestFixture::new().await;
    let ctx = &fixture.ctx;
    fixture.initialize_stake_pool().await;
    fixture.initialize_config(None).await;
    fixture.initialize_steward_state().await;

    let clock: Clock = fixture.get_sysvar().await;
    let epoch_schedule: EpochSchedule = fixture.get_sysvar().await;
    fixture
        .advance_num_epochs(epoch_schedule.first_normal_epoch - clock.epoch, 0)
        .await;
    let mut steward_config: Config = fixture
        .load_and_deserialize(&fixture.steward_config.pubkey())
        .await;
    let mut steward_state_account: StewardStateAccount =
        fixture.load_and_deserialize(&fixture.steward_state).await;

    steward_config.parameters.num_delegation_validators = MAX_VALIDATORS as u32;
    steward_config.parameters.instant_unstake_epoch_progress = 0.9;
    steward_state_account.state.state_tag = StewardStateEnum::Idle;
    steward_state_account.state.next_cycle_epoch = epoch_schedule.first_normal_epoch + 10;
    steward_state_account.state.current_epoch = epoch_schedule.first_normal_epoch;
    steward_state_account.state.num_pool_validators = MAX_VALIDATORS;

    ctx.borrow_mut().set_account(
        &fixture.steward_state,
        &serialized_steward_state_account(steward_state_account).into(),
    );
    ctx.borrow_mut().set_account(
        &fixture.steward_config.pubkey(),
        &serialized_config(steward_config).into(),
    );

    // Basic test - nothing happens
    let idle_ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::Idle {
            config: fixture.steward_config.pubkey(),
            state_account: fixture.steward_state,
            signer: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::Idle {}.data(),
    };
    let tx = Transaction::new_signed_with_payer(
        &[idle_ix.clone()],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        ctx.borrow().last_blockhash,
    );

    fixture.submit_transaction_assert_success(tx).await;

    let mut steward_state_account: StewardStateAccount =
        fixture.load_and_deserialize(&fixture.steward_state).await;
    assert!(matches!(
        steward_state_account.state.state_tag,
        StewardStateEnum::Idle
    ));

    // Test pause
    let pause_ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::PauseSteward {
            config: fixture.steward_config.pubkey(),
            authority: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::PauseSteward {}.data(),
    };
    let blockhash = fixture.get_latest_blockhash().await;
    let tx = Transaction::new_signed_with_payer(
        &[pause_ix, idle_ix.clone()],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        blockhash,
    );

    fixture
        .submit_transaction_assert_error(tx, "StateMachinePaused")
        .await;

    // Test wrong state
    steward_state_account.state.state_tag = StewardStateEnum::ComputeScores;
    ctx.borrow_mut().set_account(
        &fixture.steward_state,
        &serialized_steward_state_account(steward_state_account).into(),
    );
    let blockhash = fixture.get_latest_blockhash().await;
    let tx = Transaction::new_signed_with_payer(
        &[idle_ix.clone()],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        blockhash,
    );
    fixture
        .submit_transaction_assert_error(tx, "InvalidState")
        .await;

    // Wait till past instant_unstake_epoch_progress mark
    /*
    // Warp to slot causing hash mismatch error due to accounts being injected into runtime, add this in when resolved
       steward_state_account.state.state_tag = StewardStateEnum::Idle;
       ctx.borrow_mut()
           .warp_to_slot(epoch_schedule.get_last_slot_in_epoch(epoch_schedule.first_normal_epoch))
           .unwrap();
       ctx.borrow_mut().set_sysvar(&clock);
       ctx.borrow_mut().set_account(
           &fixture.steward_state,
           &serialized_steward_state_account(steward_state_account).into(),
       );
       let blockhash = ctx.borrow_mut().get_new_latest_blockhash().await.unwrap();
       let tx = Transaction::new_signed_with_payer(
           &[idle_ix.clone()],
           Some(&fixture.keypair.pubkey()),
           &[&fixture.keypair],
           blockhash,
       );
       fixture.submit_transaction_assert_success(tx).await;
       steward_state_account = fixture.load_and_deserialize(&fixture.steward_state).await;
       assert!(matches!(
           steward_state_account.state.state_tag,
           StewardStateEnum::ComputeInstantUnstake
       ));
    */

    drop(fixture);
}

#[tokio::test]
async fn test_rebalance_increase() {
    let fixture = TestFixture::new().await;
    let ctx = &fixture.ctx;
    let clock: Clock = fixture.get_sysvar().await;
    let epoch_schedule: EpochSchedule = fixture.get_sysvar().await;
    fixture
        .advance_num_epochs(epoch_schedule.first_normal_epoch - clock.epoch, 10)
        .await;
    fixture.initialize_stake_pool().await;
    fixture.initialize_config(None).await;
    fixture.initialize_steward_state().await;

    let mut steward_config: Config = fixture
        .load_and_deserialize(&fixture.steward_config.pubkey())
        .await;
    let mut steward_state_account: StewardStateAccount =
        fixture.load_and_deserialize(&fixture.steward_state).await;
    steward_config.parameters.scoring_unstake_cap_bps = 0;
    steward_config.parameters.instant_unstake_cap_bps = 0;
    steward_config.parameters.stake_deposit_unstake_cap_bps = 0;
    steward_config.parameters.minimum_voting_epochs = 1;
    steward_state_account.state.state_tag = StewardStateEnum::Rebalance;
    steward_state_account.state.num_pool_validators = MAX_VALIDATORS - 1;
    steward_state_account.state.next_cycle_epoch = epoch_schedule.first_normal_epoch + 10;
    steward_state_account.state.current_epoch = epoch_schedule.first_normal_epoch;

    let mut rng = StdRng::from_seed([42; 32]);
    let mut arr: Vec<u16> = (0..MAX_VALIDATORS as u16).collect();
    arr.shuffle(&mut rng);

    // Ensure that the validator with validator_list_index MAX_VALIDATORS - 1 is the last element in the scores array
    // This guarantees we will iterate through all scores as well as all validators in the CPI, for max compute
    let last_validator_index = arr
        .iter()
        .position(|&x| x == MAX_VALIDATORS as u16 - 1)
        .unwrap();

    arr.swap(last_validator_index, MAX_VALIDATORS - 1);

    steward_state_account
        .state
        .sorted_score_indices
        .copy_from_slice(&arr);

    steward_state_account
        .state
        .sorted_yield_score_indices
        .copy_from_slice(&arr);

    // Unrealistic scenario to ensure target validator gets delegation
    for i in 0..MAX_VALIDATORS {
        steward_state_account.state.delegations[i] = Delegation {
            numerator: 0,
            denominator: 1,
        };
    }
    steward_state_account.state.delegations[MAX_VALIDATORS - 1] = Delegation {
        numerator: 1,
        denominator: 1,
    };

    ctx.borrow_mut().set_account(
        &fixture.steward_state,
        &serialized_steward_state_account(steward_state_account).into(),
    );

    let vote_account = Pubkey::new_unique();
    let validator_history_address =
        fixture.initialize_validator_history_with_credits(vote_account, 0);

    let validator_list_account_info = fixture
        .get_account(&fixture.stake_pool_meta.validator_list)
        .await;

    let validator_list: ValidatorList = fixture
        .load_and_deserialize(&fixture.stake_pool_meta.validator_list)
        .await;

    let mut spl_validator_list = validator_list.as_ref().clone();

    let stake_program_minimum = fixture.fetch_minimum_delegation().await;
    let pool_minimum_delegation = minimum_delegation(stake_program_minimum);
    let stake_rent = fixture.fetch_stake_rent().await;
    let minimum_active_stake_with_rent = pool_minimum_delegation + stake_rent;

    // Adds all validators except the last one, to be added in auto_add_validator_to_pool.
    // That instruction also creates the stake account for the validator
    for _ in 0..MAX_VALIDATORS - 1 {
        spl_validator_list.validators.push(ValidatorStakeInfo {
            active_stake_lamports: minimum_active_stake_with_rent.into(),
            vote_account_address: Pubkey::new_unique(),
            ..ValidatorStakeInfo::default()
        });
    }

    ctx.borrow_mut().set_account(
        &fixture.stake_pool_meta.validator_list,
        &serialized_validator_list_account(
            spl_validator_list.clone(),
            Some(validator_list_account_info.data.len()),
        )
        .into(),
    );

    let stake_pool: StakePool = fixture
        .load_and_deserialize(&fixture.stake_pool_meta.stake_pool)
        .await;

    // Added in a bunch of validators with SOL, need to update balances accordingly
    let mut stake_pool_spl = stake_pool.as_ref().clone();
    stake_pool_spl.pool_token_supply +=
        (MAX_VALIDATORS as u64 - 1) * minimum_active_stake_with_rent;
    stake_pool_spl.total_lamports += (MAX_VALIDATORS as u64 - 1) * minimum_active_stake_with_rent;

    ctx.borrow_mut().set_account(
        &fixture.stake_pool_meta.stake_pool,
        &serialized_stake_pool_account(stake_pool_spl, std::mem::size_of::<StakePool>()).into(),
    );

    let mut steward_state_account: StewardStateAccount =
        fixture.load_and_deserialize(&fixture.steward_state).await;

    steward_state_account.state.num_pool_validators += 1;
    ctx.borrow_mut().set_account(
        &fixture.steward_state,
        &serialized_steward_state_account(steward_state_account).into(),
    );

    let (stake_account_address, transient_stake_account_address, withdraw_authority) =
        fixture.stake_accounts_for_validator(vote_account).await;

    let add_validator_to_pool_ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::AutoAddValidator {
            validator_history_account: validator_history_address,
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
            stake_program: stake::program::id(),
            system_program: solana_program::system_program::id(),
            signer: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::AutoAddValidatorToPool {}.data(),
    };
    let ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::Rebalance {
            config: fixture.steward_config.pubkey(),
            state_account: fixture.steward_state,
            validator_history: validator_history_address,
            validator_list: fixture.stake_pool_meta.validator_list,
            stake_pool: fixture.stake_pool_meta.stake_pool,
            reserve_stake: fixture.stake_pool_meta.reserve,
            stake_pool_program: spl_stake_pool::id(),
            staker: fixture.staker,
            withdraw_authority,
            vote_account,
            stake_account: stake_account_address,
            transient_stake_account: transient_stake_account_address,
            clock: sysvar::clock::id(),
            rent: sysvar::rent::id(),
            system_program: solana_program::system_program::id(),
            stake_program: stake::program::id(),
            stake_config: stake::config::ID,
            stake_history: solana_program::sysvar::stake_history::id(),
            signer: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::Rebalance {
            validator_list_index: MAX_VALIDATORS - 1,
        }
        .data(),
    };

    let tx = Transaction::new_signed_with_payer(
        &[
            ComputeBudgetInstruction::request_heap_frame(256 * 1024),
            ComputeBudgetInstruction::set_compute_unit_limit(1_400_000),
            add_validator_to_pool_ix,
        ],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        ctx.borrow().last_blockhash,
    );

    fixture.submit_transaction_assert_success(tx).await;

    let reserve_before_rebalance = fixture.get_account(&fixture.stake_pool_meta.reserve).await;

    let tx = Transaction::new_signed_with_payer(
        &[
            ComputeBudgetInstruction::request_heap_frame(256 * 1024),
            ComputeBudgetInstruction::set_compute_unit_limit(1_400_000),
            ix,
        ],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        ctx.borrow().last_blockhash,
    );

    fixture.submit_transaction_assert_success(tx).await;

    let stake_account_data = fixture.get_account(&stake_account_address).await;
    let stake_account: StakeStateV2 =
        StakeStateV2::deserialize(&mut stake_account_data.data.as_slice()).unwrap();

    let transient_stake_account_data = fixture.get_account(&transient_stake_account_address).await;

    let transient_stake_account =
        StakeStateV2::deserialize(&mut transient_stake_account_data.data.as_slice()).unwrap();

    // No increase yet, transient warming up
    assert_eq!(
        stake_account.stake().unwrap().delegation.stake,
        pool_minimum_delegation
    );

    let expected_transient_stake = reserve_before_rebalance.lamports - 2 * stake_rent;
    assert_eq!(
        transient_stake_account.stake().unwrap().delegation.stake,
        expected_transient_stake
    );

    drop(fixture);
}

#[tokio::test]
async fn test_rebalance_decrease() {
    let fixture = TestFixture::new().await;
    let ctx = &fixture.ctx;
    let clock: Clock = fixture.get_sysvar().await;
    let epoch_schedule: EpochSchedule = fixture.get_sysvar().await;
    fixture
        .advance_num_epochs(epoch_schedule.first_normal_epoch - clock.epoch, 10)
        .await;
    fixture.initialize_stake_pool().await;
    fixture.initialize_config(None).await;
    fixture.initialize_steward_state().await;

    let mut steward_config: Config = fixture
        .load_and_deserialize(&fixture.steward_config.pubkey())
        .await;
    let mut steward_state_account: StewardStateAccount =
        fixture.load_and_deserialize(&fixture.steward_state).await;

    // TODO FIX?
    steward_config.parameters.scoring_unstake_cap_bps = 250;
    steward_config.parameters.instant_unstake_cap_bps = 250;
    steward_config.parameters.stake_deposit_unstake_cap_bps = 250;
    steward_config.parameters.minimum_voting_epochs = 1;

    steward_state_account.state.state_tag = StewardStateEnum::Rebalance;
    steward_state_account.state.num_pool_validators = MAX_VALIDATORS - 1;
    steward_state_account.state.next_cycle_epoch = epoch_schedule.first_normal_epoch + 10;
    steward_state_account.state.current_epoch = epoch_schedule.first_normal_epoch;

    let mut rng = StdRng::from_seed([42; 32]);
    let mut arr: Vec<u16> = (0..MAX_VALIDATORS as u16).collect();
    arr.shuffle(&mut rng);

    // Ensure that the validator with validator_list_index MAX_VALIDATORS - 1 is the first element in the scores array
    // This guarantees we will iterate through all scores as well as all validators in the CPI, for max compute
    let last_validator_index = arr
        .iter()
        .position(|&x| x == MAX_VALIDATORS as u16 - 1)
        .unwrap();

    arr.swap(last_validator_index, 0);

    steward_state_account
        .state
        .sorted_score_indices
        .copy_from_slice(&arr);

    steward_state_account
        .state
        .sorted_yield_score_indices
        .copy_from_slice(&arr);

    for i in 0..MAX_VALIDATORS {
        steward_state_account.state.delegations[i] = Delegation {
            numerator: 1,
            denominator: MAX_VALIDATORS as u32,
        };
    }

    // Setup force unstake from half of validators to ensure we are going into inner condition
    // and also doing a real unstake from the target validator
    for i in 0..MAX_VALIDATORS / 2 {
        steward_state_account
            .state
            .instant_unstake
            .set(i, true)
            .unwrap();
    }
    steward_state_account
        .state
        .instant_unstake
        .set(MAX_VALIDATORS - 1, true)
        .unwrap();

    ctx.borrow_mut().set_account(
        &fixture.steward_state,
        &serialized_steward_state_account(steward_state_account).into(),
    );

    ctx.borrow_mut().set_account(
        &fixture.steward_config.pubkey(),
        &serialized_config(steward_config).into(),
    );

    let vote_account = Pubkey::new_unique();
    let validator_history_address =
        fixture.initialize_validator_history_with_credits(vote_account, 0);

    let validator_list_account_info = fixture
        .get_account(&fixture.stake_pool_meta.validator_list)
        .await;

    let validator_list: ValidatorList = fixture
        .load_and_deserialize(&fixture.stake_pool_meta.validator_list)
        .await;

    let mut spl_validator_list = validator_list.as_ref().clone();

    let stake_program_minimum = fixture.fetch_minimum_delegation().await;
    let pool_minimum_delegation = minimum_delegation(stake_program_minimum);
    let stake_rent = fixture.fetch_stake_rent().await;
    let minimum_active_stake_with_rent = pool_minimum_delegation + stake_rent;

    // Adds all validators except the last one, to be added in auto_add_validator_to_pool.
    // That instruction also creates the stake account for the validator
    for _ in 0..MAX_VALIDATORS - 1 {
        spl_validator_list.validators.push(ValidatorStakeInfo {
            active_stake_lamports: (1000 + minimum_active_stake_with_rent).into(),
            vote_account_address: Pubkey::new_unique(),
            ..ValidatorStakeInfo::default()
        });
    }

    ctx.borrow_mut().set_account(
        &fixture.stake_pool_meta.validator_list,
        &serialized_validator_list_account(
            spl_validator_list.clone(),
            Some(validator_list_account_info.data.len()),
        )
        .into(),
    );

    let (stake_account_address, transient_stake_account_address, withdraw_authority) =
        fixture.stake_accounts_for_validator(vote_account).await;

    let add_validator_to_pool_ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::AutoAddValidator {
            validator_history_account: validator_history_address,
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
            stake_program: stake::program::id(),
            system_program: solana_program::system_program::id(),
            signer: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::AutoAddValidatorToPool {}.data(),
    };

    let tx = Transaction::new_signed_with_payer(
        &[
            ComputeBudgetInstruction::set_compute_unit_limit(1_400_000),
            ComputeBudgetInstruction::request_heap_frame(256 * 1024),
            add_validator_to_pool_ix,
        ],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        ctx.borrow().last_blockhash,
    );
    fixture.submit_transaction_assert_success(tx).await;

    // Simulating stake deposit
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

    // Increase stake on validator so there's a lot to unstake

    let stake_pool: StakePool = fixture
        .load_and_deserialize(&fixture.stake_pool_meta.stake_pool)
        .await;
    let mut stake_pool_spl = stake_pool.as_ref().clone();
    stake_pool_spl.pool_token_supply +=
        (MAX_VALIDATORS as u64 - 1) * (minimum_active_stake_with_rent + 1000) + 2_000_000_000;
    stake_pool_spl.total_lamports +=
        (MAX_VALIDATORS as u64 - 1) * (minimum_active_stake_with_rent + 1000) + 2_000_000_000;

    stake_stake.delegation.stake += 2_000_000_000;
    stake_account = StakeStateV2::Stake(stake_meta, stake_stake, stake_flags);

    ctx.borrow_mut().set_account(
        &stake_account_address,
        &serialized_stake_account(stake_account, stake_account_data.lamports + 2_000_000_000)
            .into(),
    );
    ctx.borrow_mut().set_account(
        &fixture.stake_pool_meta.stake_pool,
        &serialized_stake_pool_account(stake_pool_spl, std::mem::size_of::<StakePool>()).into(),
    );

    let validator_list: ValidatorList = fixture
        .load_and_deserialize(&fixture.stake_pool_meta.validator_list)
        .await;

    let mut spl_validator_list = validator_list.as_ref().clone();
    let lamports = spl_validator_list.validators[MAX_VALIDATORS - 1].active_stake_lamports;
    spl_validator_list.validators[MAX_VALIDATORS - 1].active_stake_lamports =
        (u64::from(lamports) + 2_000_000_000).into();

    ctx.borrow_mut().set_account(
        &fixture.stake_pool_meta.validator_list,
        &serialized_validator_list_account(
            spl_validator_list.clone(),
            Some(validator_list_account_info.data.len()),
        )
        .into(),
    );

    let mut steward_state_account: StewardStateAccount =
        fixture.load_and_deserialize(&fixture.steward_state).await;
    steward_state_account.state.num_pool_validators += 1;
    ctx.borrow_mut().set_account(
        &fixture.steward_state,
        &serialized_steward_state_account(steward_state_account).into(),
    );

    let rebalance_ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::Rebalance {
            config: fixture.steward_config.pubkey(),
            state_account: fixture.steward_state,
            validator_history: validator_history_address,
            validator_list: fixture.stake_pool_meta.validator_list,
            stake_pool: fixture.stake_pool_meta.stake_pool,
            reserve_stake: fixture.stake_pool_meta.reserve,
            stake_pool_program: spl_stake_pool::id(),
            staker: fixture.staker,
            withdraw_authority,
            vote_account,
            stake_account: stake_account_address,
            transient_stake_account: transient_stake_account_address,
            clock: sysvar::clock::id(),
            rent: sysvar::rent::id(),
            system_program: solana_program::system_program::id(),
            stake_program: stake::program::id(),
            stake_config: stake::config::ID,
            stake_history: solana_program::sysvar::stake_history::id(),
            signer: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::Rebalance {
            validator_list_index: MAX_VALIDATORS - 1,
        }
        .data(),
    };

    let tx = Transaction::new_signed_with_payer(
        &[
            ComputeBudgetInstruction::request_heap_frame(256 * 1024),
            ComputeBudgetInstruction::set_compute_unit_limit(1_400_000),
            rebalance_ix,
        ],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        ctx.borrow().last_blockhash,
    );
    fixture.submit_transaction_assert_success(tx).await;

    let stake_account_data = fixture.get_account(&stake_account_address).await;

    let stake_account = StakeStateV2::deserialize(&mut stake_account_data.data.as_slice()).unwrap();

    let transient_stake_account = fixture.get_account(&transient_stake_account_address).await;
    let transient_stake_account =
        StakeStateV2::deserialize(&mut transient_stake_account.data.as_slice()).unwrap();

    assert_eq!(
        stake_account.stake().unwrap().delegation.stake,
        pool_minimum_delegation
    );
    assert_eq!(
        transient_stake_account.stake().unwrap().delegation.stake,
        2_000_000_000
    );

    // Assert delegations were modified properly
    let steward_state_account: StewardStateAccount =
        fixture.load_and_deserialize(&fixture.steward_state).await;
    assert_eq!(
        steward_state_account.state.delegations[MAX_VALIDATORS - 1].numerator,
        0
    );
    assert!(
        steward_state_account.state.delegations[0].numerator == 1
            && steward_state_account.state.delegations[0].denominator == MAX_VALIDATORS as u32 - 1
    );

    drop(fixture);
}

#[tokio::test]
async fn test_rebalance_other_cases() {
    // Test pause
    let fixture = TestFixture::new().await;
    let ctx = &fixture.ctx;
    let clock: Clock = fixture.get_sysvar().await;
    let epoch_schedule: EpochSchedule = fixture.get_sysvar().await;
    fixture
        .advance_num_epochs(epoch_schedule.first_normal_epoch - clock.epoch, 10)
        .await;
    fixture.initialize_stake_pool().await;
    fixture.initialize_config(None).await;
    fixture.initialize_steward_state().await;

    let mut steward_config: Config = fixture
        .load_and_deserialize(&fixture.steward_config.pubkey())
        .await;
    steward_config.set_paused(true);
    steward_config.parameters.minimum_voting_epochs = 1;

    let vote_account = Pubkey::new_unique();
    let validator_history_address =
        fixture.initialize_validator_history_with_credits(vote_account, 0);

    let validator_list_account_info = fixture
        .get_account(&fixture.stake_pool_meta.validator_list)
        .await;

    let validator_list: ValidatorList = fixture
        .load_and_deserialize(&fixture.stake_pool_meta.validator_list)
        .await;

    let mut spl_validator_list = validator_list.as_ref().clone();

    let stake_program_minimum = fixture.fetch_minimum_delegation().await;
    let pool_minimum_delegation = minimum_delegation(stake_program_minimum);
    let stake_rent = fixture.fetch_stake_rent().await;
    let minimum_active_stake_with_rent = pool_minimum_delegation + stake_rent;

    // Adds all validators except the last one, to be added in auto_add_validator_to_pool.
    // That instruction also creates the stake account for the validator
    for _ in 0..MAX_VALIDATORS - 1 {
        spl_validator_list.validators.push(ValidatorStakeInfo {
            active_stake_lamports: minimum_active_stake_with_rent.into(),
            vote_account_address: Pubkey::new_unique(),
            ..ValidatorStakeInfo::default()
        });
    }

    ctx.borrow_mut().set_account(
        &fixture.stake_pool_meta.validator_list,
        &serialized_validator_list_account(
            spl_validator_list.clone(),
            Some(validator_list_account_info.data.len()),
        )
        .into(),
    );

    ctx.borrow_mut().set_account(
        &fixture.steward_config.pubkey(),
        &serialized_config(steward_config).into(),
    );

    let (stake_account_address, transient_stake_account_address, withdraw_authority) =
        fixture.stake_accounts_for_validator(vote_account).await;

    let add_validator_to_pool_ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::AutoAddValidator {
            validator_history_account: validator_history_address,
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
            stake_program: stake::program::id(),
            system_program: solana_program::system_program::id(),
            signer: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::AutoAddValidatorToPool {}.data(),
    };

    let tx = Transaction::new_signed_with_payer(
        &[
            ComputeBudgetInstruction::set_compute_unit_limit(1_400_000),
            ComputeBudgetInstruction::request_heap_frame(256 * 1024),
            add_validator_to_pool_ix,
        ],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        ctx.borrow().last_blockhash,
    );
    fixture.submit_transaction_assert_success(tx).await;

    let rebalance_ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::Rebalance {
            config: fixture.steward_config.pubkey(),
            state_account: fixture.steward_state,
            validator_history: validator_history_address,
            validator_list: fixture.stake_pool_meta.validator_list,
            stake_pool: fixture.stake_pool_meta.stake_pool,
            reserve_stake: fixture.stake_pool_meta.reserve,
            stake_pool_program: spl_stake_pool::id(),
            staker: fixture.staker,
            withdraw_authority,
            vote_account,
            stake_account: stake_account_address,
            transient_stake_account: transient_stake_account_address,
            clock: sysvar::clock::id(),
            rent: sysvar::rent::id(),
            system_program: solana_program::system_program::id(),
            stake_program: stake::program::id(),
            stake_config: stake::config::ID,
            stake_history: solana_program::sysvar::stake_history::id(),
            signer: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::Rebalance {
            validator_list_index: MAX_VALIDATORS - 1,
        }
        .data(),
    };
    let tx = Transaction::new_signed_with_payer(
        &[rebalance_ix.clone()],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        ctx.borrow().last_blockhash,
    );
    fixture
        .submit_transaction_assert_error(tx, "StateMachinePaused")
        .await;

    // Test wrong state
    let mut steward_state_account: StewardStateAccount =
        fixture.load_and_deserialize(&fixture.steward_state).await;

    steward_state_account.state.state_tag = StewardStateEnum::Idle;
    steward_state_account.state.num_pool_validators = MAX_VALIDATORS;
    steward_state_account.state.next_cycle_epoch = epoch_schedule.first_normal_epoch + 10;
    steward_state_account.state.current_epoch = epoch_schedule.first_normal_epoch;
    steward_config.set_paused(false);
    ctx.borrow_mut().set_account(
        &fixture.steward_state,
        &serialized_steward_state_account(steward_state_account).into(),
    );
    ctx.borrow_mut().set_account(
        &fixture.steward_config.pubkey(),
        &serialized_config(steward_config).into(),
    );

    let blockhash = fixture.get_latest_blockhash().await;
    let tx = Transaction::new_signed_with_payer(
        &[rebalance_ix.clone()],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        blockhash,
    );
    fixture
        .submit_transaction_assert_error(tx, "InvalidState")
        .await;

    // Test transition to Idle when complete
    let mut steward_state_account: StewardStateAccount =
        fixture.load_and_deserialize(&fixture.steward_state).await;
    steward_state_account.state.state_tag = StewardStateEnum::Rebalance;
    for i in 0..MAX_VALIDATORS {
        steward_state_account.state.sorted_score_indices[i] = i as u16;
        steward_state_account.state.sorted_yield_score_indices[i] = i as u16;
        // Skip over current validator
        if i != MAX_VALIDATORS - 1 {
            steward_state_account.state.progress.set(i, true).unwrap();
        }
    }

    ctx.borrow_mut().set_account(
        &fixture.steward_state,
        &serialized_steward_state_account(steward_state_account).into(),
    );

    // doesn't matter what it does for staking, just that it completes and transitions

    let blockhash = fixture.get_latest_blockhash().await;
    let tx = Transaction::new_signed_with_payer(
        &[
            ComputeBudgetInstruction::set_compute_unit_limit(1_400_000),
            rebalance_ix,
        ],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        blockhash,
    );

    fixture.submit_transaction_assert_success(tx).await;
    let steward_state_account: StewardStateAccount =
        fixture.load_and_deserialize(&fixture.steward_state).await;
    assert!(matches!(
        steward_state_account.state.state_tag,
        StewardStateEnum::Idle
    ));

    drop(fixture);
}
