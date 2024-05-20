#![allow(clippy::await_holding_refcell_ref)]
use {
    anchor_lang::{
        solana_program::{instruction::Instruction, slot_history::SlotHistory},
        InstructionData, ToAccountMetas,
    },
    rand::{rngs::StdRng, Rng, SeedableRng},
    solana_program_test::*,
    solana_sdk::{
        clock::Clock, compute_budget::ComputeBudgetInstruction, epoch_schedule::EpochSchedule,
        signer::Signer, transaction::Transaction,
    },
    tests::validator_history_fixtures::TestFixture,
    validator_history::{confirmed_blocks_in_epoch, ClusterHistory},
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
    let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(1_400_000);

    Transaction::new_signed_with_payer(
        &[heap_request_ix, compute_budget_ix, instruction],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        fixture.ctx.borrow().last_blockhash,
    )
}

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
    let transaction = create_copy_cluster_history_transaction(&fixture);

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

    assert!(account.history.idx == 1);
    assert!(account.history.arr[0].epoch == 0);
    assert!(account.history.arr[0].total_blocks == 3);
    assert!(clock.epoch == 1);
    assert!(clock.slot == 32);
    assert!(clock.slot == latest_slot);
    assert!(account.cluster_history_last_update_slot == latest_slot);
    assert!(account.history.arr[1].epoch == 1);
    assert!(account.history.arr[1].total_blocks == 1);
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
    assert_ne!(account.history.arr[0].epoch_start_timestamp, u64::MAX);
    assert_ne!(account.history.arr[1].epoch_start_timestamp, u64::MAX);
    assert_eq!(
        account.history.arr[0].epoch_start_timestamp,
        account.history.arr[1].epoch_start_timestamp - dif_slots * MS_PER_SLOT
    );
}

#[tokio::test]
async fn test_cluster_history_compute_limit() {
    // Initialize
    let fixture = TestFixture::new().await;
    let ctx = &fixture.ctx;
    fixture.initialize_config().await;
    fixture.initialize_cluster_history_account().await;

    // Set EpochSchedule with 432,000 slots per epoch
    let epoch_schedule = EpochSchedule::default();
    ctx.borrow_mut().set_sysvar(&epoch_schedule);

    fixture
        .advance_num_epochs(epoch_schedule.first_normal_epoch)
        .await;
    fixture
        .advance_clock(epoch_schedule.first_normal_epoch, 400)
        .await;

    let clock: Clock = ctx
        .borrow_mut()
        .banks_client
        .get_sysvar()
        .await
        .expect("clock");

    let mut rng = StdRng::from_seed([42; 32]);
    let mut prev_epoch_total_blocks = 0;
    let mut current_epoch_total_blocks = 0;
    // Set SlotHistory sysvar
    let mut slot_history = SlotHistory::default();
    for i in epoch_schedule.get_first_slot_in_epoch(clock.epoch - 1)
        ..=epoch_schedule.get_last_slot_in_epoch(clock.epoch - 1)
    {
        if rng.gen_bool(0.5) {
            prev_epoch_total_blocks += 1;
            slot_history.add(i);
        }
    }
    for i in epoch_schedule.get_first_slot_in_epoch(clock.epoch)
        ..=epoch_schedule.get_last_slot_in_epoch(clock.epoch)
    {
        if rng.gen_bool(0.5) {
            current_epoch_total_blocks += 1;
            slot_history.add(i);
        }
    }

    ctx.borrow_mut()
        .warp_to_slot(epoch_schedule.get_last_slot_in_epoch(clock.epoch))
        .unwrap();
    let mut clock: Clock = ctx
        .borrow_mut()
        .banks_client
        .get_sysvar()
        .await
        .expect("clock");
    clock.slot = epoch_schedule.get_last_slot_in_epoch(clock.epoch);
    ctx.borrow_mut().set_sysvar(&clock);
    ctx.borrow_mut().set_sysvar(&slot_history);

    // Submit instruction
    let transaction = create_copy_cluster_history_transaction(&fixture);
    fixture.submit_transaction_assert_success(transaction).await;

    let account: ClusterHistory = fixture
        .load_and_deserialize(&fixture.cluster_history_account)
        .await;

    assert!(account.history.arr[0].epoch as u64 == clock.epoch - 1);
    assert!(account.history.arr[0].total_blocks == prev_epoch_total_blocks);
    assert!(account.history.arr[1].epoch as u64 == clock.epoch);
    assert!(account.history.arr[1].total_blocks == current_epoch_total_blocks);
}

// Non-fixture test to ensure that the SlotHistory partial slot logic works
#[test]
fn test_confirmed_blocks_in_epoch_partial_blocks() {
    let mut slot_history = SlotHistory::default();
    for i in 50..=149 {
        slot_history.add(i);
    }
    // First partial block: 50 -> 64
    // Full block: 64 -> 127
    // Last partial block: 128 -> 149
    let (num_blocks, _) = confirmed_blocks_in_epoch(50, 149, slot_history.clone()).unwrap();
    assert_eq!(num_blocks, 100);

    let (num_blocks, _) = confirmed_blocks_in_epoch(50, 99, slot_history.clone()).unwrap();
    assert_eq!(num_blocks, 50);

    let (num_blocks, _) = confirmed_blocks_in_epoch(64, 127, slot_history.clone()).unwrap();
    assert_eq!(num_blocks, 64);

    let (num_blocks, _) = confirmed_blocks_in_epoch(64, 64, slot_history.clone()).unwrap();
    assert_eq!(num_blocks, 1);

    let (num_blocks, _) = confirmed_blocks_in_epoch(100, 149, slot_history.clone()).unwrap();
    assert_eq!(num_blocks, 50);
}
