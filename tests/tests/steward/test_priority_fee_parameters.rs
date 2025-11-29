use anchor_lang::{InstructionData, ToAccountMetas};
use jito_steward::{
    constants::BASIS_POINTS_MAX, instructions::AuthorityType, Config, Parameters,
    UpdatePriorityFeeParametersArgs,
};
use solana_program_test::*;
use solana_sdk::{instruction::Instruction, signer::Signer, transaction::Transaction};
use tests::steward_fixtures::TestFixture;

fn _validate_config_priority_fee_settings(
    config: &Config,
    expected_priority_fee_parameters: &UpdatePriorityFeeParametersArgs,
) {
    if let Some(priority_fee_lookback_epochs) =
        expected_priority_fee_parameters.priority_fee_lookback_epochs
    {
        assert_eq!(
            config.parameters.priority_fee_lookback_epochs, priority_fee_lookback_epochs,
            "priority_fee_lookback_epochs, does not match update"
        );
    }

    if let Some(priority_fee_lookback_offset) =
        expected_priority_fee_parameters.priority_fee_lookback_offset
    {
        assert_eq!(
            config.parameters.priority_fee_lookback_offset, priority_fee_lookback_offset,
            "priority_fee_lookback_offset, does not match update"
        );
    }

    if let Some(priority_fee_max_commission_bps) =
        expected_priority_fee_parameters.priority_fee_max_commission_bps
    {
        assert_eq!(
            config.parameters.priority_fee_max_commission_bps, priority_fee_max_commission_bps,
            "priority_fee_max_commission_bps, does not match update"
        );
    }

    if let Some(priority_fee_error_margin_bps) =
        expected_priority_fee_parameters.priority_fee_error_margin_bps
    {
        assert_eq!(
            config.parameters.priority_fee_error_margin_bps, priority_fee_error_margin_bps,
            "priority_fee_error_margin_bps, does not match update"
        );
    }

    if let Some(priority_fee_scoring_start_epoch) =
        expected_priority_fee_parameters.priority_fee_scoring_start_epoch
    {
        assert_eq!(
            config.parameters.priority_fee_scoring_start_epoch, priority_fee_scoring_start_epoch,
            "priority_fee_scoring_start_epoch, does not match update"
        );
    }
}

#[tokio::test]
async fn test_initialize_steward_sets_priority_fee_parameters() {
    let fixture = TestFixture::new().await;
    fixture.initialize_stake_pool().await;

    let expected_priority_fee_parameters = UpdatePriorityFeeParametersArgs {
        priority_fee_lookback_epochs: Some(123),
        priority_fee_lookback_offset: Some(12),
        priority_fee_max_commission_bps: Some(1234),
        priority_fee_error_margin_bps: Some(12),
        priority_fee_scoring_start_epoch: Some(10),
    };
    fixture
        .initialize_steward(None, Some(expected_priority_fee_parameters.clone()))
        .await;

    let config: Config = fixture
        .load_and_deserialize(&fixture.steward_config.pubkey())
        .await;

    _validate_config_priority_fee_settings(&config, &expected_priority_fee_parameters);
}

#[tokio::test]
async fn test_update_priority_fee_parameters() {
    let fixture = TestFixture::new().await;
    fixture.initialize_stake_pool().await;
    fixture.initialize_steward(None, None).await;

    let priority_fee_authority_keypair = fixture
        .set_new_authority(AuthorityType::SetPriorityFeeParameterAuthority)
        .await;

    let update_priority_fee_parameters_args = UpdatePriorityFeeParametersArgs {
        priority_fee_lookback_epochs: Some(1),
        priority_fee_lookback_offset: Some(1),
        priority_fee_max_commission_bps: Some(1),
        priority_fee_error_margin_bps: Some(1),
        priority_fee_scoring_start_epoch: Some(1),
    };

    let ctx = &fixture.ctx;
    let ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::UpdatePriorityFeeParameters {
            config: fixture.steward_config.pubkey(),
            authority: priority_fee_authority_keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::UpdatePriorityFeeParameters {
            update_priority_fee_parameters_args: update_priority_fee_parameters_args.clone(),
        }
        .data(),
    };
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&priority_fee_authority_keypair.pubkey()),
        &[&priority_fee_authority_keypair],
        ctx.borrow().last_blockhash,
    );

    fixture.submit_transaction_assert_success(tx).await;

    let config: Config = fixture
        .load_and_deserialize(&fixture.steward_config.pubkey())
        .await;

    _validate_config_priority_fee_settings(&config, &update_priority_fee_parameters_args);
}

#[tokio::test]
async fn test_bad_authority() {
    let fixture = TestFixture::new().await;
    fixture.initialize_stake_pool().await;
    fixture.initialize_steward(None, None).await;

    fixture
        .set_new_authority(AuthorityType::SetPriorityFeeParameterAuthority)
        .await;

    let update_priority_fee_parameters_args = UpdatePriorityFeeParametersArgs {
        priority_fee_lookback_epochs: Some(1),
        priority_fee_lookback_offset: Some(1),
        priority_fee_max_commission_bps: Some(1),
        priority_fee_error_margin_bps: Some(1),
        priority_fee_scoring_start_epoch: Some(1),
    };

    let ctx = &fixture.ctx;
    let ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::UpdatePriorityFeeParameters {
            config: fixture.steward_config.pubkey(),
            authority: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::UpdatePriorityFeeParameters {
            update_priority_fee_parameters_args: update_priority_fee_parameters_args.clone(),
        }
        .data(),
    };
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        ctx.borrow().last_blockhash,
    );

    fixture
        .submit_transaction_assert_error(tx, "ConstraintAddress")
        .await;
}

#[test]
fn test_priority_parameter_validation() {
    let valid_parameters = Parameters {
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
        priority_fee_lookback_epochs: 10,
        priority_fee_lookback_offset: 2,
        priority_fee_max_commission_bps: 5_000,
        priority_fee_error_margin_bps: 10,
        priority_fee_scoring_start_epoch: 0,
        directed_stake_unstake_cap_bps: 750,
        compute_score_epoch_progress: 0.5,
        undirected_stake_floor_lamports: (10_000_000u64 * 1_000_000_000u64).to_le_bytes(),
        _padding_0: [0; 6],
        _padding_1: [0; 28],
        _padding_2: [0; 6],
    };

    // First Valid Epoch
    let current_epoch = 512;
    let slots_per_epoch = 432_000;

    let update_priority_fee_parameters_args = UpdatePriorityFeeParametersArgs {
        priority_fee_lookback_epochs: Some(1),
        priority_fee_lookback_offset: Some(1),
        priority_fee_max_commission_bps: Some(BASIS_POINTS_MAX + 1),
        priority_fee_error_margin_bps: Some(1),
        priority_fee_scoring_start_epoch: Some(1),
    };
    let res = valid_parameters.priority_fee_parameters(
        &update_priority_fee_parameters_args,
        current_epoch,
        slots_per_epoch,
    );
    assert!(res.is_err());

    let update_priority_fee_parameters_args = UpdatePriorityFeeParametersArgs {
        priority_fee_lookback_epochs: Some(1),
        priority_fee_lookback_offset: Some(1),
        priority_fee_max_commission_bps: Some(1),
        priority_fee_error_margin_bps: Some(BASIS_POINTS_MAX + 1),
        priority_fee_scoring_start_epoch: Some(1),
    };
    let res = valid_parameters.priority_fee_parameters(
        &update_priority_fee_parameters_args,
        current_epoch,
        slots_per_epoch,
    );
    assert!(res.is_err());
}
