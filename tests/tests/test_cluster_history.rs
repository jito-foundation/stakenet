#![allow(clippy::await_holding_refcell_ref)]
use anchor_lang::{
    solana_program::{instruction::Instruction, slot_history::SlotHistory},
    InstructionData, ToAccountMetas,
};
use solana_program_test::*;
use solana_sdk::{
    clock::Clock, compute_budget::ComputeBudgetInstruction, signer::Signer,
    transaction::Transaction,
};
use tests::fixtures::TestFixture;
use validator_history::ClusterHistory;

#[tokio::test]
async fn test_copy_cluster_info() {
    // Initialize
    let fixture = TestFixture::new().await;
    let ctx = &fixture.ctx;
    fixture.initialize_config().await;
    fixture.initialize_cluster_history_account().await;

    fixture.advance_num_epochs(1).await;

    // Set SlotHistory sysvar with a few slots
    let mut slot_history = SlotHistory::default();
    slot_history.add(0);
    slot_history.add(1);
    slot_history.add(2);

    let latest_slot = ctx.borrow_mut().banks_client.get_root_slot().await.unwrap();
    slot_history.add(latest_slot);
    slot_history.add(latest_slot + 1);

    // Submit instruction
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

    let transaction = Transaction::new_signed_with_payer(
        &[heap_request_ix, compute_budget_ix, instruction],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        ctx.borrow().last_blockhash,
    );

    ctx.borrow_mut().set_sysvar(&slot_history);
    fixture.submit_transaction_assert_success(transaction).await;

    let account: ClusterHistory = fixture
        .load_and_deserialize(&fixture.cluster_history_account)
        .await;

    let clock: Clock = ctx
        .borrow_mut()
        .banks_client
        .get_sysvar()
        .await
        .expect("clock");

    assert!(clock.epoch == 1);
    assert!(account.history.idx == 1);
    assert!(account.history.arr[0].epoch == 0);
    assert!(account.history.arr[0].total_blocks == 3);
    assert!(account.history.arr[1].epoch == 1);
    assert!(account.history.arr[1].total_blocks == 2);
    assert_eq!(account.cluster_history_last_update_slot, latest_slot)
}
