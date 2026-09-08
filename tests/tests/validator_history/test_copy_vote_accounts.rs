#![allow(clippy::await_holding_refcell_ref)]
use anchor_lang::{solana_program::instruction::Instruction, InstructionData, ToAccountMetas};
use solana_program_test::*;
use solana_sdk::{
    clock::Clock, compute_budget::ComputeBudgetInstruction, instruction::AccountMeta,
    pubkey::Pubkey, signer::Signer, transaction::Transaction,
};
use tests::validator_history_fixtures::{new_vote_account, system_account, TestFixture};
use validator_history::{constants::MAX_ALLOC_BYTES, ValidatorHistory};

/// Helper: initialize (and fully realloc) a ValidatorHistory account for an arbitrary
/// vote account, using the fixture's signer. Mirrors the fixture's single-account setup.
async fn init_validator_history(
    fixture: &TestFixture,
    vote_account: &Pubkey,
    validator_history_account: &Pubkey,
) {
    let init_ix = Instruction {
        program_id: validator_history::id(),
        accounts: validator_history::accounts::InitializeValidatorHistoryAccount {
            validator_history_account: *validator_history_account,
            vote_account: *vote_account,
            system_program: anchor_lang::solana_program::system_program::id(),
            signer: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: validator_history::instruction::InitializeValidatorHistoryAccount {}.data(),
    };

    let mut ixs = vec![init_ix];
    let num_reallocs = (ValidatorHistory::SIZE - MAX_ALLOC_BYTES) / MAX_ALLOC_BYTES + 1;
    ixs.extend(vec![
        Instruction {
            program_id: validator_history::id(),
            accounts: validator_history::accounts::ReallocValidatorHistoryAccount {
                validator_history_account: *validator_history_account,
                vote_account: *vote_account,
                config: fixture.validator_history_config,
                system_program: anchor_lang::solana_program::system_program::id(),
                signer: fixture.keypair.pubkey(),
            }
            .to_account_metas(None),
            data: validator_history::instruction::ReallocValidatorHistoryAccount {}.data(),
        };
        num_reallocs
    ]);

    let transaction = Transaction::new_signed_with_payer(
        &ixs,
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        fixture.ctx.borrow().last_blockhash,
    );
    fixture.submit_transaction_assert_success(transaction).await;
}

/// Sets up `count` validators, each with its own vote account and initialized
/// ValidatorHistory PDA. Returns (vote_account, validator_history_account) pairs.
async fn setup_validators(fixture: &TestFixture, count: usize) -> Vec<(Pubkey, Pubkey)> {
    let mut validators = Vec::with_capacity(count);
    let identity = fixture.identity_keypair.pubkey();

    for _ in 0..count {
        let vote_account = Pubkey::new_unique();
        let (validator_history_account, _) = Pubkey::find_program_address(
            &[ValidatorHistory::SEED, vote_account.as_ref()],
            &validator_history::id(),
        );

        // Vote account must exist with >= MIN_VOTE_EPOCHS epochs for initialization.
        fixture.ctx.borrow_mut().set_account(
            &vote_account,
            &new_vote_account(identity, vote_account, 1, Some(vec![(0, 0, 0); 10])).into(),
        );

        init_validator_history(fixture, &vote_account, &validator_history_account).await;
        validators.push((vote_account, validator_history_account));
    }

    validators
}

/// The core behavior: one `copy_vote_accounts` instruction updates many
/// ValidatorHistory accounts, producing exactly the same result as issuing one
/// `copy_vote_account` per validator would.
#[tokio::test]
async fn test_copy_vote_accounts_bulk() {
    let fixture = TestFixture::new().await;
    let ctx = &fixture.ctx;
    fixture.initialize_config().await;

    let count = 4usize;
    let validators = setup_validators(&fixture, count).await;
    let identity = fixture.identity_keypair.pubkey();

    // Give each validator distinct commission and credits so we can prove there is no
    // cross-contamination between accounts in the batch.
    // Stored epoch_credits for epoch 0 == (credits - prev_credits).
    for (i, (vote_account, _)) in validators.iter().enumerate() {
        let commission = (3 + i) as u8;
        let credits = 20 + (i as u64) * 5; // stored credits == 10 + i*5
        ctx.borrow_mut().set_account(
            vote_account,
            &new_vote_account(
                identity,
                *vote_account,
                commission,
                Some(vec![(0, credits, 10)]),
            )
            .into(),
        );
    }

    // Build a single bulk instruction. Accounts: [signer, vh_0, vote_0, vh_1, vote_1, ...]
    let mut accounts = validator_history::accounts::CopyVoteAccounts {
        signer: fixture.keypair.pubkey(),
    }
    .to_account_metas(None);
    for (vote_account, validator_history_account) in validators.iter() {
        accounts.push(AccountMeta::new(*validator_history_account, false));
        accounts.push(AccountMeta::new_readonly(*vote_account, false));
    }

    let instruction = Instruction {
        program_id: validator_history::id(),
        data: validator_history::instruction::CopyVoteAccounts {}.data(),
        accounts,
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

    let clock: Clock = ctx
        .borrow_mut()
        .banks_client
        .get_sysvar()
        .await
        .expect("clock");
    assert_eq!(clock.epoch, 0);

    // Every account must reflect exactly its own vote account's data.
    for (i, (_, validator_history_account)) in validators.iter().enumerate() {
        let account: ValidatorHistory = fixture
            .load_and_deserialize(validator_history_account)
            .await;

        assert_eq!(account.history.idx, 0);
        assert_eq!(account.history.arr[0].epoch, 0);
        assert!(account.history.arr[0].vote_account_last_update_slot <= clock.slot);
        assert_eq!(
            account.history.arr[0].epoch_credits,
            10 + (i as u32) * 5,
            "validator {i} epoch_credits mismatch"
        );
        assert_eq!(
            account.history.arr[0].commission,
            (3 + i) as u8,
            "validator {i} commission mismatch"
        );
    }
}

/// A bulk instruction with an odd number of remaining accounts must fail atomically.
#[tokio::test]
async fn test_copy_vote_accounts_odd_accounts_fails() {
    let fixture = TestFixture::new().await;
    let ctx = &fixture.ctx;
    fixture.initialize_config().await;

    let validators = setup_validators(&fixture, 1).await;
    let (vote_account, validator_history_account) = validators[0];

    let mut accounts = validator_history::accounts::CopyVoteAccounts {
        signer: fixture.keypair.pubkey(),
    }
    .to_account_metas(None);
    // Push only the validator_history_account, omitting the vote account -> odd count.
    accounts.push(AccountMeta::new(validator_history_account, false));
    let _ = vote_account;

    let instruction = Instruction {
        program_id: validator_history::id(),
        data: validator_history::instruction::CopyVoteAccounts {}.data(),
        accounts,
    };
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        ctx.borrow().last_blockhash,
    );
    // Empty substring asserts the transaction failed (without depending on the exact message).
    fixture
        .submit_transaction_assert_error(transaction, "")
        .await;
}

/// A validator_history_account paired with the wrong vote account must fail: the PDA is
/// derived from the vote account, so a mismatched pair is rejected (no writes happen).
#[tokio::test]
async fn test_copy_vote_accounts_mismatched_pair_fails() {
    let fixture = TestFixture::new().await;
    let ctx = &fixture.ctx;
    fixture.initialize_config().await;

    let validators = setup_validators(&fixture, 2).await;
    let (vote_a, vh_a) = validators[0];
    let (vote_b, _vh_b) = validators[1];

    // Pair validator A's history account with validator B's vote account.
    let mut accounts = validator_history::accounts::CopyVoteAccounts {
        signer: fixture.keypair.pubkey(),
    }
    .to_account_metas(None);
    accounts.push(AccountMeta::new(vh_a, false));
    accounts.push(AccountMeta::new_readonly(vote_b, false));
    let _ = vote_a;

    let instruction = Instruction {
        program_id: validator_history::id(),
        data: validator_history::instruction::CopyVoteAccounts {}.data(),
        accounts,
    };
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        ctx.borrow().last_blockhash,
    );
    fixture
        .submit_transaction_assert_error(transaction, "")
        .await;
}

/// A non-program-owned account passed where a ValidatorHistory account is expected must
/// be rejected (guards against writing to arbitrary accounts).
#[tokio::test]
async fn test_copy_vote_accounts_wrong_owner_fails() {
    let fixture = TestFixture::new().await;
    let ctx = &fixture.ctx;
    fixture.initialize_config().await;

    let validators = setup_validators(&fixture, 1).await;
    let (vote_account, _vh) = validators[0];

    // A system-owned account passed in place of the validator_history PDA.
    let bogus = Pubkey::new_unique();
    ctx.borrow_mut()
        .set_account(&bogus, &system_account(1_000_000_000).into());

    let mut accounts = validator_history::accounts::CopyVoteAccounts {
        signer: fixture.keypair.pubkey(),
    }
    .to_account_metas(None);
    accounts.push(AccountMeta::new(bogus, false));
    accounts.push(AccountMeta::new_readonly(vote_account, false));

    let instruction = Instruction {
        program_id: validator_history::id(),
        data: validator_history::instruction::CopyVoteAccounts {}.data(),
        accounts,
    };
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        ctx.borrow().last_blockhash,
    );
    fixture
        .submit_transaction_assert_error(transaction, "")
        .await;
}
