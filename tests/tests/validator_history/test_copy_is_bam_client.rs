use anchor_lang::{solana_program::instruction::Instruction, InstructionData, ToAccountMetas};
use solana_program_test::*;
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction, signature::Keypair, signer::Signer,
    transaction::Transaction,
};
use tests::validator_history_fixtures::TestFixture;
use validator_history::ValidatorHistory;

fn create_copy_is_bam_client_transaction(
    fixture: &TestFixture,
    oracle_authority: &Keypair,
    is_jito_bam_client: u8,
) -> Transaction {
    let instruction = Instruction {
        program_id: validator_history::id(),
        data: validator_history::instruction::CopyIsJitoBamClient { is_jito_bam_client }.data(),
        accounts: validator_history::accounts::CopyIsJitoBamClient {
            config: fixture.validator_history_config,
            validator_history_account: fixture.validator_history_account,
            vote_account: fixture.vote_account,
            oracle_authority: oracle_authority.pubkey(),
        }
        .to_account_metas(None),
    };
    let heap_request_ix = ComputeBudgetInstruction::request_heap_frame(256 * 1024);
    let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(1_400_000);

    Transaction::new_signed_with_payer(
        &[heap_request_ix, compute_budget_ix, instruction],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair, oracle_authority],
        fixture.ctx.borrow().last_blockhash,
    )
}

#[tokio::test]
async fn test_copy_is_bam_client_success() {
    // Initialize
    let fixture = TestFixture::new().await;
    fixture.initialize_config().await;
    fixture.initialize_validator_history_account().await;

    let is_bam_client = 1;

    // Submit instruction
    let transaction =
        create_copy_is_bam_client_transaction(&fixture, &fixture.keypair, is_bam_client);

    fixture.submit_transaction_assert_success(transaction).await;

    let account: ValidatorHistory = fixture
        .load_and_deserialize(&fixture.validator_history_account)
        .await;
    assert_eq!(account.history.arr[0].is_jito_bam_client, is_bam_client);
}

#[tokio::test]
async fn test_copy_is_bam_client_invalid_oracle_authority_fails() {
    // Initialize
    let fixture = TestFixture::new().await;
    fixture.initialize_config().await;
    fixture.initialize_validator_history_account().await;

    let is_bam_client = 1;

    let invalid_oracle_authority = Keypair::new();

    // Submit instruction
    let transaction =
        create_copy_is_bam_client_transaction(&fixture, &invalid_oracle_authority, is_bam_client);

    fixture
        .submit_transaction_assert_error(transaction, "ConstraintHasOne")
        .await;
}
