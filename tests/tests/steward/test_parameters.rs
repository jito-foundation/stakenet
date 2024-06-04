/// Basic integration test
use anchor_lang::{solana_program::instruction::Instruction, InstructionData, ToAccountMetas};
use jito_steward::{
    constants::{
        BASIS_POINTS_MAX, COMMISSION_MAX, COMPUTE_SCORE_SLOT_RANGE_MIN, EPOCH_PROGRESS_MAX,
        MAX_VALIDATORS, NUM_EPOCHS_BETWEEN_SCORING_MAX,
    },
    Config, Parameters, UpdateParametersArgs,
};
use solana_program_test::*;
use solana_sdk::{signer::Signer, transaction::Transaction};
use tests::steward_fixtures::TestFixture;

// ---------- INTEGRATION TESTS ----------
async fn _set_parameter(fixture: &TestFixture, update_parameters: &UpdateParametersArgs) {
    let ctx = &fixture.ctx;
    let ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::UpdateParameters {
            config: fixture.steward_config.pubkey(),
            authority: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::UpdateParameters {
            update_parameters_args: update_parameters.clone(),
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

    if let Some(mev_commission_range) = update_parameters.mev_commission_range {
        assert_eq!(
            config.parameters.mev_commission_range, mev_commission_range,
            "mev_commission_range, does not match update"
        );
    }

    if let Some(epoch_credits_range) = update_parameters.epoch_credits_range {
        assert_eq!(
            config.parameters.epoch_credits_range, epoch_credits_range,
            "epoch_credits_range, does not match update"
        );
    }

    if let Some(commission_range) = update_parameters.commission_range {
        assert_eq!(
            config.parameters.commission_range, commission_range,
            "commission_range, does not match update"
        );
    }

    if let Some(scoring_delinquency_threshold_ratio) =
        update_parameters.scoring_delinquency_threshold_ratio
    {
        assert_eq!(
            config.parameters.scoring_delinquency_threshold_ratio,
            scoring_delinquency_threshold_ratio,
            "scoring_delinquency_threshold_ratio, does not match update"
        );
    }

    if let Some(instant_unstake_delinquency_threshold_ratio) =
        update_parameters.instant_unstake_delinquency_threshold_ratio
    {
        assert_eq!(
            config
                .parameters
                .instant_unstake_delinquency_threshold_ratio,
            instant_unstake_delinquency_threshold_ratio,
            "instant_unstake_delinquency_threshold_ratio, does not match update"
        );
    }

    if let Some(mev_commission_bps_threshold) = update_parameters.mev_commission_bps_threshold {
        assert_eq!(
            config.parameters.mev_commission_bps_threshold, mev_commission_bps_threshold,
            "mev_commission_bps_threshold, does not match update"
        );
    }

    if let Some(commission_threshold) = update_parameters.commission_threshold {
        assert_eq!(
            config.parameters.commission_threshold, commission_threshold,
            "commission_threshold, does not match update"
        );
    }

    if let Some(num_delegation_validators) = update_parameters.num_delegation_validators {
        assert_eq!(
            config.parameters.num_delegation_validators, num_delegation_validators,
            "num_delegation_validators, does not match update"
        );
    }

    if let Some(scoring_unstake_cap_bps) = update_parameters.scoring_unstake_cap_bps {
        assert_eq!(
            config.parameters.scoring_unstake_cap_bps, scoring_unstake_cap_bps,
            "scoring_unstake_cap_bps, does not match update"
        );
    }

    if let Some(instant_unstake_cap_bps) = update_parameters.instant_unstake_cap_bps {
        assert_eq!(
            config.parameters.instant_unstake_cap_bps, instant_unstake_cap_bps,
            "instant_unstake_cap_bps, does not match update"
        );
    }

    if let Some(stake_deposit_unstake_cap_bps) = update_parameters.stake_deposit_unstake_cap_bps {
        assert_eq!(
            config.parameters.stake_deposit_unstake_cap_bps, stake_deposit_unstake_cap_bps,
            "stake_deposit_unstake_cap_bps, does not match update"
        );
    }

    if let Some(instant_unstake_epoch_progress) = update_parameters.instant_unstake_epoch_progress {
        assert_eq!(
            config.parameters.instant_unstake_epoch_progress, instant_unstake_epoch_progress,
            "instant_unstake_epoch_progress, does not match update"
        );
    }

    if let Some(compute_score_slot_range) = update_parameters.compute_score_slot_range {
        assert_eq!(
            config.parameters.compute_score_slot_range, compute_score_slot_range,
            "compute_score_slot_range, does not match update"
        );
    }

    if let Some(instant_unstake_inputs_epoch_progress) =
        update_parameters.instant_unstake_inputs_epoch_progress
    {
        assert_eq!(
            config.parameters.instant_unstake_inputs_epoch_progress,
            instant_unstake_inputs_epoch_progress,
            "instant_unstake_inputs_epoch_progress, does not match update"
        );
    }

    if let Some(num_epochs_between_scoring) = update_parameters.num_epochs_between_scoring {
        assert_eq!(
            config.parameters.num_epochs_between_scoring, num_epochs_between_scoring,
            "num_epochs_between_scoring, does not match update"
        );
    }

    if let Some(minimum_stake_lamports) = update_parameters.minimum_stake_lamports {
        assert_eq!(
            config.parameters.minimum_stake_lamports, minimum_stake_lamports,
            "minimum_stake_lamports, does not match update"
        );
    }

    if let Some(minimum_voting_epochs) = update_parameters.minimum_voting_epochs {
        assert_eq!(
            config.parameters.minimum_voting_epochs, minimum_voting_epochs,
            "minimum_voting_epochs, does not match update"
        );
    }
}

#[tokio::test]
async fn test_update_parameters() {
    let fixture = TestFixture::new().await;
    fixture.initialize_stake_pool().await;
    fixture.initialize_config(None).await;
    fixture.initialize_steward_state().await;

    _set_parameter(
        &fixture,
        &UpdateParametersArgs {
            ..UpdateParametersArgs::default()
        },
    )
    .await;

    fixture.advance_num_epochs(3000, 0).await;

    _set_parameter(
        &fixture,
        &UpdateParametersArgs {
            mev_commission_range: Some(10),
            epoch_credits_range: Some(20),
            commission_range: Some(20),
            scoring_delinquency_threshold_ratio: Some(0.875),
            instant_unstake_delinquency_threshold_ratio: Some(0.1),
            mev_commission_bps_threshold: Some(999),
            commission_threshold: Some(10),
            historical_commission_threshold: Some(34),
            num_delegation_validators: Some(3),
            scoring_unstake_cap_bps: Some(1000),
            instant_unstake_cap_bps: Some(1000),
            stake_deposit_unstake_cap_bps: Some(1000),
            instant_unstake_epoch_progress: Some(0.95),
            compute_score_slot_range: Some(500),
            instant_unstake_inputs_epoch_progress: Some(0.3),
            num_epochs_between_scoring: Some(8),
            minimum_stake_lamports: Some(1),
            minimum_voting_epochs: Some(1),
        },
    )
    .await;

    drop(fixture);
}

// ---------- UNIT TESTS ----------

fn _test_parameter(
    update_parameters: &UpdateParametersArgs,
    current_epoch: Option<u64>,
    slots_per_epoch: Option<u64>,
    valid_parameters: Option<Parameters>,
) -> Result<Parameters, anchor_lang::error::Error> {
    let valid_parameters = valid_parameters.unwrap_or(Parameters {
        mev_commission_range: 10,
        epoch_credits_range: 30,
        commission_range: 30,
        scoring_delinquency_threshold_ratio: 0.85,
        instant_unstake_delinquency_threshold_ratio: 0.7,
        mev_commission_bps_threshold: 1000,
        commission_threshold: 5,
        historical_commission_threshold: 50,
        num_delegation_validators: 200,
        scoring_unstake_cap_bps: 10,
        instant_unstake_cap_bps: 10,
        stake_deposit_unstake_cap_bps: 10,
        instant_unstake_epoch_progress: 0.9,
        compute_score_slot_range: 1000,
        instant_unstake_inputs_epoch_progress: 0.5,
        num_epochs_between_scoring: 10,
        minimum_stake_lamports: 5_000_000_000_000,
        minimum_voting_epochs: 5,
        padding0: [0; 6],
    });

    // First Valid Epoch
    let current_epoch = current_epoch.unwrap_or(512);
    let slots_per_epoch = slots_per_epoch.unwrap_or(432_000);

    valid_parameters.get_valid_updated_parameters(update_parameters, current_epoch, slots_per_epoch)
}

#[test]
fn test_mev_commission_range() {
    let new_value = 1;
    let update_parameters = UpdateParametersArgs {
        mev_commission_range: Some(new_value),
        ..UpdateParametersArgs::default()
    };

    {
        // Out of range
        let not_okay_epoch = 0;
        let result = _test_parameter(&update_parameters, Some(not_okay_epoch), None, None);
        assert!(result.is_err());
    }

    {
        // In range
        let okay_epoch = 512;
        let result = _test_parameter(&update_parameters, Some(okay_epoch), None, None);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().mev_commission_range, new_value);
    }
}

#[test]
fn test_epoch_credits_range() {
    let new_value = 1;
    let update_parameters = UpdateParametersArgs {
        epoch_credits_range: Some(new_value),
        ..UpdateParametersArgs::default()
    };

    {
        // Out of range
        let not_okay_epoch = 0;
        let result = _test_parameter(&update_parameters, Some(not_okay_epoch), None, None);
        assert!(result.is_err());
    }

    {
        // In range
        let okay_epoch = 512;
        let result = _test_parameter(&update_parameters, Some(okay_epoch), None, None);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().epoch_credits_range, new_value);
    }
}

