/// Basic integration test
use anchor_lang::{solana_program::instruction::Instruction, InstructionData, ToAccountMetas};
use jito_steward::{Config, UpdateParametersArgs};
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
            mev_commission_range: None,
            epoch_credits_range: None,
            commission_range: None,
            scoring_delinquency_threshold_ratio: None,
            instant_unstake_delinquency_threshold_ratio: None,
            mev_commission_bps_threshold: None,
            commission_threshold: None,
            num_delegation_validators: None,
            scoring_unstake_cap_bps: None,
            instant_unstake_cap_bps: None,
            stake_deposit_unstake_cap_bps: None,
            instant_unstake_epoch_progress: None,
            compute_score_slot_range: None,
            instant_unstake_inputs_epoch_progress: None,
            num_epochs_between_scoring: None,
            minimum_stake_lamports: None,
            minimum_voting_epochs: None,
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
