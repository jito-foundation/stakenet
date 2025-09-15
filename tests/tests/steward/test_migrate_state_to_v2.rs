use anchor_lang::Discriminator;
use jito_steward::{constants::MAX_VALIDATORS, StewardStateAccount, StewardStateAccountV2};
use rand::{seq::SliceRandom, thread_rng, Rng};
use solana_program_test::*;
use tests::steward_fixtures::TestFixture;

#[tokio::test]
async fn test_migrate_state_to_v2() {
    let fixture = TestFixture::new().await;
    fixture.initialize_stake_pool().await;

    // Initialize with V1 state and realloc to full size
    fixture.initialize_steward_v1(None, None).await;
    fixture.realloc_steward_state().await;

    // Inject random test data into the V1 state fields that will be migrated
    {
        let mut rng = thread_rng();
        let mut account = fixture.get_account(&fixture.steward_state).await;

        // Calculate offsets (matching the migration logic)
        let base = 8usize;
        let size_state_enum = 1usize;
        let size_u64_array = 8 * MAX_VALIDATORS;
        let size_u32_array = 4 * MAX_VALIDATORS;
        let size_u16_array = 2 * MAX_VALIDATORS;

        let off_v1_balances = size_state_enum;
        let off_v1_scores = off_v1_balances + size_u64_array;
        let off_v1_sorted_score_indices = off_v1_scores + size_u32_array;
        let off_v1_yield_scores = off_v1_sorted_score_indices + size_u16_array;
        let off_v1_sorted_yield_score_indices = off_v1_yield_scores + size_u32_array;

        // Generate and write random scores
        for i in 0..MAX_VALIDATORS {
            let score = rng.gen_range(0, 1_000_000u32);
            let score_offset = base + off_v1_scores + i * 4;
            account.data[score_offset..score_offset + 4].copy_from_slice(&score.to_le_bytes());

            let yield_score = rng.gen_range(0, 500_000u32);
            let yield_score_offset = base + off_v1_yield_scores + i * 4;
            account.data[yield_score_offset..yield_score_offset + 4]
                .copy_from_slice(&yield_score.to_le_bytes());
        }

        // Generate and write shuffled indices
        let mut score_indices: Vec<u16> = (0..MAX_VALIDATORS as u16).collect();
        let mut yield_indices: Vec<u16> = (0..MAX_VALIDATORS as u16).collect();
        score_indices.shuffle(&mut rng);
        yield_indices.shuffle(&mut rng);

        for i in 0..MAX_VALIDATORS {
            let sorted_score_offset = base + off_v1_sorted_score_indices + i * 2;
            account.data[sorted_score_offset..sorted_score_offset + 2]
                .copy_from_slice(&score_indices[i].to_le_bytes());

            let sorted_yield_offset = base + off_v1_sorted_yield_score_indices + i * 2;
            account.data[sorted_yield_offset..sorted_yield_offset + 2]
                .copy_from_slice(&yield_indices[i].to_le_bytes());
        }

        // Update the account in the test context
        fixture
            .ctx
            .borrow_mut()
            .set_account(&fixture.steward_state, &account.into());
    }

    // Get the account before migration
    let steward_state_v1 = fixture
        .load_and_deserialize::<StewardStateAccount>(&fixture.steward_state)
        .await;

    // Verify it has the V1 discriminator
    let steward_state_v1_account = fixture.get_account(&fixture.steward_state).await;
    let steward_state_v1_account_data = steward_state_v1_account.data;
    let discriminator_v1 = &steward_state_v1_account_data[0..8];
    assert_eq!(discriminator_v1, StewardStateAccount::DISCRIMINATOR);

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

    // Verify V1 padding is all zeros
    for (i, &byte) in steward_state_v1.state._padding0.iter().enumerate() {
        assert_eq!(
            byte, 0,
            "V1 padding byte at index {} should be 0, but was {}",
            i, byte
        );
    }

    // Verify V2 padding is all zeros
    for (i, &byte) in steward_state_v2.state._padding0.iter().enumerate() {
        assert_eq!(
            byte, 0,
            "V2 padding byte at index {} should be 0, but was {}",
            i, byte
        );
    }
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