#[test]
fn test_commission_range() {
    let new_value = 1;
    let update_parameters = UpdateParametersArgs {
        commission_range: Some(new_value),
        ..UpdateParametersArgs::default()
    };

    {
        // Out of range
        let not_okay_epoch = 0;
        let result = _test_parameter(&update_parameters, Some(not_okay_epoch), None, None);
        assert!(result.is_err());
    }

    {
        // In range
        let okay_epoch = 512;
        let result = _test_parameter(&update_parameters, Some(okay_epoch), None, None);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().commission_range, new_value);
    }
}

#[test]
fn test_scoring_delinquency_threshold_ratio() {
    {
        // Cannot be less than 0
        let new_value = -0.1;
        let update_bad_arg = UpdateParametersArgs {
            scoring_delinquency_threshold_ratio: Some(new_value),
            ..UpdateParametersArgs::default()
        };
        let result = _test_parameter(&update_bad_arg, None, None, None);
        assert!(result.is_err());
    }

    {
        // Cannot be greater than 1
        let new_value = 1.1;
        let update_bad_arg = UpdateParametersArgs {
            scoring_delinquency_threshold_ratio: Some(new_value),
            ..UpdateParametersArgs::default()
        };
        let result = _test_parameter(&update_bad_arg, None, None, None);
        assert!(result.is_err());
    }

    {
        // 0.0 is okay
        let new_value = 0.0;
        let okay_arg = UpdateParametersArgs {
            scoring_delinquency_threshold_ratio: Some(new_value),
            ..UpdateParametersArgs::default()
        };

        let result = _test_parameter(&okay_arg, None, None, None);
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap().scoring_delinquency_threshold_ratio,
            new_value
        );
    }

    {
        // 1.0 is okay
        let new_value = 1.0;
        let okay_arg = UpdateParametersArgs {
            scoring_delinquency_threshold_ratio: Some(new_value),
            ..UpdateParametersArgs::default()
        };
        let result = _test_parameter(&okay_arg, None, None, None);
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap().scoring_delinquency_threshold_ratio,
            new_value
        );
    }

    {
        // 0.5 is okay
        let new_value = 0.5;
        let okay_arg = UpdateParametersArgs {
            scoring_delinquency_threshold_ratio: Some(new_value),
            ..UpdateParametersArgs::default()
        };
        let result = _test_parameter(&okay_arg, None, None, None);
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap().scoring_delinquency_threshold_ratio,
            new_value
        );
    }
}

