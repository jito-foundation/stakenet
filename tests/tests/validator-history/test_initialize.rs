#![allow(clippy::await_holding_refcell_ref)]
use anchor_lang::{solana_program::instruction::Instruction, InstructionData, ToAccountMetas};
use solana_program_test::*;
use solana_sdk::{signer::Signer, transaction::Transaction};
use tests::validator_history_fixtures::{new_vote_account, TestFixture};
use validator_history::{constants::MAX_ALLOC_BYTES, Config, ValidatorHistory};

#[tokio::test]
async fn test_initialize() {
    let test = TestFixture::new().await;
    let ctx = &test.ctx;

    // Initialize config
    // config keypair
    let instruction = Instruction {
        program_id: validator_history::id(),
        accounts: validator_history::accounts::InitializeConfig {
            config: test.validator_history_config,
            system_program: anchor_lang::solana_program::system_program::id(),
            signer: test.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: validator_history::instruction::InitializeConfig {
            authority: test.keypair.pubkey(),
        }
        .data(),
    };
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&test.keypair.pubkey()),
        &[&test.keypair],
        ctx.borrow().last_blockhash,
    );
    test.submit_transaction_assert_success(transaction).await;

    let config: Config = test
        .load_and_deserialize(&test.validator_history_config)
        .await;

    assert!(config.counter == 0);
    assert!(config.oracle_authority == test.keypair.pubkey());
    assert!(config.admin == test.keypair.pubkey());

    // Initialize validator history account

    let instruction = Instruction {
        program_id: validator_history::id(),
        accounts: validator_history::accounts::InitializeValidatorHistoryAccount {
            validator_history_account: test.validator_history_account,
            vote_account: test.vote_account,
            system_program: anchor_lang::solana_program::system_program::id(),
            signer: test.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: validator_history::instruction::InitializeValidatorHistoryAccount {}.data(),
    };
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&test.keypair.pubkey()),
        &[&test.keypair],
        ctx.borrow().last_blockhash,
    );
    test.submit_transaction_assert_success(transaction).await;

    // Get account and Assert exists
    let account = ctx
        .borrow_mut()
        .banks_client
        .get_account(test.validator_history_account)
        .await
        .unwrap();
    assert!(account.is_some());
    let account = account.unwrap();
    assert!(account.owner == validator_history::id());
    assert!(account.data.len() == MAX_ALLOC_BYTES);

    // Realloc validator history account
    let num_reallocs = (ValidatorHistory::SIZE - MAX_ALLOC_BYTES) / MAX_ALLOC_BYTES + 1;
    let ixs = vec![
        Instruction {
            program_id: validator_history::id(),
            accounts: validator_history::accounts::ReallocValidatorHistoryAccount {
                validator_history_account: test.validator_history_account,
                vote_account: test.vote_account,
                config: test.validator_history_config,
                system_program: anchor_lang::solana_program::system_program::id(),
                signer: test.keypair.pubkey(),
            }
            .to_account_metas(None),
            data: validator_history::instruction::ReallocValidatorHistoryAccount {}.data(),
        };
        num_reallocs
    ];
    let transaction = Transaction::new_signed_with_payer(
        &ixs,
        Some(&test.keypair.pubkey()),
        &[&test.keypair],
        ctx.borrow().last_blockhash,
    );
    test.submit_transaction_assert_success(transaction).await;

    // Assert final state
    let account: ValidatorHistory = test
        .load_and_deserialize(&test.validator_history_account)
        .await;
    assert!(account.index == 0);
    assert!(account.vote_account == test.vote_account);
    assert!(account.struct_version == 0);
    assert!(account.history.idx == 511);
    assert!(account.history.arr.len() == 512);
    assert!(account.history.is_empty == 1);
}

#[tokio::test]
async fn test_initialize_fail() {
    let test = TestFixture::new().await;
    let ctx = &test.ctx;

    test.initialize_config().await;

    // Bad vote account: less than 5 epochs of credits
    let vote_account = new_vote_account(test.vote_account, test.vote_account, 0, None);

    ctx.borrow_mut()
        .set_account(&test.vote_account, &vote_account.into());
    // Initialize validator history account

    let instruction = Instruction {
        program_id: validator_history::id(),
        accounts: validator_history::accounts::InitializeValidatorHistoryAccount {
            validator_history_account: test.validator_history_account,
            vote_account: test.vote_account,
            system_program: anchor_lang::solana_program::system_program::id(),
            signer: test.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: validator_history::instruction::InitializeValidatorHistoryAccount {}.data(),
    };
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&test.keypair.pubkey()),
        &[&test.keypair],
        ctx.borrow().last_blockhash,
    );
    test.submit_transaction_assert_error(transaction, "NotEnoughVotingHistory")
        .await;
}

#[tokio::test]
async fn test_extra_realloc() {
    let fixture = TestFixture::new().await;
    let ctx = &fixture.ctx;
    fixture.initialize_config().await;
    fixture.initialize_validator_history_account().await;

    // Set initial value
    let stake = 1000;
    let rank = 42;
    let is_superminority = false;
    let instruction = Instruction {
        program_id: validator_history::id(),
        data: validator_history::instruction::UpdateStakeHistory {
            epoch: 0,
            lamports: stake,
            rank,
            is_superminority,
        }
        .data(),
        accounts: validator_history::accounts::UpdateStakeHistory {
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

    let ix = Instruction {
        program_id: validator_history::id(),
        accounts: validator_history::accounts::ReallocValidatorHistoryAccount {
            validator_history_account: fixture.validator_history_account,
            vote_account: fixture.vote_account,
            config: fixture.validator_history_config,
            system_program: anchor_lang::solana_program::system_program::id(),
            signer: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: validator_history::instruction::ReallocValidatorHistoryAccount {}.data(),
    };
    let transaction = Transaction::new_signed_with_payer(
        &[ix],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        ctx.borrow().last_blockhash,
    );
    fixture.submit_transaction_assert_success(transaction).await;

    let account: ValidatorHistory = fixture
        .load_and_deserialize(&fixture.validator_history_account)
        .await;

    // Extra realloc should not wipe the account
    assert!(account.history.idx == 0);
    assert!(account.history.arr[0].activated_stake_lamports == stake);
    assert!(account.history.arr[0].rank == rank);
    assert!(account.history.arr[0].is_superminority == 0);
}
