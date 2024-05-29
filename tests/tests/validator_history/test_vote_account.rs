#![allow(clippy::await_holding_refcell_ref)]
use anchor_lang::{
    solana_program::instruction::Instruction, AnchorSerialize, Discriminator, InstructionData,
    ToAccountMetas,
};
use solana_program_test::*;
use solana_sdk::{
    account::Account, clock::Clock, compute_budget::ComputeBudgetInstruction, signer::Signer,
    transaction::Transaction, vote::state::MAX_EPOCH_CREDITS_HISTORY,
};
use tests::validator_history_fixtures::{new_vote_account, TestFixture};
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

fn serialized_validator_history_account(validator_history: ValidatorHistory) -> Account {
    let mut data = vec![];
    validator_history.serialize(&mut data).unwrap();
    for byte in ValidatorHistory::discriminator().into_iter().rev() {
        data.insert(0, byte);
    }
    Account {
        lamports: 1_000_000_000,
        data,
        owner: validator_history::id(),
        ..Account::default()
    }
}

#[tokio::test]
async fn test_insert_missing_entries_wraparound() {
    // initialize validator history account with > 600 epochs of entries, missing one. This will force wraparound
    //
    // initialize vote account with 64 epochs, filling in the missing one

    let fixture = TestFixture::new().await;
    let ctx = &fixture.ctx;
    fixture.initialize_config().await;
    fixture.initialize_validator_history_account().await;

    fixture.advance_num_epochs(610).await;

    let mut validator_history: ValidatorHistory = fixture
        .load_and_deserialize(&fixture.validator_history_account)
        .await;

    // Fill in 600 epochs of entries, skipping 590
    // This will create wraparound, storing epochs 87 - 599
    for i in 0..600 {
        if i == 590 {
            continue;
        }
        validator_history.history.push(ValidatorHistoryEntry {
            epoch: i as u16,
            epoch_credits: 10,
            commission: 9,
            vote_account_last_update_slot: 0,
            ..Default::default()
        });
    }

    ctx.borrow_mut().set_account(
        &fixture.validator_history_account,
        &serialized_validator_history_account(validator_history).into(),
    );

    // New vote account with epochs 580 - 610
    // 11 credits per epoch
    let epoch_credits: Vec<(u64, u64, u64)> = (580..611).map(|i| (i, 11, 0)).collect();

    let vote_account = new_vote_account(
        fixture.vote_account,
        fixture.vote_account,
        10,
        Some(epoch_credits.clone()),
    );

    ctx.borrow_mut()
        .set_account(&fixture.vote_account, &vote_account.into());

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

    // 610 % 512 == 98
    assert_eq!(account.history.idx, 98);

    // Ensures that all entries exist, including missing 590
    // and entries 600 - 610 inserted after last entry
    // And that epoch credits were updated properly
    for i in account.history.idx as usize + 1..611 {
        let index = i % ValidatorHistory::MAX_ITEMS;
        assert_eq!(account.history.arr[index].epoch, i as u16,);
        if i >= 580 {
            assert_eq!(account.history.arr[index].epoch_credits, 11);
        } else {
            assert_eq!(account.history.arr[index].epoch_credits, 10);
        }
    }

    drop(fixture);
}
