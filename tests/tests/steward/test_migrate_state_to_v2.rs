use anchor_lang::{Discriminator, InstructionData, ToAccountMetas};
use jito_steward::{StewardStateAccount, StewardStateAccountV2};
use solana_program_test::*;
use solana_sdk::signature::{Keypair, Signer};
use tests::steward_fixtures::TestFixture;

#[tokio::test]
async fn test_migrate_state_to_v2() {
    let fixture = TestFixture::new().await;
    fixture.initialize_stake_pool().await;
    // Config is initialized as part of steward initialization

    // Initialize with V1 state
    fixture.initialize_steward_v1(None, None).await;

    // Get the account before migration
    let account_before = fixture
        .load_and_deserialize::<StewardStateAccount>(&fixture.steward_state)
        .await;

    // Verify it has the V1 discriminator
    let account_raw_before = fixture.get_account(&fixture.steward_state).await;
    let account_data_before = account_raw_before.data;
    let discriminator_before = &account_data_before[0..8];
    assert_eq!(discriminator_before, StewardStateAccount::DISCRIMINATOR);

    // Record some values to verify they are preserved
    let state_tag_before = account_before.state.state_tag;
    let num_validators_before = account_before.state.num_pool_validators;
    let current_epoch_before = account_before.state.current_epoch;
    let next_cycle_epoch_before = account_before.state.next_cycle_epoch;

    // Perform the migration
    fixture.migrate_steward_state_to_v2().await;

    // Get the account after migration
    let account_after = fixture
        .load_and_deserialize::<StewardStateAccountV2>(&fixture.steward_state)
        .await;

    // Verify it has the V2 discriminator
    let account_after_raw = fixture.get_account(&fixture.steward_state).await;
    let account_data_after = account_after_raw.data;
    let discriminator_after = &account_data_after[0..8];
    assert_eq!(discriminator_after, StewardStateAccountV2::DISCRIMINATOR);
    assert_ne!(
        discriminator_before, discriminator_after,
        "Discriminator should have changed"
    );

    // Verify data was preserved correctly
    assert_eq!(account_after.state.state_tag, state_tag_before);
    assert_eq!(
        account_after.state.num_pool_validators,
        num_validators_before
    );
    assert_eq!(account_after.state.current_epoch, current_epoch_before);
    assert_eq!(
        account_after.state.next_cycle_epoch,
        next_cycle_epoch_before
    );

    // Verify scores were zero-extended from u32 to u64
    for i in 0..10 {
        assert_eq!(
            account_after.state.scores[i],
            account_before.state.scores[i] as u64
        );
        assert_eq!(
            account_after.state.raw_scores[i],
            account_before.state.yield_scores[i] as u64
        );
    }
}

#[tokio::test]
async fn test_migrate_state_to_v2_twice_fails() {
    let fixture = TestFixture::new().await;
    fixture.initialize_stake_pool().await;
    // Config is initialized as part of steward initialization

    // Initialize with V1 state
    fixture.initialize_steward_v1(None, None).await;

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

#[tokio::test]
async fn test_migrate_requires_admin() {
    let fixture = TestFixture::new().await;
    fixture.initialize_stake_pool().await;
    // Config is initialized as part of steward initialization
    fixture.initialize_steward_v1(None, None).await;

    // Try to migrate with a different signer
    let fake_admin = Keypair::new();
    fixture.ctx.borrow_mut().set_account(
        &fake_admin.pubkey(),
        &solana_sdk::account::Account {
            lamports: 1_000_000_000,
            ..Default::default()
        }
        .into(),
    );

    // This should fail due to permission check
    let instruction = solana_sdk::instruction::Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::MigrateStateToV2 {
            state_account: fixture.steward_state,
            config: fixture.steward_config.pubkey(),
            signer: fake_admin.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::MigrateStateToV2 {}.data(),
    };

    let transaction = solana_sdk::transaction::Transaction::new_signed_with_payer(
        &[instruction],
        Some(&fake_admin.pubkey()),
        &[&fake_admin],
        fixture.ctx.borrow().last_blockhash,
    );

    let result = fixture
        .ctx
        .borrow_mut()
        .banks_client
        .process_transaction(transaction)
        .await;
    assert!(result.is_err());
}
