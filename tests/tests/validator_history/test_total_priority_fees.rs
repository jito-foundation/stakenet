#![allow(clippy::await_holding_refcell_ref)]
use anchor_lang::{solana_program::instruction::Instruction, InstructionData, ToAccountMetas};
use solana_program_test::*;
use solana_sdk::{clock::Clock, signature::Keypair, signer::Signer, transaction::Transaction};
use tests::validator_history_fixtures::{system_account, TestFixture};
use validator_history::ValidatorHistory;

#[tokio::test]
async fn test_priority_fee_history_basic_update() {
    // init fixture
    let fixture = TestFixture::new().await;
    let ctx = &fixture.ctx;
    fixture.initialize_config().await;
    fixture.initialize_validator_history_account().await;

    // update priority_fee history
    let total_priority_fees: u64 = 20000;
    let instruction = Instruction {
        program_id: validator_history::id(),
        data: validator_history::instruction::UpdatePriorityFeeHistory {
            epoch: 0,
            lamports: total_priority_fees,
        }
        .data(),
        accounts: validator_history::accounts::UpdatePriorityFeeHistory {
            validator_history_account: fixture.validator_history_account,
            vote_account: fixture.vote_account,
            config: fixture.validator_history_config,
            oracle_authority: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
    };
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        ctx.borrow().last_blockhash,
    );
    fixture.submit_transaction_assert_success(transaction).await;

    // assert value
    let account: ValidatorHistory = fixture
        .load_and_deserialize(&fixture.validator_history_account)
        .await;
    assert_eq!(account.history.idx, 0);
    assert_eq!(account.history.arr[0].epoch, 0);
    assert_eq!(
        account.history.arr[0].total_priority_fees,
        total_priority_fees
    );

    // sleep 2 epochs, wait again
    fixture.advance_num_epochs(2).await;

    let instruction = Instruction {
        program_id: validator_history::id(),
        data: validator_history::instruction::UpdatePriorityFeeHistory {
            epoch: 2,
            lamports: total_priority_fees + 1,
        }
        .data(),
        accounts: validator_history::accounts::UpdatePriorityFeeHistory {
            validator_history_account: fixture.validator_history_account,
            vote_account: fixture.vote_account,
            config: fixture.validator_history_config,
            oracle_authority: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
    };
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        ctx.borrow().last_blockhash,
    );
    fixture.submit_transaction_assert_success(transaction).await;

    // assert value
    let account: ValidatorHistory = fixture
        .load_and_deserialize(&fixture.validator_history_account)
        .await;
    assert_eq!(account.history.idx, 1);
    assert_eq!(account.history.arr[1].epoch, 2);
    assert_eq!(
        account.history.arr[1].total_priority_fees,
        total_priority_fees + 1
    );
}

#[tokio::test]
async fn test_priority_fee_history_wrong_authority() {
    // init fixture
    let fixture = TestFixture::new().await;
    let ctx = &fixture.ctx;
    fixture.initialize_config().await;
    fixture.initialize_validator_history_account().await;

    // attempt update with wrong authority
    let new_authority = Keypair::new();
    ctx.borrow_mut()
        .set_account(&new_authority.pubkey(), &system_account(10000000).into());

    let total_priority_fees = 1000;
    let instruction = Instruction {
        program_id: validator_history::id(),
        data: validator_history::instruction::UpdatePriorityFeeHistory {
            epoch: 0,
            lamports: total_priority_fees,
        }
        .data(),
        accounts: validator_history::accounts::UpdatePriorityFeeHistory {
            validator_history_account: fixture.validator_history_account,
            vote_account: fixture.vote_account,
            config: fixture.validator_history_config,
            oracle_authority: new_authority.pubkey(),
        }
        .to_account_metas(None),
    };
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&new_authority.pubkey()),
        &[&new_authority],
        ctx.borrow().last_blockhash,
    );

    fixture
        .submit_transaction_assert_error(transaction, "ConstraintHasOne")
        .await;
}

#[tokio::test]
async fn test_priority_fee_history_future_epoch() {
    // init fixture
    let fixture = TestFixture::new().await;
    let ctx = &fixture.ctx;
    fixture.initialize_config().await;
    fixture.initialize_validator_history_account().await;

    let clock: Clock = ctx
        .borrow_mut()
        .banks_client
        .get_sysvar()
        .await
        .expect("Failed getting clock");
    // attempt update with future epoch
    let total_priority_fees = 1000;
    let instruction = Instruction {
        program_id: validator_history::id(),
        data: validator_history::instruction::UpdatePriorityFeeHistory {
            epoch: clock.epoch + 1,
            lamports: total_priority_fees,
        }
        .data(),
        accounts: validator_history::accounts::UpdatePriorityFeeHistory {
            validator_history_account: fixture.validator_history_account,
            vote_account: fixture.vote_account,
            config: fixture.validator_history_config,
            oracle_authority: fixture.keypair.pubkey(),
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
        .submit_transaction_assert_error(transaction, "EpochOutOfRange")
        .await;
}
