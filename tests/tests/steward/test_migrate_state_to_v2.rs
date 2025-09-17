use anchor_lang::Discriminator;
use jito_steward::{
    constants::MAX_VALIDATORS, utils::U8Bool, StewardStateAccount, StewardStateAccountV2,
};
use rand::{seq::SliceRandom, thread_rng, Rng};
use solana_program_test::*;
use tests::steward_fixtures::{serialized_steward_state_account_v1, TestFixture};

/// Holds the random test data we generate for migration testing
struct RandomTestData {
    scores: Vec<u32>,
    yield_scores: Vec<u32>,
    sorted_score_indices: Vec<u16>,
    sorted_yield_score_indices: Vec<u16>,
}

#[tokio::test]
async fn test_migrate_state_to_v2() {
    let fixture = TestFixture::new().await;
    fixture.initialize_stake_pool().await;

    // Initialize with V1 state and realloc to full size
    fixture.initialize_steward_v1(None, None).await;
    fixture.realloc_steward_state().await;

    // Generate random test data
    let mut rng = thread_rng();
    let random_data = RandomTestData {
        scores: (0..MAX_VALIDATORS)
            .map(|_| rng.gen_range(1, 1_000_000u32))
            .collect(),
        yield_scores: (0..MAX_VALIDATORS)
            .map(|_| rng.gen_range(1, 500_000u32))
            .collect(),
        sorted_score_indices: {
            let mut indices: Vec<u16> = (0..MAX_VALIDATORS as u16).collect();
            indices.shuffle(&mut rng);
            indices
        },
        sorted_yield_score_indices: {
            let mut indices: Vec<u16> = (0..MAX_VALIDATORS as u16).collect();
            indices.shuffle(&mut rng);
            indices
        },
    };

    // Load the V1 state and update it with our random values
    let mut steward_state_v1 = fixture
        .load_and_deserialize::<StewardStateAccount>(&fixture.steward_state)
        .await;

    // Update the V1 state with our random values
    for i in 0..MAX_VALIDATORS {
        steward_state_v1.state.scores[i] = random_data.scores[i];
        steward_state_v1.state.yield_scores[i] = random_data.yield_scores[i];
        steward_state_v1.state.sorted_score_indices[i] = random_data.sorted_score_indices[i];
        steward_state_v1.state.sorted_yield_score_indices[i] =
            random_data.sorted_yield_score_indices[i];
    }

    // Update the account
    fixture.ctx.borrow_mut().set_account(
        &fixture.steward_state,
        &serialized_steward_state_account_v1(steward_state_v1).into(),
    );

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
    // Scores - compare against both V1 and random data
    for (ix, el) in steward_state_v1.state.scores.iter().enumerate() {
        // Compare V2 against V1
        assert_eq!(
            (*el) as u64,
            steward_state_v2.state.scores[ix],
            "Score mismatch at index {}: V1={}, V2={}",
            ix,
            el,
            steward_state_v2.state.scores[ix]
        );
        // Additionally compare V2 against original random data
        assert_eq!(
            random_data.scores[ix] as u64, steward_state_v2.state.scores[ix],
            "Score mismatch at index {}: random={}, V2={}",
            ix, random_data.scores[ix], steward_state_v2.state.scores[ix]
        );
    }
    // Sorted score indices - compare against both V1 and random data
    for (ix, el) in steward_state_v1
        .state
        .sorted_score_indices
        .iter()
        .enumerate()
    {
        // Compare V2 against V1
        assert_eq!(
            *el, steward_state_v2.state.sorted_score_indices[ix],
            "Sorted score index mismatch at {}: V1={}, V2={}",
            ix, el, steward_state_v2.state.sorted_score_indices[ix]
        );
        // Additionally compare V2 against original random data
        assert_eq!(
            random_data.sorted_score_indices[ix],
            steward_state_v2.state.sorted_score_indices[ix],
            "Sorted score index mismatch at {}: random={}, V2={}",
            ix,
            random_data.sorted_score_indices[ix],
            steward_state_v2.state.sorted_score_indices[ix]
        );
    }
    // Yield scores -> Raw scores (expanded from u32 to u64) - compare against both V1 and random data
    for (ix, el) in steward_state_v1.state.yield_scores.iter().enumerate() {
        // Compare V2 against V1
        assert_eq!(
            (*el) as u64,
            steward_state_v2.state.raw_scores[ix],
            "Raw score mismatch at index {}: V1_yield={}, V2_raw={}",
            ix,
            el,
            steward_state_v2.state.raw_scores[ix]
        );
        // Additionally compare V2 against original random data
        assert_eq!(
            random_data.yield_scores[ix] as u64, steward_state_v2.state.raw_scores[ix],
            "Raw score mismatch at index {}: random_yield={}, V2_raw={}",
            ix, random_data.yield_scores[ix], steward_state_v2.state.raw_scores[ix]
        );
    }
    // Sorted yield score indices -> Sorted raw score indices - compare against both V1 and random data
    for (ix, el) in steward_state_v1
        .state
        .sorted_yield_score_indices
        .iter()
        .enumerate()
    {
        // Compare V2 against V1
        assert_eq!(
            *el, steward_state_v2.state.sorted_raw_score_indices[ix],
            "Sorted raw score index mismatch at {}: V1_yield={}, V2_raw={}",
            ix, el, steward_state_v2.state.sorted_raw_score_indices[ix]
        );
        // Additionally compare V2 against original random data
        assert_eq!(
            random_data.sorted_yield_score_indices[ix],
            steward_state_v2.state.sorted_raw_score_indices[ix],
            "Sorted raw score index mismatch at {}: random_yield={}, V2_raw={}",
            ix,
            random_data.sorted_yield_score_indices[ix],
            steward_state_v2.state.sorted_raw_score_indices[ix]
        );
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

    // Verify v1 is_initialized vs v2 _padding0
    assert_eq!(steward_state_v1.is_initialized, U8Bool::from(true));
    assert_eq!(
        steward_state_v2._padding0, 0,
        "V2 _padding0 should be zeroed out"
    );

    // Verify bump field is preserved
    assert_eq!(
        steward_state_v1.bump, steward_state_v2.bump,
        "Bump should be preserved during migration"
    );

    // Verify v1 _padding vs v2 _padding1
    for (i, &byte) in steward_state_v1._padding.iter().enumerate() {
        assert_eq!(
            byte, steward_state_v2._padding1[i],
            "V1 _padding[{}] should equal V2 _padding1[{}]",
            i, i
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