#[test]
fn test_instant_unstake_delinquency_threshold_ratio() {
    {
        // Cannot be less than 0
        let new_value = -0.1;
        let update_bad_arg = UpdateParametersArgs {
            instant_unstake_delinquency_threshold_ratio: Some(new_value),
            ..UpdateParametersArgs::default()
        };
        let result = _test_parameter(&update_bad_arg, None, None, None);
        assert!(result.is_err());
    }

    {
        // Cannot be greater than 1
        let new_value = 1.1;
        let update_bad_arg = UpdateParametersArgs {
            instant_unstake_delinquency_threshold_ratio: Some(new_value),
            ..UpdateParametersArgs::default()
        };
        let result = _test_parameter(&update_bad_arg, None, None, None);
        assert!(result.is_err());
    }

    {
        // 0.0 is okay
        let new_value = 0.0;
        let okay_arg = UpdateParametersArgs {
            instant_unstake_delinquency_threshold_ratio: Some(new_value),
            ..UpdateParametersArgs::default()
        };

        let result = _test_parameter(&okay_arg, None, None, None);
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap().instant_unstake_delinquency_threshold_ratio,
            new_value
        );
    }

    {
        // 1.0 is okay
        let new_value = 1.0;
        let okay_arg = UpdateParametersArgs {
            instant_unstake_delinquency_threshold_ratio: Some(new_value),
            ..UpdateParametersArgs::default()
        };
        let result = _test_parameter(&okay_arg, None, None, None);
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap().instant_unstake_delinquency_threshold_ratio,
            new_value
        );
    }

    {
        // 0.5 is okay
        let new_value = 0.5;
        let okay_arg = UpdateParametersArgs {
            instant_unstake_delinquency_threshold_ratio: Some(new_value),
            ..UpdateParametersArgs::default()
        };
        let result = _test_parameter(&okay_arg, None, None, None);
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap().instant_unstake_delinquency_threshold_ratio,
            new_value
        );
    }
}

