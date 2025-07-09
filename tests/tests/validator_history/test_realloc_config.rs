use anchor_lang::{system_program, InstructionData, ToAccountMetas, Discriminator};
use borsh::{BorshDeserialize, BorshSerialize};
use solana_program_test::*;

use solana_sdk::{
    account::Account,
    instruction::Instruction,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    transaction::Transaction,
};
use jito_priority_fee_distribution::{
     ID as PRIORITY_FEE_DISTRIBUTION_PROGRAM_ID,
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
        tip_distribution_program: Pubkey::from_str("4R3gSG8BpU4t19KYj8CfnbtRpnT8gtk4dvTHxVRwc2r7").unwrap(),
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
    assert_eq!(config_account_data_after.len(), Config::SIZE as usize);

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
