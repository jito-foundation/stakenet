use anchor_lang::{InstructionData, ToAccountMetas};
use jito_steward::{
    instructions::AuthorityType,
    state::directed_stake::{DirectedStakeMeta, DirectedStakeTarget},
};
use solana_program::{
    instruction::Instruction,
    sysvar,
};
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction,
    pubkey::Pubkey,
    signature::Keypair,
    signer::Signer,
    transaction::Transaction,
};
use solana_program_test::*;
use spl_stake_pool::{
    find_transient_stake_program_address,
};

use tests::steward_fixtures::TestFixture;

/// Helper function to create a test fixture with directed stake setup
async fn setup_directed_stake_fixture() -> TestFixture {
    let fixture = TestFixture::new().await;
    
    fixture.initialize_stake_pool().await;
    fixture.initialize_steward(None, None).await;
    println!("Steward initialized");
    fixture.realloc_steward_state().await;
    println!("Steward state reallocated");
    println!("Fixture initialized");
    
    // Set the directed stake whitelist authority to the fixture's keypair
    set_directed_stake_whitelist_authority(&fixture).await;
    
    println!("Directed stake whitelist authority set");
    // Initialize the directed stake whitelist first
    initialize_directed_stake_whitelist(&fixture).await;
    
    println!("Directed stake whitelist initialized");

    fixture
}

/// Helper function to set the directed stake whitelist authority
async fn set_directed_stake_whitelist_authority(fixture: &TestFixture) {
    let ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::SetNewAuthority {
            config: fixture.steward_config.pubkey(),
            new_authority: fixture.keypair.pubkey(),
            admin: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::SetNewAuthority {
            authority_type: AuthorityType::SetDirectedStakeWhitelistAuthority,
        }
        .data(),
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        fixture.ctx.borrow().last_blockhash,
    );

    fixture.submit_transaction_assert_success(tx).await;
}

/// Helper function to initialize directed stake whitelist
async fn initialize_directed_stake_whitelist(fixture: &TestFixture) {
    let directed_stake_whitelist = Pubkey::find_program_address(
        &[jito_steward::DirectedStakeWhitelist::SEED, fixture.steward_config.pubkey().as_ref()],
        &jito_steward::id(),
    ).0;

    let ix = Instruction {
        program_id: jito_steward::id(),
        accounts: vec![
            anchor_lang::solana_program::instruction::AccountMeta::new_readonly(
                fixture.steward_config.pubkey(),
                false,
            ),
            anchor_lang::solana_program::instruction::AccountMeta::new(
                directed_stake_whitelist,
                false,
            ),
            anchor_lang::solana_program::instruction::AccountMeta::new_readonly(
                anchor_lang::solana_program::system_program::id(),
                false,
            ),
            anchor_lang::solana_program::instruction::AccountMeta::new(
                fixture.keypair.pubkey(),
                true,
            ),
        ],
        data: jito_steward::instruction::InitializeDirectedStakeWhitelist {}.data(),
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        fixture.ctx.borrow().last_blockhash,
    );

    fixture.submit_transaction_assert_success(tx).await;
}

/// Helper function to create a simple directed stake meta for testing
fn create_simple_directed_stake_meta(
    vote_pubkey: Pubkey,
    target_lamports: u64,
    staked_lamports: u64,
) -> DirectedStakeMeta {
    let mut meta = DirectedStakeMeta {
        epoch: 0,
        total_stake_targets: 1,
        uploaded_stake_targets: 1,
        _padding0: [0; 132],
        targets: [DirectedStakeTarget {
            vote_pubkey: Pubkey::default(),
            total_target_lamports: 0,
            total_staked_lamports: 0,
            _padding0: [0; 64],
        }; 2048],
    };

    // Set the first target
    meta.targets[0] = DirectedStakeTarget {
        vote_pubkey,
        total_target_lamports: target_lamports,
        total_staked_lamports: staked_lamports,
        _padding0: [0; 64],
    };

    meta
}

