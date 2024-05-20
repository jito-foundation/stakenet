#![allow(clippy::await_holding_refcell_ref)]
use anchor_lang::{solana_program::instruction::Instruction, InstructionData, ToAccountMetas};
use jito_tip_distribution::sdk::derive_tip_distribution_account_address;
use solana_program_test::*;
use solana_sdk::{pubkey::Pubkey, signer::Signer, transaction::Transaction};
use tests::validator_history_fixtures::{new_tip_distribution_account, TestFixture};
use validator_history::{Config, ValidatorHistory, ValidatorHistoryEntry};

#[tokio::test]
async fn test_mev_commission() {
    let fixture = TestFixture::new().await;
    let ctx = &fixture.ctx;
    fixture.initialize_config().await;
    fixture.initialize_validator_history_account().await;

    let epoch = 0;
    let tip_distribution_account = derive_tip_distribution_account_address(
        &jito_tip_distribution::id(),
        &fixture.vote_account,
        epoch,
    )
    .0;
    // No MEV earned
    ctx.borrow_mut().set_account(
        &tip_distribution_account,
        &new_tip_distribution_account(fixture.vote_account, 42, None).into(),
    );

    // update mev commission
    let instruction = Instruction {
        program_id: validator_history::id(),
        data: validator_history::instruction::CopyTipDistributionAccount { epoch }.data(),
        accounts: validator_history::accounts::CopyTipDistributionAccount {
            validator_history_account: fixture.validator_history_account,
            vote_account: fixture.vote_account,
            config: fixture.validator_history_config,
            tip_distribution_account,
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

    // assert values, mev earned default
    let account: ValidatorHistory = fixture
        .load_and_deserialize(&fixture.validator_history_account)
        .await;
    assert!(account.history.idx == 0);
    assert!(account.history.arr[0].epoch == 0);
    assert!(account.history.arr[0].mev_commission == 42);
    assert!(account.history.arr[0].mev_earned == ValidatorHistoryEntry::default().mev_earned);

    ctx.borrow_mut().set_account(
        &tip_distribution_account,
        &new_tip_distribution_account(fixture.vote_account, 42, Some(123_236_567_899)).into(),
    );

    let blockhash = ctx.borrow_mut().get_new_latest_blockhash().await.unwrap();
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        blockhash,
    );

    fixture.submit_transaction_assert_success(transaction).await;

    // assert mev earned no longer default
    let account: ValidatorHistory = fixture
        .load_and_deserialize(&fixture.validator_history_account)
        .await;
    assert!(account.history.arr[0].mev_earned == 12324); // fixed point representation
    assert!((account.history.arr[0].mev_earned as f64 / 100.0) == 123.24_f64);

    // TODO this is causing a hash mismatch issue
    // fixture.advance_num_epochs(1).await;

    // // create new TDA
    // let tip_distribution_account = derive_tip_distribution_account_address(
    //     &jito_tip_distribution::id(),
    //     &fixture.vote_account,
    //     1,
    // )
    // .0;
    // ctx.borrow_mut().set_account(
    //     &tip_distribution_account,
    //     &new_tip_distribution_account(fixture.vote_account, 43).into(),
    // );

    // let instruction = Instruction {
    //     program_id: validator_history::id(),
    //     data: validator_history::instruction::UpdateMevCommission {}.data(),
    //     accounts: validator_history::accounts::UpdateMevCommission {
    //         validator_history_account: fixture.validator_history_account,
    //         vote_account: fixture.vote_account,
    //         config: fixture.validator_history_config,
    //         tip_distribution_account,
    //         signer: fixture.keypair.pubkey(),
    //     }
    //     .to_account_metas(None),
    // };

    // let transaction = Transaction::new_signed_with_payer(
    //     &[instruction],
    //     Some(&fixture.keypair.pubkey()),
    //     &[&fixture.keypair],
    //     ctx.borrow().last_blockhash,
    // );

    // fixture.submit_transaction_assert_success(transaction).await;
    // // Assert values
    // let account: ValidatorHistory = fixture
    //     .load_and_deserialize(&fixture.validator_history_account)
    //     .await;
    // assert!(account.history.idx == 1);
    // assert!(account.history.arr[1].epoch == 1);
    // assert!(account.history.arr[1].mev_commission == 43);
}

#[tokio::test]
async fn test_mev_commission_fail() {
    let fixture = TestFixture::new().await;
    let ctx = &fixture.ctx;
    fixture.initialize_config().await;
    fixture.initialize_validator_history_account().await;
    let epoch = 0;

    // test update mev commission with uninitialized TDA
    let tip_distribution_account = derive_tip_distribution_account_address(
        &jito_tip_distribution::id(),
        &fixture.vote_account,
        epoch,
    )
    .0;
    let instruction = Instruction {
        program_id: validator_history::id(),
        data: validator_history::instruction::CopyTipDistributionAccount { epoch }.data(),
        accounts: validator_history::accounts::CopyTipDistributionAccount {
            validator_history_account: fixture.validator_history_account,
            vote_account: fixture.vote_account,
            config: fixture.validator_history_config,
            tip_distribution_account,
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
    // test update mev commission with wrong epoch
    // note that just advancing the fixture's epoch cause a failure bc we relaxed the epoch constraints in the instruction/on tip_distribution_account
    // explicitly pass the instruction a different epoch than the one used to generate the tip_distribution pda
    let instruction = Instruction {
        program_id: validator_history::id(),
        data: validator_history::instruction::CopyTipDistributionAccount { epoch: 1 }.data(),
        accounts: validator_history::accounts::CopyTipDistributionAccount {
            validator_history_account: fixture.validator_history_account,
            vote_account: fixture.vote_account,
            config: fixture.validator_history_config,
            tip_distribution_account,
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
    let tip_distribution_account =
        derive_tip_distribution_account_address(&jito_tip_distribution::id(), &new_vote_account, 1)
            .0;
    ctx.borrow_mut().set_account(
        &tip_distribution_account,
        &new_tip_distribution_account(new_vote_account, 42, Some(123456)).into(),
    );

    // test update mev commission with wrong validator's TDA
    let instruction = Instruction {
        program_id: validator_history::id(),
        data: validator_history::instruction::CopyTipDistributionAccount { epoch }.data(),
        accounts: validator_history::accounts::CopyTipDistributionAccount {
            validator_history_account: fixture.validator_history_account,
            vote_account: new_vote_account,
            config: fixture.validator_history_config,
            tip_distribution_account,
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

// Test change tip distribution authority
#[tokio::test]
async fn test_change_tip_distribution_authority() {
    let fixture = TestFixture::new().await;
    let ctx = &fixture.ctx;

    fixture.initialize_config().await;

    let new_authority = Pubkey::new_unique();

    // Change tip distribution authority
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

    // Change tip distribution authority
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
