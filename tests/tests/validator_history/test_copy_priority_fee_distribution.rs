#![allow(clippy::await_holding_refcell_ref)]
use anchor_lang::{solana_program::instruction::Instruction, InstructionData, ToAccountMetas};

use solana_program_test::*;
use solana_sdk::{pubkey::Pubkey, signer::Signer, transaction::Transaction};
use std::str::FromStr;
use tests::{
    priority_fee_distribution_helpers::derive_priority_fee_distribution_account_address,
    validator_history_fixtures::{new_priority_fee_distribution_account, TestFixture},
};
use validator_history::{Config, MerkleRootUploadAuthority, ValidatorHistory};

const TIP_ROUTER_AUTHORITY: &str = "8F4jGUmxF36vQ6yabnsxX6AQVXdKBhs8kGSUuRKSg8Xt";

#[tokio::test]
async fn test_priority_fee_distribution_account_does_not_exist() {
    let fixture = TestFixture::new().await;
    let ctx = &fixture.ctx;
    fixture.initialize_config().await;
    fixture.advance_num_epochs(1).await;
    fixture.initialize_validator_history_account().await;

    let epoch = 0;
    let distribution_account = derive_priority_fee_distribution_account_address(
        &jito_priority_fee_distribution::id(),
        &fixture.vote_account,
        epoch,
    )
    .0;

    // Account does not exist on-chain

    let instruction = Instruction {
        program_id: validator_history::id(),
        data: validator_history::instruction::CopyPriorityFeeDistribution { epoch }.data(),
        accounts: validator_history::accounts::CopyPriorityFeeDistribution {
            validator_history_account: fixture.validator_history_account,
            vote_account: fixture.vote_account,
            config: fixture.validator_history_config,
            distribution_account,
            signer: fixture.keypair.pubkey(),
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

    let account: ValidatorHistory = fixture
        .load_and_deserialize(&fixture.validator_history_account)
        .await;
    assert!(account.history.idx == 0);
    assert!(account.history.arr[0].epoch == 0);
    assert!(account.history.arr[0].priority_fee_commission == 0);
    assert!(account.history.arr[0].priority_fee_tips == 0);
    assert!(
        account.history.arr[0].priority_fee_merkle_root_upload_authority
            == MerkleRootUploadAuthority::DNE
    );
}

#[tokio::test]
async fn test_priority_fee_commission_none_earned() {
    let fixture = TestFixture::new().await;
    let ctx = &fixture.ctx;
    fixture.initialize_config().await;
    fixture.advance_num_epochs(1).await;
    fixture.initialize_validator_history_account().await;

    let epoch = 0;
    let tda_merkle_root_upload_authority = Pubkey::from_str(TIP_ROUTER_AUTHORITY).unwrap();
    let distribution_account = derive_priority_fee_distribution_account_address(
        &jito_priority_fee_distribution::id(),
        &fixture.vote_account,
        epoch,
    )
    .0;
    // No PriorityFees earned
    ctx.borrow_mut().set_account(
        &distribution_account,
        &new_priority_fee_distribution_account(
            fixture.vote_account,
            42,
            None,
            tda_merkle_root_upload_authority,
        )
        .into(),
    );

    // update priority fee commission
    let instruction = Instruction {
        program_id: validator_history::id(),
        data: validator_history::instruction::CopyPriorityFeeDistribution { epoch }.data(),
        accounts: validator_history::accounts::CopyPriorityFeeDistribution {
            validator_history_account: fixture.validator_history_account,
            vote_account: fixture.vote_account,
            config: fixture.validator_history_config,
            distribution_account,
            signer: fixture.keypair.pubkey(),
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

    // assert values, Priority Fee earned default
    let account: ValidatorHistory = fixture
        .load_and_deserialize(&fixture.validator_history_account)
        .await;
    assert!(account.history.idx == 0);
    assert!(account.history.arr[0].epoch == 0);
    assert!(account.history.arr[0].priority_fee_commission == 42);
    assert!(account.history.arr[0].priority_fee_tips == 0);
    assert!(
        account.history.arr[0].priority_fee_merkle_root_upload_authority
            == MerkleRootUploadAuthority::TipRouter
    );
}

#[tokio::test]
async fn test_priority_fee_commission_earned() {
    let fixture = TestFixture::new().await;
    let ctx = &fixture.ctx;
    fixture.initialize_config().await;
    fixture.advance_num_epochs(1).await;
    fixture.initialize_validator_history_account().await;

    let epoch = 0;
    let tda_merkle_root_upload_authority = Pubkey::from_str(TIP_ROUTER_AUTHORITY).unwrap();
    let priority_fees_earned: u64 = 123_236_567_899;
    let distribution_account = derive_priority_fee_distribution_account_address(
        &jito_priority_fee_distribution::id(),
        &fixture.vote_account,
        epoch,
    )
    .0;
    // No PriorityFees earned
    ctx.borrow_mut().set_account(
        &distribution_account,
        &new_priority_fee_distribution_account(
            fixture.vote_account,
            42,
            Some(priority_fees_earned),
            tda_merkle_root_upload_authority,
        )
        .into(),
    );

    // update priority fee commission
    let instruction = Instruction {
        program_id: validator_history::id(),
        data: validator_history::instruction::CopyPriorityFeeDistribution { epoch }.data(),
        accounts: validator_history::accounts::CopyPriorityFeeDistribution {
            validator_history_account: fixture.validator_history_account,
            vote_account: fixture.vote_account,
            config: fixture.validator_history_config,
            distribution_account,
            signer: fixture.keypair.pubkey(),
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

    // assert values, Priority Fee earned default
    let account: ValidatorHistory = fixture
        .load_and_deserialize(&fixture.validator_history_account)
        .await;
    assert!(account.history.idx == 0);
    assert!(account.history.arr[0].epoch == 0);
    assert!(account.history.arr[0].priority_fee_commission == 42);
    assert!(account.history.arr[0].priority_fee_tips == priority_fees_earned);
    assert!(
        account.history.arr[0].priority_fee_merkle_root_upload_authority
            == MerkleRootUploadAuthority::TipRouter
    );
}

#[tokio::test]
async fn test_priority_fee_commission_fail() {
    let fixture = TestFixture::new().await;
    let ctx = &fixture.ctx;
    fixture.initialize_config().await;
    fixture.initialize_validator_history_account().await;
    let epoch = 0;

    // test update Priority Fee commission with incorrect PDA
    let distribution_account = derive_priority_fee_distribution_account_address(
        &Pubkey::new_unique(),
        &fixture.vote_account,
        epoch,
    )
    .0;
    let instruction = Instruction {
        program_id: validator_history::id(),
        data: validator_history::instruction::CopyPriorityFeeDistribution { epoch }.data(),
        accounts: validator_history::accounts::CopyPriorityFeeDistribution {
            validator_history_account: fixture.validator_history_account,
            vote_account: fixture.vote_account,
            config: fixture.validator_history_config,
            distribution_account,
            signer: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
    };
    let transaction = Transaction::new_signed_with_payer(
        &[instruction.clone()],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        ctx.borrow().last_blockhash,
    );
    fixture
        .submit_transaction_assert_error(transaction, "ConstraintSeeds")
        .await;
}

#[tokio::test]
async fn test_priority_fee_commission_fail_double_copy() {
    let fixture = TestFixture::new().await;
    let ctx = &fixture.ctx;
    fixture.initialize_config().await;
    fixture.advance_num_epochs(1).await;
    fixture.initialize_validator_history_account().await;

    let epoch = 0;
    let priority_fees_earned: u64 = 123_236_567_899;
    let tda_merkle_root_upload_authority = Pubkey::from_str(TIP_ROUTER_AUTHORITY).unwrap();
    let distribution_account = derive_priority_fee_distribution_account_address(
        &jito_priority_fee_distribution::id(),
        &fixture.vote_account,
        epoch,
    )
    .0;

    ctx.borrow_mut().set_account(
        &distribution_account,
        &new_priority_fee_distribution_account(
            fixture.vote_account,
            42,
            Some(priority_fees_earned),
            tda_merkle_root_upload_authority,
        )
        .into(),
    );

    let instruction = Instruction {
        program_id: validator_history::id(),
        data: validator_history::instruction::CopyPriorityFeeDistribution { epoch }.data(),
        accounts: validator_history::accounts::CopyPriorityFeeDistribution {
            validator_history_account: fixture.validator_history_account,
            vote_account: fixture.vote_account,
            config: fixture.validator_history_config,
            distribution_account,
            signer: fixture.keypair.pubkey(),
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
        .submit_transaction_assert_success(transaction.clone())
        .await;

    let blockhash = ctx.borrow_mut().get_new_latest_blockhash().await.unwrap();

    let transaction = Transaction::new_signed_with_payer(
        &[instruction.clone()],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        blockhash,
    );

    fixture
        .submit_transaction_assert_error(transaction, "PriorityFeeDistributionAccountAlreadyCopied")
        .await;
}

// Test change validator history authority
#[tokio::test]
async fn test_change_priority_fee_distribution_authority() {
    let fixture = TestFixture::new().await;
    let ctx = &fixture.ctx;

    fixture.initialize_config().await;

    let new_authority = Pubkey::new_unique();

    // Change Validator history authority
    let instruction = Instruction {
        program_id: validator_history::id(),
        accounts: validator_history::accounts::SetNewAdmin {
            config: fixture.validator_history_config,
            new_admin: new_authority,
            admin: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: validator_history::instruction::SetNewAdmin {}.data(),
    };
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        ctx.borrow().last_blockhash,
    );
    fixture.submit_transaction_assert_success(transaction).await;

    // Assert new authority
    let config: Config = fixture
        .load_and_deserialize(&fixture.validator_history_config)
        .await;

    assert!(config.admin == new_authority);

    // Change validator history authority
    let instruction = Instruction {
        program_id: validator_history::id(),
        accounts: validator_history::accounts::SetNewAdmin {
            config: fixture.validator_history_config,
            new_admin: fixture.keypair.pubkey(),
            admin: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: validator_history::instruction::SetNewAdmin {}.data(),
    };

    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        ctx.borrow().last_blockhash,
    );
    fixture
        .submit_transaction_assert_error(transaction, "ConstraintHasOne.")
        .await;
}
