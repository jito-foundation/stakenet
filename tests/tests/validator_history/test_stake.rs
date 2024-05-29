#![allow(clippy::await_holding_refcell_ref)]
use anchor_lang::{solana_program::instruction::Instruction, InstructionData, ToAccountMetas};
use solana_program_test::*;
use solana_sdk::{
    clock::Clock, pubkey::Pubkey, signature::Keypair, signer::Signer, transaction::Transaction,
};
use tests::validator_history_fixtures::{system_account, TestFixture};
use validator_history::{Config, ValidatorHistory};

#[tokio::test]
async fn test_stake_history_basic_update() {
    // init fixture
    let fixture = TestFixture::new().await;
    let ctx = &fixture.ctx;
    fixture.initialize_config().await;
    fixture.initialize_validator_history_account().await;

    // update stake history
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

    // assert value
    let account: ValidatorHistory = fixture
        .load_and_deserialize(&fixture.validator_history_account)
        .await;
    assert!(account.history.idx == 0);
    assert!(account.history.arr[0].epoch == 0);
    assert!(account.history.arr[0].activated_stake_lamports == stake);
    assert!(account.history.arr[0].is_superminority == 0);
    assert!(account.history.arr[0].rank == rank);

    // sleep 2 epochs, wait again
    fixture.advance_num_epochs(2).await;

    let instruction = Instruction {
        program_id: validator_history::id(),
        data: validator_history::instruction::UpdateStakeHistory {
            epoch: 2,
            lamports: stake + 1,
            rank: rank - 1,
            is_superminority: true,
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

    // assert value
    let account: ValidatorHistory = fixture
        .load_and_deserialize(&fixture.validator_history_account)
        .await;
    assert!(account.history.idx == 1);
    assert!(account.history.arr[1].epoch == 2);
    assert!(account.history.arr[1].activated_stake_lamports == stake + 1);
    assert!(account.history.arr[1].is_superminority == 1);
    assert!(account.history.arr[1].rank == rank - 1);
}

#[tokio::test]
async fn test_stake_history_wrong_authority() {
    // init fixture
    let fixture = TestFixture::new().await;
    let ctx = &fixture.ctx;
    fixture.initialize_config().await;
    fixture.initialize_validator_history_account().await;

    // attempt update with wrong authority
    let new_authority = Keypair::new();
    ctx.borrow_mut()
        .set_account(&new_authority.pubkey(), &system_account(10000000).into());

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
async fn test_stake_history_future_epoch() {
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
    let stake = 1000;
    let rank = 42;
    let is_superminority = false;
    let instruction = Instruction {
        program_id: validator_history::id(),
        data: validator_history::instruction::UpdateStakeHistory {
            epoch: clock.epoch + 1,
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
    fixture
        .submit_transaction_assert_error(transaction, "EpochOutOfRange")
        .await;
}

#[tokio::test]
async fn test_change_oracle_authority() {
    let test = TestFixture::new().await;
    let ctx = &test.ctx;

    test.initialize_config().await;

    let new_authority = Pubkey::new_unique();

    // Change stake authority
    let instruction = Instruction {
        program_id: validator_history::id(),
        accounts: validator_history::accounts::SetNewOracleAuthority {
            config: test.validator_history_config,
            new_oracle_authority: new_authority,
            admin: test.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: validator_history::instruction::SetNewOracleAuthority {}.data(),
    };
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&test.keypair.pubkey()),
        &[&test.keypair],
        ctx.borrow().last_blockhash,
    );
    test.submit_transaction_assert_success(transaction).await;

    // Assert
    let config: Config = test
        .load_and_deserialize(&test.validator_history_config)
        .await;

    assert!(config.oracle_authority == new_authority);

    // Try to change it back with wrong signer
    let instruction = Instruction {
        program_id: validator_history::id(),
        accounts: validator_history::accounts::SetNewOracleAuthority {
            config: test.validator_history_config,
            new_oracle_authority: test.keypair.pubkey(),
            admin: test.identity_keypair.pubkey(),
        }
        .to_account_metas(None),
        data: validator_history::instruction::SetNewOracleAuthority {}.data(),
    };
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&test.identity_keypair.pubkey()),
        &[&test.identity_keypair],
        ctx.borrow().last_blockhash,
    );

    test.submit_transaction_assert_error(transaction, "ConstraintHasOne")
        .await;
}