#[test]
fn test_mev_commission_bps_threshold() {
    {
        // Out of range
        let new_value = BASIS_POINTS_MAX + 1;
        let update_parameters = UpdateParametersArgs {
            mev_commission_bps_threshold: Some(new_value),
            ..UpdateParametersArgs::default()
        };
        let result = _test_parameter(&update_parameters, None, None, None);
        assert!(result.is_err());
    }

    {
        // In range
        let new_value = 0;
        let update_parameters = UpdateParametersArgs {
            mev_commission_bps_threshold: Some(new_value),
            ..UpdateParametersArgs::default()
        };
        let result = _test_parameter(&update_parameters, None, None, None);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().mev_commission_bps_threshold, new_value);
    }
}

#[test]
fn test_commission_threshold() {
    {
        // Out of range
        let new_value = COMMISSION_MAX + 1;
        let update_parameters = UpdateParametersArgs {
            commission_threshold: Some(new_value),
            ..UpdateParametersArgs::default()
        };
        let result = _test_parameter(&update_parameters, None, None, None);
        assert!(result.is_err());
    }

    {
        // In range
        let new_value = 0;
        let update_parameters = UpdateParametersArgs {
            commission_threshold: Some(new_value),
            ..UpdateParametersArgs::default()
        };
        let result = _test_parameter(&update_parameters, None, None, None);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().commission_threshold, new_value);
    }
}

#[test]
fn test_historical_commission_threshold() {
    {
        // Out of range
        let new_value = COMMISSION_MAX + 1;
        let update_parameters = UpdateParametersArgs {
            historical_commission_threshold: Some(new_value),
            ..UpdateParametersArgs::default()
        };
        let result = _test_parameter(&update_parameters, None, None, None);
        assert!(result.is_err());
    }

    {
        // In range
        let new_value = 0;
        let update_parameters = UpdateParametersArgs {
            historical_commission_threshold: Some(new_value),
            ..UpdateParametersArgs::default()
        };
        let result = _test_parameter(&update_parameters, None, None, None);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().historical_commission_threshold, new_value);
    }
}

#[test]
fn test_num_delegation_validators() {
    {
        // Cannot be 0
        let new_value = 0;
        let update_parameters = UpdateParametersArgs {
            num_delegation_validators: Some(new_value),
            ..UpdateParametersArgs::default()
        };
        let result = _test_parameter(&update_parameters, None, None, None);
        assert!(result.is_err());
    }

    {
        // Cannot be more than MAX_VALIDATORS
        let new_value = MAX_VALIDATORS + 1;
        let update_parameters = UpdateParametersArgs {
            num_delegation_validators: Some(new_value as u32),
            ..UpdateParametersArgs::default()
        };
        let result = _test_parameter(&update_parameters, None, None, None);
        assert!(result.is_err());
    }

    {
        // In range
        let new_value = 1;
        let update_parameters = UpdateParametersArgs {
            num_delegation_validators: Some(new_value),
            ..UpdateParametersArgs::default()
        };
        let result = _test_parameter(&update_parameters, None, None, None);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().num_delegation_validators, new_value);
    }
}

