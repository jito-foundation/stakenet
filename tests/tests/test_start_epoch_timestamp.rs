#![allow(clippy::await_holding_refcell_ref)]

use {
    anchor_lang::{
        solana_program::{instruction::Instruction, slot_history::SlotHistory},
        InstructionData, ToAccountMetas,
    },
    solana_program_test::*,
    solana_sdk::{
        compute_budget::ComputeBudgetInstruction, signer::Signer, transaction::Transaction,
    },
    tests::fixtures::TestFixture,
    validator_history::ClusterHistory,
};

const MS_PER_SLOT: u64 = 400;

fn create_copy_cluster_history_transaction(fixture: &TestFixture) -> Transaction {
    let instruction = Instruction {
        program_id: validator_history::id(),
        data: validator_history::instruction::CopyClusterInfo {}.data(),
        accounts: validator_history::accounts::CopyClusterInfo {
            cluster_history_account: fixture.cluster_history_account,
            slot_history: anchor_lang::solana_program::sysvar::slot_history::id(),
            signer: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
    };
    let heap_request_ix = ComputeBudgetInstruction::request_heap_frame(256 * 1024);
    let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(300_000);

    Transaction::new_signed_with_payer(
        &[heap_request_ix, compute_budget_ix, instruction],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        fixture.ctx.borrow().last_blockhash,
    )
}

#[tokio::test]
async fn test_start_epoch_timestamp() {
    // Initialize
    let fixture = TestFixture::new().await;
    let ctx = &fixture.ctx;
    fixture.initialize_config().await;
    fixture.initialize_cluster_history_account().await;

    // Set SlotHistory sysvar
    let slot_history = SlotHistory::default();
    ctx.borrow_mut().set_sysvar(&slot_history);

    // Submit epoch 0 instruction
    let transaction = create_copy_cluster_history_transaction(&fixture);
    fixture.submit_transaction_assert_success(transaction).await;

    // Change epoch and set clock timestamps in the future
    fixture.advance_num_epochs(1).await;
    let dif_slots = fixture.advance_clock(1, MS_PER_SLOT).await;

    // Submit epoch 1 instruction
    let transaction = create_copy_cluster_history_transaction(&fixture);
    fixture.submit_transaction_assert_success(transaction).await;

    let account: ClusterHistory = fixture
        .load_and_deserialize(&fixture.cluster_history_account)
        .await;

    assert_eq!(account.history.arr[0].epoch, 0);
    assert_eq!(account.history.arr[1].epoch, 1);
    assert_ne!(account.history.arr[0].epoch_start_timestamp, u32::MAX);
    assert_ne!(account.history.arr[1].epoch_start_timestamp, u32::MAX);
    assert_eq!(
        account.history.arr[0].epoch_start_timestamp,
        account.history.arr[1].epoch_start_timestamp - (dif_slots * MS_PER_SLOT) as u32
    );
}
