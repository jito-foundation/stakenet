use anchor_lang::{system_program, InstructionData, ToAccountMetas};
use borsh::BorshDeserialize;
use solana_program_test::*;

use solana_sdk::{
    account::Account,
    instruction::Instruction,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    transaction::Transaction,
};
use tests::{steward_fixtures::system_account, validator_history_fixtures::TestFixture};
use validator_history::Config;

#[derive(BorshDeserialize)]
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

    // Override and set the config account to be like an old config. This allows the test
    // to guard against old values being zeroed and anchor trying to deserialize a smaller account.
    let old_data = vec![
        155, 12, 170, 224, 30, 250, 204, 130, 50, 188, 7, 199, 253, 229, 63, 44, 159, 69, 138, 232,
        81, 242, 88, 42, 158, 196, 251, 0, 10, 135, 214, 103, 196, 119, 15, 22, 209, 209, 252, 156,
        69, 30, 61, 213, 13, 59, 123, 133, 54, 4, 92, 43, 122, 194, 236, 37, 148, 115, 235, 194,
        90, 227, 188, 190, 31, 190, 177, 125, 82, 251, 199, 190, 169, 202, 56, 195, 181, 73, 237,
        112, 140, 152, 55, 180, 250, 179, 224, 36, 3, 89, 36, 25, 203, 32, 61, 151, 43, 195, 71,
        78, 205, 193, 106, 112, 151, 12, 0, 0, 248, 0, 0, 0,
    ];
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
    let mut data: &[u8] = &old_data[8..];
    let old_config = OldConfig::deserialize(&mut data).unwrap();

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

    // Validate the config account data did not change
    assert_eq!(
        old_config.tip_distribution_program,
        config_after.tip_distribution_program
    );
    assert_eq!(old_config.admin, config_after.admin);
    assert_eq!(old_config.oracle_authority, config_after.oracle_authority);
    assert_eq!(old_config.bump, config_after.bump);
    assert_eq!(old_config.counter, config_after.counter);
}