#[test]
fn test_scoring_unstake_cap_bps() {
    {
        // Out of range
        let new_value = BASIS_POINTS_MAX + 1;
        let update_parameters = UpdateParametersArgs {
            scoring_unstake_cap_bps: Some(new_value as u32),
            ..UpdateParametersArgs::default()
        };
        let result = _test_parameter(&update_parameters, None, None, None);
        assert!(result.is_err());
    }

    {
        // In range
        let new_value = 0;
        let update_parameters = UpdateParametersArgs {
            scoring_unstake_cap_bps: Some(new_value),
            ..UpdateParametersArgs::default()
        };
        let result = _test_parameter(&update_parameters, None, None, None);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().scoring_unstake_cap_bps, new_value);
    }
}

#[test]
fn test_instant_unstake_cap_bps() {
    {
        // Out of range
        let new_value = BASIS_POINTS_MAX + 1;
        let update_parameters = UpdateParametersArgs {
            instant_unstake_cap_bps: Some(new_value as u32),
            ..UpdateParametersArgs::default()
        };
        let result = _test_parameter(&update_parameters, None, None, None);
        assert!(result.is_err());
    }

    {
        // In range
        let new_value = 0;
        let update_parameters = UpdateParametersArgs {
            instant_unstake_cap_bps: Some(new_value),
            ..UpdateParametersArgs::default()
        };
        let result = _test_parameter(&update_parameters, None, None, None);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().instant_unstake_cap_bps, new_value);
    }
}

#[test]
fn test_stake_deposit_unstake_cap_bps() {
    {
        // Out of range
        let new_value = BASIS_POINTS_MAX + 1;
        let update_parameters = UpdateParametersArgs {
            stake_deposit_unstake_cap_bps: Some(new_value as u32),
            ..UpdateParametersArgs::default()
        };
        let result = _test_parameter(&update_parameters, None, None, None);
        assert!(result.is_err());
    }

    {
        // In range
        let new_value = 0;
        let update_parameters = UpdateParametersArgs {
            stake_deposit_unstake_cap_bps: Some(new_value),
            ..UpdateParametersArgs::default()
        };
        let result = _test_parameter(&update_parameters, None, None, None);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().stake_deposit_unstake_cap_bps, new_value);
    }
}

#[test]
fn test_instant_unstake_epoch_progress() {
    {
        // Cannot be less than 0
        let new_value = -0.1;
        let update_bad_arg = UpdateParametersArgs {
            instant_unstake_epoch_progress: Some(new_value),
            ..UpdateParametersArgs::default()
        };
        let result = _test_parameter(&update_bad_arg, None, None, None);
        assert!(result.is_err());
    }

    {
        // Cannot be greater than 1
        let new_value = 1.1;
        let update_bad_arg = UpdateParametersArgs {
            instant_unstake_epoch_progress: Some(new_value),
            ..UpdateParametersArgs::default()
        };
        let result = _test_parameter(&update_bad_arg, None, None, None);
        assert!(result.is_err());
    }

    {
        // 0.0 is okay
        let new_value = 0.0;
        let okay_arg = UpdateParametersArgs {
            instant_unstake_epoch_progress: Some(new_value),
            ..UpdateParametersArgs::default()
        };

        let result = _test_parameter(&okay_arg, None, None, None);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().instant_unstake_epoch_progress, new_value);
    }

    {
        // EPOCH_PROGRESS_MAX is okay
        let new_value = EPOCH_PROGRESS_MAX;
        let okay_arg = UpdateParametersArgs {
            instant_unstake_epoch_progress: Some(new_value),
            ..UpdateParametersArgs::default()
        };
        let result = _test_parameter(&okay_arg, None, None, None);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().instant_unstake_epoch_progress, new_value);
    }

    {
        // 0.5 is okay
        let new_value = 0.5;
        let okay_arg = UpdateParametersArgs {
            instant_unstake_epoch_progress: Some(new_value),
            ..UpdateParametersArgs::default()
        };
        let result = _test_parameter(&okay_arg, None, None, None);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().instant_unstake_epoch_progress, new_value);
    }
}

