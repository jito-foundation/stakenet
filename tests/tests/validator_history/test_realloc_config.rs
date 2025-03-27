use anchor_lang::{system_program, InstructionData, ToAccountMetas};
use solana_program_test::*;

use solana_sdk::{
    instruction::Instruction,
    signature::{Keypair, Signer},
    transaction::Transaction,
};
use test_case::test_case;
use tests::validator_history_fixtures::{system_account, TestFixture};
use validator_history::Config;

#[tokio::test]
async fn test_realloc_config_happy_path() {
    // init fixture
    let fixture = TestFixture::new().await;
    let ctx = &fixture.ctx;
    fixture.initialize_config().await;

    let config_account_data_before = ctx
        .borrow_mut()
        .banks_client
        .get_account(fixture.validator_history_config)
        .await
        .unwrap()
        .unwrap()
        .data;
    let config_before: Config = fixture
        .load_and_deserialize(&fixture.validator_history_config)
        .await;

    // TX to re-alloc the config account
    let new_size = 500;
    let instruction = Instruction {
        program_id: validator_history::id(),
        data: validator_history::instruction::ReallocConfigAccount { new_size }.data(),
        accounts: validator_history::accounts::ReallocConfigAccount {
            config_account: fixture.validator_history_config,
            system_program: system_program::ID,
            admin: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
    };

    let blockhash = ctx.borrow_mut().get_new_latest_blockhash().await.unwrap();
    let transaction = Transaction::new_signed_with_payer(
        &[instruction.clone()],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        blockhash,
    );

    fixture.submit_transaction_assert_success(transaction).await;

    // Validate that the config account has extra space.
    let config_account_data_after = ctx
        .borrow_mut()
        .banks_client
        .get_account(fixture.validator_history_config)
        .await
        .unwrap()
        .unwrap()
        .data;
    let config_after: Config = fixture
        .load_and_deserialize(&fixture.validator_history_config)
        .await;
    assert!(config_account_data_after.len() > config_account_data_before.len());
    assert_eq!(config_account_data_after.len(), new_size as usize);

    // Validate the config account data did not change
    assert_eq!(config_before.admin, config_after.admin);
    assert_eq!(
        config_before.oracle_authority,
        config_after.oracle_authority
    );
    assert_eq!(config_before.bump, config_after.bump);
    assert_eq!(config_before.counter, config_after.counter);
}

#[test_case(1_001, "AccountFullySized" ; "when expanding beyond 1,000 bytes")]
#[test_case((Config::SIZE - 1) as u64, "DeallocNotAllowed" ; "when trying to allocate for less space than Config::SIZE")]
#[tokio::test]
async fn test_realloc_config_bad_sizes(new_size: u64, expected_error: &str) {
    // init fixture
    let fixture = TestFixture::new().await;
    let ctx = &fixture.ctx;
    fixture.initialize_config().await;

    let instruction = Instruction {
        program_id: validator_history::id(),
        data: validator_history::instruction::ReallocConfigAccount { new_size }.data(),
        accounts: validator_history::accounts::ReallocConfigAccount {
            config_account: fixture.validator_history_config,
            system_program: system_program::ID,
            admin: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
    };

    let blockhash = ctx.borrow_mut().get_new_latest_blockhash().await.unwrap();
    let transaction = Transaction::new_signed_with_payer(
        &[instruction.clone()],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        blockhash,
    );
    fixture
        .submit_transaction_assert_error(transaction, expected_error)
        .await;
}

#[tokio::test]
async fn test_realloc_bad_admin() {
    // init fixture
    let fixture = TestFixture::new().await;
    let ctx = &fixture.ctx;
    fixture.initialize_config().await;

    // attempt update with wrong authority
    let new_authority = Keypair::new();
    ctx.borrow_mut()
        .set_account(&new_authority.pubkey(), &system_account(10000000).into());

    let new_size = 500;
    let instruction = Instruction {
        program_id: validator_history::id(),
        data: validator_history::instruction::ReallocConfigAccount { new_size }.data(),
        accounts: validator_history::accounts::ReallocConfigAccount {
            config_account: fixture.validator_history_config,
            system_program: system_program::ID,
            admin: new_authority.pubkey(),
        }
        .to_account_metas(None),
    };

    let blockhash = ctx.borrow_mut().get_new_latest_blockhash().await.unwrap();
    let transaction = Transaction::new_signed_with_payer(
        &[instruction.clone()],
        Some(&new_authority.pubkey()),
        &[&new_authority],
        blockhash,
    );
    fixture
        .submit_transaction_assert_error(transaction, "ConstraintHasOne")
        .await;
}
