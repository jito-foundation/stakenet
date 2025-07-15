#![allow(clippy::await_holding_refcell_ref)]

use anchor_lang::{system_program, Discriminator, InstructionData, ToAccountMetas};
use borsh::{BorshDeserialize, BorshSerialize};
use solana_program_test::*;

use jito_priority_fee_distribution::ID as PRIORITY_FEE_DISTRIBUTION_PROGRAM_ID;
use solana_sdk::{
    account::Account,
    instruction::Instruction,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    transaction::Transaction,
};

use std::str::FromStr;
use tests::{steward_fixtures::system_account, validator_history_fixtures::TestFixture};
use validator_history::Config;

#[derive(BorshSerialize, BorshDeserialize)]
struct OldConfig {
    pub tip_distribution_program: Pubkey,
    pub admin: Pubkey,
    pub oracle_authority: Pubkey,
    pub counter: u32,
    pub bump: u8,
}

#[tokio::test]
async fn test_realloc_with_actual_current_config() {
    // init fixture
    let fixture = TestFixture::new().await;
    let ctx = &fixture.ctx;

    // create a new payer to ensure IX is permissionless
    let random_payer = Keypair::new();
    ctx.borrow_mut()
        .set_account(&random_payer.pubkey(), &system_account(10000000).into());

    let old_data = vec![
        0x9b, 0x0c, 0xaa, 0xe0, 0x1e, 0xfa, 0xcc, 0x82, 0x32, 0xbc, 0x07, 0xc7, 0xfd, 0xe5, 0x3f,
        0x2c, 0x9f, 0x45, 0x8a, 0xe8, 0x51, 0xf2, 0x58, 0x2a, 0x9e, 0xc4, 0xfb, 0x00, 0x0a, 0x87,
        0xd6, 0x67, 0xc4, 0x77, 0x0f, 0x16, 0xd1, 0xd1, 0xfc, 0x9c, 0x45, 0x1e, 0x3d, 0xd5, 0x0d,
        0x3b, 0x7b, 0x85, 0x36, 0x04, 0x5c, 0x2b, 0x7a, 0xc2, 0xec, 0x25, 0x94, 0x73, 0xeb, 0xc2,
        0x5a, 0xe3, 0xbc, 0xbe, 0x1f, 0xbe, 0xb1, 0x7d, 0x52, 0xfb, 0xc7, 0xbe, 0xa9, 0xca, 0x38,
        0xc3, 0xb5, 0x49, 0xed, 0x70, 0x8c, 0x98, 0x37, 0xb4, 0xfa, 0xb3, 0xe0, 0x24, 0x03, 0x59,
        0x24, 0x19, 0xcb, 0x20, 0x3d, 0x97, 0x2b, 0xc3, 0x47, 0x4e, 0xcd, 0xc1, 0x6a, 0x70, 0x39,
        0x0d, 0x00, 0x00, 0xf8, 0x00, 0x00, 0x00,
    ];

    let mut data: &[u8] = &old_data[8..];
    let old_config = OldConfig::deserialize(&mut data).unwrap();

    // Override and set the config account to be like an old config. This allows the test
    // to guard against old values being zeroed and anchor trying to deserialize a smaller account.
    let old_config_account = Account {
        lamports: 1_670_400,
        owner: validator_history::id(),
        executable: false,
        rent_epoch: 0,
        data: old_data.clone(),
    };
    ctx.borrow_mut().set_account(
        &fixture.validator_history_config,
        &old_config_account.into(),
    );

    // TX to re-alloc the config account
    let instruction = Instruction {
        program_id: validator_history::id(),
        data: validator_history::instruction::ReallocConfigAccount {}.data(),
        accounts: validator_history::accounts::ReallocConfigAccount {
            config_account: fixture.validator_history_config,
            system_program: system_program::ID,
            payer: random_payer.pubkey(),
        }
        .to_account_metas(None),
    };

    let blockhash = ctx.borrow_mut().get_new_latest_blockhash().await.unwrap();
    let transaction = Transaction::new_signed_with_payer(
        &[instruction.clone()],
        Some(&random_payer.pubkey()),
        &[&random_payer],
        blockhash,
    );

    fixture.submit_transaction_assert_success(transaction).await;

    // Validate that the config account has extra space.
    let config_account_data_after = ctx
        .borrow_mut()
        .banks_client
        .get_account(fixture.validator_history_config)
        .await
        .unwrap()
        .unwrap()
        .data;
    let config_after: Config = fixture
        .load_and_deserialize(&fixture.validator_history_config)
        .await;
    assert!(config_account_data_after.len() > old_data.len());
    assert_eq!(config_account_data_after.len(), Config::SIZE);

    // Validate the old config account data did not change
    assert_eq!(
        old_config.tip_distribution_program,
        config_after.tip_distribution_program
    );
    assert_eq!(old_config.admin, config_after.admin);
    assert_eq!(old_config.oracle_authority, config_after.oracle_authority);
    assert_eq!(old_config.bump, config_after.bump);
    assert_eq!(old_config.counter, config_after.counter);

    // Validate the, previously unset, priority_fee_oracle_authority is now set
    assert_eq!(
        old_config.oracle_authority,
        config_after.priority_fee_oracle_authority
    );
    assert_eq!(
        config_after.priority_fee_distribution_program,
        PRIORITY_FEE_DISTRIBUTION_PROGRAM_ID
    );
}

