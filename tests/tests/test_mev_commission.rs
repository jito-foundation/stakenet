#![allow(clippy::await_holding_refcell_ref)]
use anchor_lang::{solana_program::instruction::Instruction, InstructionData, ToAccountMetas};
use jito_tip_distribution::sdk::derive_tip_distribution_account_address;
use solana_program_test::*;
use solana_sdk::{pubkey::Pubkey, signer::Signer, transaction::Transaction};
use tests::fixtures::{new_tip_distribution_account, TestFixture};
use validator_history::{Config, ValidatorHistory};

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
    ctx.borrow_mut().set_account(
        &tip_distribution_account,
        &new_tip_distribution_account(fixture.vote_account, 42).into(),
    );

    // update mev commission
    let instruction = Instruction {
        program_id: validator_history::id(),
        data: validator_history::instruction::UpdateMevCommission {}.data(),
        accounts: validator_history::accounts::UpdateMevCommission {
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

    if let Err(e) = ctx
        .borrow_mut()
        .banks_client
        .process_transaction_with_preflight(transaction)
        .await
    {
        panic!("Error: {}", e);
    }

    // assert value
    let account: ValidatorHistory = fixture
        .load_and_deserialize(&fixture.validator_history_account)
        .await;
    assert!(account.history.idx == 0);
    assert!(account.history.arr[0].epoch == 0);
    assert!(account.history.arr[0].mev_commission == 42);

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

    // test update mev commission with uninitialized TDA
    let tip_distribution_account = derive_tip_distribution_account_address(
        &jito_tip_distribution::id(),
        &fixture.vote_account,
        0,
    )
    .0;
    let instruction = Instruction {
        program_id: validator_history::id(),
        data: validator_history::instruction::UpdateMevCommission {}.data(),
        accounts: validator_history::accounts::UpdateMevCommission {
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
        &new_tip_distribution_account(new_vote_account, 42).into(),
    );

    // test update mev commission with wrong validator's TDA
    let instruction = Instruction {
        program_id: validator_history::id(),
        data: validator_history::instruction::UpdateMevCommission {}.data(),
        accounts: validator_history::accounts::UpdateMevCommission {
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