#[test]
fn test_instant_inputs_epoch_progress() {
    {
        // Cannot be less than 0
        let new_value = -0.1;
        let update_bad_arg = UpdateParametersArgs {
            instant_unstake_inputs_epoch_progress: Some(new_value),
            ..UpdateParametersArgs::default()
        };
        let result = _test_parameter(&update_bad_arg, None, None, None);
        assert!(result.is_err());
    }

    {
        // Cannot be greater than 1
        let new_value = 1.1;
        let update_bad_arg = UpdateParametersArgs {
            instant_unstake_inputs_epoch_progress: Some(new_value),
            ..UpdateParametersArgs::default()
        };
        let result = _test_parameter(&update_bad_arg, None, None, None);
        assert!(result.is_err());
    }

    {
        // 0.0 is okay
        let new_value = 0.0;
        let okay_arg = UpdateParametersArgs {
            instant_unstake_inputs_epoch_progress: Some(new_value),
            ..UpdateParametersArgs::default()
        };

        let result = _test_parameter(&okay_arg, None, None, None);
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap().instant_unstake_inputs_epoch_progress,
            new_value
        );
    }

    {
        // EPOCH_PROGRESS_MAX is okay
        let new_value = EPOCH_PROGRESS_MAX;
        let okay_arg = UpdateParametersArgs {
            instant_unstake_inputs_epoch_progress: Some(new_value),
            ..UpdateParametersArgs::default()
        };
        let result = _test_parameter(&okay_arg, None, None, None);
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap().instant_unstake_inputs_epoch_progress,
            new_value
        );
    }

    {
        // 0.5 is okay
        let new_value = 0.5;
        let okay_arg = UpdateParametersArgs {
            instant_unstake_inputs_epoch_progress: Some(new_value),
            ..UpdateParametersArgs::default()
        };
        let result = _test_parameter(&okay_arg, None, None, None);
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap().instant_unstake_inputs_epoch_progress,
            new_value
        );
    }
}

#[test]
fn test_minimum_voting_epochs() {
    let new_value = 1;
    let update_parameters = UpdateParametersArgs {
        minimum_voting_epochs: Some(new_value),
        ..UpdateParametersArgs::default()
    };

    {
        // Out of range
        let not_okay_epoch = 0;
        let result = _test_parameter(&update_parameters, Some(not_okay_epoch), None, None);
        assert!(result.is_err());
    }

    {
        // In range
        let okay_epoch = 512;
        let result = _test_parameter(&update_parameters, Some(okay_epoch), None, None);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().minimum_voting_epochs, new_value);
    }
}

#[test]
fn test_compute_score_slot_range() {
    let min_slots_per_epoch = COMPUTE_SCORE_SLOT_RANGE_MIN;
    let slots_per_epoch = 432_000;

    {
        // Cannot be below min_slots_per_epoch
        let new_value = min_slots_per_epoch - 1;
        let update_parameters = UpdateParametersArgs {
            compute_score_slot_range: Some(new_value),
            ..UpdateParametersArgs::default()
        };
        let result = _test_parameter(&update_parameters, None, Some(slots_per_epoch), None);
        assert!(result.is_err());
    }

    {
        // Cannot be above slots_per_epoch
        let new_value = slots_per_epoch + 1;
        let update_parameters = UpdateParametersArgs {
            compute_score_slot_range: Some(new_value as usize),
            ..UpdateParametersArgs::default()
        };
        let result = _test_parameter(&update_parameters, None, Some(slots_per_epoch), None);
        assert!(result.is_err());
    }

    {
        // In range
        let new_value = COMPUTE_SCORE_SLOT_RANGE_MIN + 1;
        let update_parameters = UpdateParametersArgs {
            compute_score_slot_range: Some(new_value),
            ..UpdateParametersArgs::default()
        };
        let result = _test_parameter(&update_parameters, None, Some(slots_per_epoch), None);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().compute_score_slot_range, new_value);
    }
}

#[test]
fn test_num_epochs_between_scoring() {
    {
        // Cannot be 0
        let new_value = 0;
        let update_parameters = UpdateParametersArgs {
            num_epochs_between_scoring: Some(new_value),
            ..UpdateParametersArgs::default()
        };
        let result = _test_parameter(&update_parameters, None, None, None);
        assert!(result.is_err());
    }

    {
        // Cannot be above
        let new_value = NUM_EPOCHS_BETWEEN_SCORING_MAX + 1;
        let update_parameters = UpdateParametersArgs {
            num_epochs_between_scoring: Some(new_value),
            ..UpdateParametersArgs::default()
        };
        let result = _test_parameter(&update_parameters, None, None, None);
        assert!(result.is_err());
    }

    {
        // In range
        let new_value = 1;
        let update_parameters = UpdateParametersArgs {
            num_epochs_between_scoring: Some(new_value),
            ..UpdateParametersArgs::default()
        };
        let result = _test_parameter(&update_parameters, None, None, None);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().num_epochs_between_scoring, new_value);
    }
}