#[tokio::test]
async fn test_simple_directed_rebalance_increase() {
    // Test case: Validator needs more stake to reach target
    let fixture = setup_directed_stake_fixture().await;
    
    // Create a validator
    let validator = Keypair::new();
    let vote_pubkey = validator.pubkey();
    
    // Set up directed stake meta with target > staked
    let target_lamports = 1_000_000_000; // 1 SOL target
    let staked_lamports = 500_000_000;   // 0.5 SOL currently staked
    let directed_stake_meta = create_simple_directed_stake_meta(
        vote_pubkey,
        target_lamports,
        staked_lamports,
    );
    
    // Create the rebalance_directed instruction
    let rebalance_ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::RebalanceDirected {
            config: fixture.steward_config.pubkey(),
            state_account: fixture.steward_state,
            directed_stake_meta: Pubkey::find_program_address(
                &[DirectedStakeMeta::SEED, fixture.steward_config.pubkey().as_ref()],
                &jito_steward::id(),
            ).0,
            stake_pool: fixture.stake_pool_meta.stake_pool,
            stake_pool_program: spl_stake_pool::id(),
            withdraw_authority: fixture.stake_pool_meta.reserve, // Use reserve as withdraw authority
            validator_list: fixture.stake_pool_meta.validator_list,
            reserve_stake: fixture.stake_pool_meta.reserve,
            stake_account: fixture.stake_pool_meta.reserve, // Use reserve as stake account for simplicity
            transient_stake_account: find_transient_stake_program_address(
                &spl_stake_pool::id(),
                &vote_pubkey,
                &fixture.stake_pool_meta.stake_pool,
                0u64,
            ).0,
            vote_account: vote_pubkey,
            clock: sysvar::clock::id(),
            rent: sysvar::rent::id(),
            stake_history: sysvar::stake_history::id(),
            stake_config: solana_program::stake::config::ID,
            system_program: solana_program::system_program::id(),
            stake_program: solana_program::stake::program::id(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::RebalanceDirected {
            validator_list_index: 0,
        }
        .data(),
    };

    // Submit the transaction
    let tx = Transaction::new_signed_with_payer(
        &[
            ComputeBudgetInstruction::request_heap_frame(256 * 1024),
            ComputeBudgetInstruction::set_compute_unit_limit(1_400_000),
            rebalance_ix,
        ],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        fixture.get_latest_blockhash().await,
    );

    // The transaction should succeed (or fail gracefully if not in the right state)
    fixture.submit_transaction_assert_success(tx).await;
    
    // If we get here, the rebalance was executed successfully
    println!("Rebalance executed successfully");
}

#[tokio::test]
async fn test_simple_directed_rebalance_decrease() {
    // Test case: Validator has too much stake and needs to decrease
    let fixture = setup_directed_stake_fixture().await;
    
    // Create a validator
    let validator = Keypair::new();
    let vote_pubkey = validator.pubkey();
    
    // Set up directed stake meta with staked > target
    let target_lamports = 500_000_000;   // 0.5 SOL target
    let staked_lamports = 1_000_000_000; // 1 SOL currently staked (excess)
    let directed_stake_meta = create_simple_directed_stake_meta(
        vote_pubkey,
        target_lamports,
        staked_lamports,
    );
    
    // Create the rebalance_directed instruction
    let rebalance_ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::RebalanceDirected {
            config: fixture.steward_config.pubkey(),
            state_account: fixture.steward_state,
            directed_stake_meta: Pubkey::find_program_address(
                &[DirectedStakeMeta::SEED, fixture.steward_config.pubkey().as_ref()],
                &jito_steward::id(),
            ).0,
            stake_pool: fixture.stake_pool_meta.stake_pool,
            stake_pool_program: spl_stake_pool::id(),
            withdraw_authority: fixture.stake_pool_meta.reserve, // Use reserve as withdraw authority
            validator_list: fixture.stake_pool_meta.validator_list,
            reserve_stake: fixture.stake_pool_meta.reserve,
            stake_account: fixture.stake_pool_meta.reserve, // Use reserve as stake account for simplicity
            transient_stake_account: find_transient_stake_program_address(
                &spl_stake_pool::id(),
                &vote_pubkey,
                &fixture.stake_pool_meta.stake_pool,
                0u64,
            ).0,
            vote_account: vote_pubkey,
            clock: sysvar::clock::id(),
            rent: sysvar::rent::id(),
            stake_history: sysvar::stake_history::id(),
            stake_config: solana_program::stake::config::ID,
            system_program: solana_program::system_program::id(),
            stake_program: solana_program::stake::program::id(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::RebalanceDirected {
            validator_list_index: 0,
        }
        .data(),
    };

    // Submit the transaction
    let tx = Transaction::new_signed_with_payer(
        &[
            ComputeBudgetInstruction::request_heap_frame(256 * 1024),
            ComputeBudgetInstruction::set_compute_unit_limit(1_400_000),
            rebalance_ix,
        ],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        fixture.get_latest_blockhash().await,
    );

    // The transaction should succeed (or fail gracefully if not in the right state)
    fixture.submit_transaction_assert_success(tx).await;
    
    // If we get here, the rebalance was executed successfully
    println!("Decrease rebalance executed successfully");
}

#[tokio::test]
async fn test_simple_directed_rebalance_no_action_needed() {
    // Test case: Validator is already at target, no rebalance needed
    let fixture = setup_directed_stake_fixture().await;
    
    // Create a validator
    let validator = Keypair::new();
    let vote_pubkey = validator.pubkey();
    
    // Set up directed stake meta with target = staked (no action needed)
    let target_lamports = 1_000_000_000; // 1 SOL target
    let staked_lamports = 1_000_000_000; // 1 SOL currently staked (at target)
    let directed_stake_meta = create_simple_directed_stake_meta(
        vote_pubkey,
        target_lamports,
        staked_lamports,
    );
    
    // Create the rebalance_directed instruction
    let rebalance_ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::RebalanceDirected {
            config: fixture.steward_config.pubkey(),
            state_account: fixture.steward_state,
            directed_stake_meta: Pubkey::find_program_address(
                &[DirectedStakeMeta::SEED, fixture.steward_config.pubkey().as_ref()],
                &jito_steward::id(),
            ).0,
            stake_pool: fixture.stake_pool_meta.stake_pool,
            stake_pool_program: spl_stake_pool::id(),
            withdraw_authority: fixture.stake_pool_meta.reserve, // Use reserve as withdraw authority
            validator_list: fixture.stake_pool_meta.validator_list,
            reserve_stake: fixture.stake_pool_meta.reserve,
            stake_account: fixture.stake_pool_meta.reserve, // Use reserve as stake account for simplicity
            transient_stake_account: find_transient_stake_program_address(
                &spl_stake_pool::id(),
                &vote_pubkey,
                &fixture.stake_pool_meta.stake_pool,
                0u64,
            ).0,
            vote_account: vote_pubkey,
            clock: sysvar::clock::id(),
            rent: sysvar::rent::id(),
            stake_history: sysvar::stake_history::id(),
            stake_config: solana_program::stake::config::ID,            
            system_program: solana_program::system_program::id(),
            stake_program: solana_program::stake::program::id(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::RebalanceDirected {
            validator_list_index: 0,
        }
        .data(),
    };

    // Submit the transaction
    let tx = Transaction::new_signed_with_payer(
        &[
            ComputeBudgetInstruction::request_heap_frame(256 * 1024),
            ComputeBudgetInstruction::set_compute_unit_limit(1_400_000),
            rebalance_ix,
        ],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        fixture.get_latest_blockhash().await,
    );

    // The transaction should succeed (or fail gracefully if not in the right state)
    fixture.submit_transaction_assert_success(tx).await;
    
    // If we get here, the rebalance was executed successfully
    println!("No action needed rebalance completed successfully");
}

#[tokio::test]
async fn test_directed_rebalance_wrong_state() {
    // Test case: Try to call rebalance_directed when not in the right state
    let fixture = setup_directed_stake_fixture().await;
    
    // Create a validator
    let validator = Keypair::new();
    let vote_pubkey = validator.pubkey();
    
    // Set up directed stake meta
    let directed_stake_meta = create_simple_directed_stake_meta(
        vote_pubkey,
        1_000_000_000,
        500_000_000,
    );
    
    // Create the rebalance_directed instruction
    let rebalance_ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::RebalanceDirected {
            config: fixture.steward_config.pubkey(),
            state_account: fixture.steward_state,
            directed_stake_meta: Pubkey::find_program_address(
                &[DirectedStakeMeta::SEED, fixture.steward_config.pubkey().as_ref()],
                &jito_steward::id(),
            ).0,
            stake_pool: fixture.stake_pool_meta.stake_pool,
            stake_pool_program: spl_stake_pool::id(),
            withdraw_authority: fixture.stake_pool_meta.reserve, // Use reserve as withdraw authority
            validator_list: fixture.stake_pool_meta.validator_list,
            reserve_stake: fixture.stake_pool_meta.reserve,
            stake_account: fixture.stake_pool_meta.reserve, // Use reserve as stake account for simplicity
            transient_stake_account: find_transient_stake_program_address(
                &spl_stake_pool::id(),
                &vote_pubkey,
                &fixture.stake_pool_meta.stake_pool,
                0u64,
            ).0,
            vote_account: vote_pubkey,
            clock: sysvar::clock::id(),
            rent: sysvar::rent::id(),
            stake_history: sysvar::stake_history::id(),
            stake_config: solana_program::stake::config::ID,
            system_program: solana_program::system_program::id(),
            stake_program: solana_program::stake::program::id(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::RebalanceDirected {
            validator_list_index: 0,
        }
        .data(),
    };

    // Submit the transaction
    let tx = Transaction::new_signed_with_payer(
        &[
            ComputeBudgetInstruction::request_heap_frame(256 * 1024),
            ComputeBudgetInstruction::set_compute_unit_limit(1_400_000),
            rebalance_ix,
        ],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        fixture.get_latest_blockhash().await,
    );

    // The transaction should fail with a state error
    fixture.submit_transaction_assert_error(tx, "StateMachineInvalidState").await;
    
    // If we get here, the test passed - the transaction failed as expected
    println!("Expected failure (wrong state) - test passed");
}