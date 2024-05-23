#![allow(clippy::await_holding_refcell_ref)]
use anchor_lang::{solana_program::instruction::Instruction, InstructionData, ToAccountMetas};
use solana_program_test::*;
use solana_sdk::{
    clock::Clock, compute_budget::ComputeBudgetInstruction, signer::Signer,
    transaction::Transaction, vote::state::MAX_EPOCH_CREDITS_HISTORY,
};
use tests::fixtures::{new_vote_account, TestFixture};
use validator_history::{ValidatorHistory, ValidatorHistoryEntry};

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

    // Skips epoch
    fixture.advance_num_epochs(2).await;
    let epoch_credits = vec![(0, 22, 10), (1, 35, 22), (2, 49, 35)];

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

    // Check that skipped epoch and new epoch entries + credits are added
    // But skipped epoch commission not added
    assert!(account.history.idx == 2);
    assert!(account.history.arr[2].epoch == 2);
    assert!(account.history.arr[1].epoch == 1);
    assert!(account.history.arr[0].epoch == 0);
    assert!(account.history.arr[2].commission == 8);
    assert!(account.history.arr[1].commission == ValidatorHistoryEntry::default().commission);
    assert!(account.history.arr[0].commission == 9);
    assert!(account.history.arr[2].epoch_credits == 14);
    assert!(account.history.arr[1].epoch_credits == 13);
    assert!(account.history.arr[0].epoch_credits == 12);
}

#[tokio::test]
async fn test_insert_missing_entries_compute() {
    // Initialize a ValidatorHistoryAccount with one entry for epoch 0, one entry for epoch 1000, and a vote account with 64 epochs of sparse credits in between
    // Expect that all 64 epochs of credits are copied over to the ValidatorHistoryAccount
    // Make sure we are within compute budget

    let fixture = TestFixture::new().await;
    let ctx = &fixture.ctx;
    fixture.initialize_config().await;
    fixture.initialize_validator_history_account().await;
    let initial_epoch_credits = vec![(0, 10, 0)];
    ctx.borrow_mut().set_account(
        &fixture.vote_account,
        &new_vote_account(
            fixture.vote_account,
            fixture.vote_account,
            9,
            Some(initial_epoch_credits),
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
        &[
            ComputeBudgetInstruction::set_compute_unit_limit(1_400_000),
            instruction,
        ],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        ctx.borrow().last_blockhash,
    );
    fixture.submit_transaction_assert_success(transaction).await;
    fixture.advance_num_epochs(1000).await;

    let initial_epoch_credits = vec![(0, 10, 0), (1000, 10, 0)];
    ctx.borrow_mut().set_account(
        &fixture.vote_account,
        &new_vote_account(
            fixture.vote_account,
            fixture.vote_account,
            9,
            Some(initial_epoch_credits),
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
    let blockhash = ctx.borrow_mut().get_new_latest_blockhash().await.unwrap();
    let transaction = Transaction::new_signed_with_payer(
        &[
            ComputeBudgetInstruction::set_compute_unit_limit(1_400_000),
            instruction,
        ],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        blockhash,
    );
    fixture.submit_transaction_assert_success(transaction).await;

    // Fake scenario: lots of new entries that were never picked up initially
    // Extreme case: validator votes once every 10 epochs
    let mut epoch_credits: Vec<(u64, u64, u64)> = vec![];
    for (i, epoch) in (1..MAX_EPOCH_CREDITS_HISTORY + 1).enumerate() {
        let i = i as u64;
        let epoch = epoch as u64;
        epoch_credits.push((epoch * 10, (i + 1) * 10, i * 10));
    }
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
    let blockhash = ctx.borrow_mut().get_new_latest_blockhash().await.unwrap();
    let transaction = Transaction::new_signed_with_payer(
        &[
            // Inserting 64 entries uses ~230k compute units, slightly above default
            ComputeBudgetInstruction::set_compute_unit_limit(300_000),
            instruction,
        ],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        blockhash,
    );
    fixture.submit_transaction_assert_success(transaction).await;

    let account: ValidatorHistory = fixture
        .load_and_deserialize(&fixture.validator_history_account)
        .await;

    // Check that all 64 epochs of credits were copied over and original entries were preserved
    let end_idx = MAX_EPOCH_CREDITS_HISTORY + 1;
    assert!(account.history.idx == end_idx as u64);
    for i in 1..end_idx {
        assert!(account.history.arr[i].epoch == 10 * i as u16);
        assert!(account.history.arr[i].epoch_credits == 10);
        assert!(account.history.arr[i].commission == ValidatorHistoryEntry::default().commission);
    }
    assert!(account.history.arr[0].epoch == 0);
    assert!(account.history.arr[0].epoch_credits == 10);
    assert!(account.history.arr[0].commission == 9);
    assert!(account.history.arr[end_idx].epoch == 1000);
    assert!(account.history.arr[end_idx].epoch_credits == 10);
    assert!(account.history.arr[end_idx].commission == 9);
    for i in end_idx + 1..ValidatorHistory::MAX_ITEMS {
        assert!(account.history.arr[i].epoch == ValidatorHistoryEntry::default().epoch);
        assert!(
            account.history.arr[i].epoch_credits == ValidatorHistoryEntry::default().epoch_credits
        );
        assert!(account.history.arr[i].commission == ValidatorHistoryEntry::default().commission);
    }

    drop(fixture);
}
