use anchor_lang::Discriminator;
use jito_steward::{StewardStateAccount, StewardStateAccountV2};
use solana_program_test::*;
use tests::steward_fixtures::TestFixture;

#[tokio::test]
async fn test_migrate_state_to_v2() {
    let fixture = TestFixture::new().await;
    fixture.initialize_stake_pool().await;

    // Initialize with V1 state and realloc to full size
    fixture.initialize_steward_v1(None, None).await;
    fixture.realloc_steward_state().await;

    // Get the account before migration
    let steward_state_v1 = fixture
        .load_and_deserialize::<StewardStateAccount>(&fixture.steward_state)
        .await;

    // Verify it has the V1 discriminator
    let steward_state_v1_account = fixture.get_account(&fixture.steward_state).await;
    let steward_state_v1_account_data = steward_state_v1_account.data;
    let discriminator_v1 = &steward_state_v1_account_data[0..8];
    assert_eq!(discriminator_v1, StewardStateAccount::DISCRIMINATOR);

    // Record some values to verify they are preserved
    // let state_tag_v1 = steward_state_v1.state.state_tag;
    // let validator_lamport_balances_v1 = steward_state_v1.state.validator_lamport_balances;
    // let scores_v1 = steward_state_v1.state.scores;
    // let sorted_scores_indices_v1 = steward_state_v1.state.sorted_score_indices;
    // let yield_scores_v1 = steward_state_v1.state.yield_scores;
    // let sorted_yield_score_indices_v1 = steward_state_v1.state.sorted_yield_score_indices;
    // let delegations_v1 = steward_state_v1.state.delegations;
    // let instant_unstake_v1 = steward_state_v1.state.instant_unstake;
    // let progress_v1 = steward_state_v1.state.progress;
    // let validators_for_immediate_removal_v1 =
    //     steward_state_v1.state.validators_for_immediate_removal;
    // let validators_to_remove_v1 = steward_state_v1.state.validators_to_remove;
    // let start_computing_scores_slot_v1 = steward_state_v1.state.start_computing_scores_slot;
    // let current_epoch_v1 = steward_state_v1.state.current_epoch;
    // let next_cycle_epoch_v1 = steward_state_v1.state.next_cycle_epoch;
    // let num_pool_validators_v1 = steward_state_v1.state.num_pool_validators;
    // let scoring_unstake_total_v1 = steward_state_v1.state.scoring_unstake_total;
    // let instant_unstake_total_v1 = steward_state_v1.state.instant_unstake_total;
    // let stake_deposit_unstake_total_v1 = steward_state_v1.state.stake_deposit_unstake_total;
    // let status_flags_v1 = steward_state_v1.state.status_flags;
    // let validators_added_v1 = steward_state_v1.state.validators_added;

    // Perform the migration
    fixture.migrate_steward_state_to_v2().await;

    // Get the account after migration
    let steward_state_v2 = fixture
        .load_and_deserialize::<StewardStateAccountV2>(&fixture.steward_state)
        .await;

    // Verify it has the V2 discriminator
    let steward_state_v2_account = fixture.get_account(&fixture.steward_state).await;
    let steward_state_v2_account_data = steward_state_v2_account.data;
    let discriminator_v2 = &steward_state_v2_account_data[0..8];
    assert_eq!(discriminator_v2, StewardStateAccountV2::DISCRIMINATOR);
    assert_ne!(
        discriminator_v1, discriminator_v2,
        "Discriminator should have changed"
    );

    // State tag
    assert_eq!(
        steward_state_v1.state.state_tag,
        steward_state_v2.state.state_tag
    );
    // Validator lamport balances
    for (ix, el) in steward_state_v1
        .state
        .validator_lamport_balances
        .iter()
        .enumerate()
    {
        assert_eq!(*el, steward_state_v2.state.validator_lamport_balances[ix]);
    }
    // Scores
    for (ix, el) in steward_state_v1.state.scores.iter().enumerate() {
        assert_eq!((*el) as u64, steward_state_v2.state.scores[ix])
    }
    // Sorted score indices
    for (ix, el) in steward_state_v1
        .state
        .sorted_score_indices
        .iter()
        .enumerate()
    {
        assert_eq!(*el, steward_state_v2.state.sorted_score_indices[ix]);
    }
    // Yield scores -> Raw scores (expanded from u32 to u64)
    for (ix, el) in steward_state_v1.state.yield_scores.iter().enumerate() {
        assert_eq!((*el) as u64, steward_state_v2.state.raw_scores[ix]);
    }
    // Sorted yield score indices -> Sorted raw score indices
    for (ix, el) in steward_state_v1
        .state
        .sorted_yield_score_indices
        .iter()
        .enumerate()
    {
        assert_eq!(*el, steward_state_v2.state.sorted_raw_score_indices[ix]);
    }
    // Delegations
    for (ix, el) in steward_state_v1.state.delegations.iter().enumerate() {
        assert_eq!(*el, steward_state_v2.state.delegations[ix]);
    }
    // Instant unstake (BitMask - compare element by element)
    for i in 0..steward_state_v1.state.instant_unstake.values.len() {
        assert_eq!(
            steward_state_v1
                .state
                .instant_unstake
                .get(i)
                .unwrap_or(false),
            steward_state_v2
                .state
                .instant_unstake
                .get(i)
                .unwrap_or(false),
            "instant_unstake mismatch at index {}",
            i
        );
    }
    // Progress (BitMask - compare element by element)
    for i in 0..steward_state_v1.state.progress.values.len() {
        assert_eq!(
            steward_state_v1.state.progress.get(i).unwrap_or(false),
            steward_state_v2.state.progress.get(i).unwrap_or(false),
            "progress mismatch at index {}",
            i
        );
    }
    // Validators for immediate removal (BitMask - compare element by element)
    for i in 0..steward_state_v1
        .state
        .validators_for_immediate_removal
        .values
        .len()
    {
        assert_eq!(
            steward_state_v1
                .state
                .validators_for_immediate_removal
                .get(i)
                .unwrap_or(false),
            steward_state_v2
                .state
                .validators_for_immediate_removal
                .get(i)
                .unwrap_or(false),
            "validators_for_immediate_removal mismatch at index {}",
            i
        );
    }
    // Validators to remove (BitMask - compare element by element)
    for i in 0..steward_state_v1.state.validators_to_remove.values.len() {
        assert_eq!(
            steward_state_v1
                .state
                .validators_to_remove
                .get(i)
                .unwrap_or(false),
            steward_state_v2
                .state
                .validators_to_remove
                .get(i)
                .unwrap_or(false),
            "validators_to_remove mismatch at index {}",
            i
        );
    }
    // Start computing scores slot
    assert_eq!(
        steward_state_v1.state.start_computing_scores_slot,
        steward_state_v2.state.start_computing_scores_slot
    );
    // Current epoch
    assert_eq!(
        steward_state_v1.state.current_epoch,
        steward_state_v2.state.current_epoch
    );
    // Next cycle epoch
    assert_eq!(
        steward_state_v1.state.next_cycle_epoch,
        steward_state_v2.state.next_cycle_epoch
    );
    // Num pool validators
    assert_eq!(
        steward_state_v1.state.num_pool_validators,
        steward_state_v2.state.num_pool_validators
    );
    // Scoring unstake total
    assert_eq!(
        steward_state_v1.state.scoring_unstake_total,
        steward_state_v2.state.scoring_unstake_total
    );
    // Instant unstake total
    assert_eq!(
        steward_state_v1.state.instant_unstake_total,
        steward_state_v2.state.instant_unstake_total
    );
    // Stake deposit unstake total
    assert_eq!(
        steward_state_v1.state.stake_deposit_unstake_total,
        steward_state_v2.state.stake_deposit_unstake_total
    );
    // Status flags
    assert_eq!(
        steward_state_v1.state.status_flags,
        steward_state_v2.state.status_flags
    );
    // Validators added
    assert_eq!(
        steward_state_v1.state.validators_added,
        steward_state_v2.state.validators_added
    );
}

#[tokio::test]
async fn test_migrate_state_to_v2_twice_fails() {
    let fixture = TestFixture::new().await;
    fixture.initialize_stake_pool().await;
    // Config is initialized as part of steward initialization

    // Initialize with V1 state and realloc to full size
    fixture.initialize_steward_v1(None, None).await;
    fixture.realloc_steward_state().await;

    // First migration should succeed
    fixture.migrate_steward_state_to_v2().await;

    // Second migration should fail because discriminator is now V2
    let result = fixture.try_migrate_steward_state_to_v2().await;
    assert!(result.is_err());

    // Verify the error is what we expect
    match result {
        Err(e) => {
            // The migration should fail with InvalidAccountData error
            println!("Expected error: {:?}", e);
        }
        Ok(_) => panic!("Migration should have failed the second time"),
    }
}
