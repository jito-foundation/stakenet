use anchor_lang::{InstructionData, ToAccountMetas};
use solana_program_test::tokio;
use solana_sdk::{
    instruction::Instruction, pubkey::Pubkey, signature::Signer, transaction::Transaction,
};
use tests::validator_history_fixtures::TestFixture;
use validator_history::Config;

#[tokio::test]
async fn test_change_priority_fee_oracle_authority() {
    let fixture = TestFixture::new().await;
    let ctx = &fixture.ctx;

    fixture.initialize_config().await;

    let new_authority = Pubkey::new_unique();

    // Change priority fee oracle authority
    let instruction = Instruction {
        program_id: validator_history::id(),
        accounts: validator_history::accounts::SetNewPriorityFeeOracleAuthority {
            config: fixture.validator_history_config,
            new_priority_fee_oracle_authority: new_authority,
            admin: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: validator_history::instruction::SetNewPriorityFeeOracleAuthority {}.data(),
    };
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        ctx.borrow().last_blockhash,
    );
    fixture.submit_transaction_assert_success(transaction).await;

    // Assert
    let config: Config = fixture
        .load_and_deserialize(&fixture.validator_history_config)
        .await;

    assert_eq!(config.priority_fee_oracle_authority, new_authority);

    // Try to change it back with wrong signer
    let instruction = Instruction {
        program_id: validator_history::id(),
        accounts: validator_history::accounts::SetNewOracleAuthority {
            config: fixture.validator_history_config,
            new_oracle_authority: fixture.keypair.pubkey(),
            admin: fixture.identity_keypair.pubkey(),
        }
        .to_account_metas(None),
        data: validator_history::instruction::SetNewOracleAuthority {}.data(),
    };
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&fixture.identity_keypair.pubkey()),
        &[&fixture.identity_keypair],
        ctx.borrow().last_blockhash,
    );

    fixture
        .submit_transaction_assert_error(transaction, "ConstraintHasOne")
        .await;
}
