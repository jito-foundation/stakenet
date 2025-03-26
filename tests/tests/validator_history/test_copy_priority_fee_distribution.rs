#![allow(clippy::await_holding_refcell_ref)]
use anchor_lang::{solana_program::instruction::Instruction, InstructionData, ToAccountMetas};

use solana_program_test::*;
use solana_sdk::{pubkey::Pubkey, signer::Signer, transaction::Transaction};
use tests::{
    priority_fee_distribution_helpers::derive_priority_fee_distribution_account_address,
    validator_history_fixtures::{new_priority_fee_distribution_account, TestFixture},
};
use validator_history::{
    Config, ValidatorHistory, ValidatorHistoryEntry,
};

#[tokio::test]
async fn test_priority_fee_commission() {
    let fixture = TestFixture::new().await;
    let ctx = &fixture.ctx;
    fixture.initialize_config().await;
    fixture.initialize_validator_history_account().await;

    let epoch = 0;
    let distribution_account = derive_priority_fee_distribution_account_address(
        &jito_priority_fee_distribution::id(),
        &fixture.vote_account,
        epoch,
    )
    .0;
    // No PriorityFees earned
    ctx.borrow_mut().set_account(
        &distribution_account,
        &new_priority_fee_distribution_account(fixture.vote_account, 42, None, Pubkey::default()).into(),
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
    assert!(account.history.arr[0].priority_fees_earned == ValidatorHistoryEntry::default().priority_fees_earned);

    ctx.borrow_mut().set_account(
        &distribution_account,
        &new_priority_fee_distribution_account(
            fixture.vote_account,
            42,
            Some(123_236_567_899),
            Pubkey::default(),
        )
        .into(),
    );

    let blockhash = ctx.borrow_mut().get_new_latest_blockhash().await.unwrap();
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        blockhash,
    );

    fixture.submit_transaction_assert_success(transaction).await;

    // assert Priority Fee earned no longer default
    let account: ValidatorHistory = fixture
        .load_and_deserialize(&fixture.validator_history_account)
        .await;
    assert!(account.history.arr[0].priority_fees_earned == 12324); // fixed point representation
    assert!((account.history.arr[0].priority_fees_earned as f64 / 100.0) == 123.24_f64);
}

#[tokio::test]
async fn test_priority_fee_commission_fail() {
    let fixture = TestFixture::new().await;
    let ctx = &fixture.ctx;
    fixture.initialize_config().await;
    fixture.initialize_validator_history_account().await;
    let epoch = 0;

    // test update Priority Fee commission with uninitialized TDA
    let distribution_account = derive_priority_fee_distribution_account_address(
        &jito_priority_fee_distribution::id(),
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
        .submit_transaction_assert_error(transaction, "ConstraintOwner")
        .await;

    fixture.advance_num_epochs(1).await;
    // test update Priority Fee commission with wrong epoch
    // note that just advancing the fixture's epoch cause a failure bc we relaxed the epoch constraints in the instruction/on distribution_account
    // explicitly pass the instruction a different epoch than the one used to generate the distribution pda
    let instruction = Instruction {
        program_id: validator_history::id(),
        data: validator_history::instruction::CopyPriorityFeeDistribution { epoch: 1 }.data(),
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
        &[instruction],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        ctx.borrow().last_blockhash,
    );
    fixture
        .submit_transaction_assert_error(transaction, "ConstraintSeeds")
        .await;

    let new_vote_account = Pubkey::new_unique();
    let distribution_account =
        derive_priority_fee_distribution_account_address(&jito_priority_fee_distribution::id(), &new_vote_account, 1)
            .0;
    ctx.borrow_mut().set_account(
        &distribution_account,
        &new_priority_fee_distribution_account(new_vote_account, 42, Some(123456), Pubkey::default()).into(),
    );

    // test update Priority Fee commission with wrong validator's TDA
    let instruction = Instruction {
        program_id: validator_history::id(),
        data: validator_history::instruction::CopyPriorityFeeDistribution { epoch }.data(),
        accounts: validator_history::accounts::CopyPriorityFeeDistribution {
            validator_history_account: fixture.validator_history_account,
            vote_account: new_vote_account,
            config: fixture.validator_history_config,
            distribution_account,
            signer: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
    };
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        ctx.borrow().last_blockhash,
    );

    fixture
        .submit_transaction_assert_error(transaction, "ConstraintSeeds")
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