#[tokio::test]
async fn test_realloc_config_happy_path() {
    // init fixture
    let fixture = TestFixture::new().await;
    let ctx = &fixture.ctx;

    // create a new payer to ensure IX is permissionless
    let random_payer = Keypair::new();
    ctx.borrow_mut()
        .set_account(&random_payer.pubkey(), &system_account(10000000).into());

    // Create an old config structure that doesn't have priority_fee_oracle_authority
    let old_config = OldConfig {
        tip_distribution_program: Pubkey::from_str("4R3gSG8BpU4t19KYj8CfnbtRpnT8gtk4dvTHxVRwc2r7")
            .unwrap(),
        admin: Pubkey::from_str("GZctHpWXmsZC1YHACTGGcHhYxjdRqQvTpYkb9LMvxDib").unwrap(),
        oracle_authority: Pubkey::from_str("8F4jGUmxF36vQ6yabnsxX6AQVXdKBhs8kGSUuRKSg8Xt").unwrap(),
        counter: 12,
        bump: 248,
    };

    let mut old_data = vec![];
    let discriminator_bytes = Config::DISCRIMINATOR.to_vec();
    let config_bytes = old_config.try_to_vec().unwrap();

    old_data.extend(&discriminator_bytes);
    old_data.extend(&config_bytes);

    // Override and set the config account to be like an old config. This allows the test
    // to guard against old values being zeroed and anchor trying to deserialize a smaller account.
    let old_config_account = Account {
        lamports: 1_670_400,
        owner: validator_history::id(),
        executable: false,
        rent_epoch: 0,
        data: old_data.clone(),
    };
    ctx.borrow_mut().set_account(
        &fixture.validator_history_config,
        &old_config_account.into(),
    );

    // TX to re-alloc the config account
    let instruction = Instruction {
        program_id: validator_history::id(),
        data: validator_history::instruction::ReallocConfigAccount {}.data(),
        accounts: validator_history::accounts::ReallocConfigAccount {
            config_account: fixture.validator_history_config,
            system_program: system_program::ID,
            payer: random_payer.pubkey(),
        }
        .to_account_metas(None),
    };

    let blockhash = ctx.borrow_mut().get_new_latest_blockhash().await.unwrap();
    let transaction = Transaction::new_signed_with_payer(
        &[instruction.clone()],
        Some(&random_payer.pubkey()),
        &[&random_payer],
        blockhash,
    );

    fixture.submit_transaction_assert_success(transaction).await;

    // Validate that the config account has extra space.
    let config_account_data_after = ctx
        .borrow_mut()
        .banks_client
        .get_account(fixture.validator_history_config)
        .await
        .unwrap()
        .unwrap()
        .data;
    let config_after: Config = fixture
        .load_and_deserialize(&fixture.validator_history_config)
        .await;
    assert!(config_account_data_after.len() > old_data.len());
    assert_eq!(config_account_data_after.len(), Config::SIZE);

    // Validate the old config account data did not change
    assert_eq!(
        old_config.tip_distribution_program,
        config_after.tip_distribution_program
    );
    assert_eq!(old_config.admin, config_after.admin);
    assert_eq!(old_config.oracle_authority, config_after.oracle_authority);
    assert_eq!(old_config.bump, config_after.bump);
    assert_eq!(old_config.counter, config_after.counter);

    // Validate the, previously unset, priority_fee_oracle_authority is now set
    assert_eq!(
        old_config.oracle_authority,
        config_after.priority_fee_oracle_authority
    );
    assert_eq!(
        config_after.priority_fee_distribution_program,
        PRIORITY_FEE_DISTRIBUTION_PROGRAM_ID
    );
}

#[tokio::test]
async fn test_realloc_fails_when_size_doesnt_change() {
    // init fixture
    let fixture = TestFixture::new().await;
    let ctx = &fixture.ctx;
    fixture.initialize_config().await;

    let instruction = Instruction {
        program_id: validator_history::id(),
        data: validator_history::instruction::ReallocConfigAccount {}.data(),
        accounts: validator_history::accounts::ReallocConfigAccount {
            config_account: fixture.validator_history_config,
            system_program: system_program::ID,
            payer: fixture.keypair.pubkey(),
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
    fixture
        .submit_transaction_assert_error(transaction, "NoReallocNeeded")
        .await;
}
