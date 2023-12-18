#![allow(clippy::await_holding_refcell_ref)]
use anchor_lang::{solana_program::instruction::Instruction, InstructionData, ToAccountMetas};
use solana_program_test::*;
use solana_sdk::{clock::Clock, signer::Signer, transaction::Transaction};
use tests::fixtures::{new_vote_account, TestFixture};
use validator_history::ValidatorHistory;

#[tokio::test]
async fn test_copy_vote_account() {
    // Initialize
    let fixture = TestFixture::new().await;
    let ctx = &fixture.ctx;
    fixture.initialize_config().await;
    fixture.initialize_validator_history_account().await;

    // Set with specific epoch credits, commission, last update slot
    let epoch_credits = vec![(0, 20, 10)];
    ctx.borrow_mut().set_account(
        &fixture.vote_account,
        &new_vote_account(
            fixture.vote_account,
            fixture.vote_account,
            9,
            Some(epoch_credits),
        )
        .into(),
    );

    let instruction = Instruction {
        program_id: validator_history::id(),
        data: validator_history::instruction::CopyVoteAccount {}.data(),
        accounts: validator_history::accounts::CopyVoteAccount {
            validator_history_account: fixture.validator_history_account,
            vote_account: fixture.vote_account,
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

    fixture.submit_transaction_assert_success(transaction).await;

    let account: ValidatorHistory = fixture
        .load_and_deserialize(&fixture.validator_history_account)
        .await;

    let clock: Clock = ctx
        .borrow_mut()
        .banks_client
        .get_sysvar()
        .await
        .expect("clock");
    assert!(clock.epoch == 0);

    assert!(account.history.idx == 0);
    assert!(account.history.arr[0].epoch == 0);
    assert!(account.history.arr[0].vote_account_last_update_slot <= clock.slot);
    assert!(account.history.arr[0].epoch_credits == 10);
    assert!(account.history.arr[0].commission == 9);

    fixture.advance_num_epochs(2).await;

    let epoch_credits = vec![(0, 22, 10), (1, 34, 22), (2, 46, 34)];

    ctx.borrow_mut().set_account(
        &fixture.vote_account,
        &new_vote_account(
            fixture.vote_account,
            fixture.vote_account,
            8,
            Some(epoch_credits),
        )
        .into(),
    );

    let instruction = Instruction {
        program_id: validator_history::id(),
        data: validator_history::instruction::CopyVoteAccount {}.data(),
        accounts: validator_history::accounts::CopyVoteAccount {
            validator_history_account: fixture.validator_history_account,
            vote_account: fixture.vote_account,
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

    fixture.submit_transaction_assert_success(transaction).await;

    let account: ValidatorHistory = fixture
        .load_and_deserialize(&fixture.validator_history_account)
        .await;

    // check new epoch 0 values get copied over, but epoch 1 should be skipped
    assert!(account.history.idx == 1);
    assert!(account.history.arr[1].epoch == 2);
    assert!(account.history.arr[1].commission == 8);
    assert!(account.history.arr[1].epoch_credits == 12);
    assert!(account.history.arr[0].epoch_credits == 12);
}
