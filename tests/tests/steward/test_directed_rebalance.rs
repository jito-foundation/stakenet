use anchor_lang::{InstructionData, ToAccountMetas};
use jito_steward::{
    instructions::AuthorityType,
    state::directed_stake::{DirectedStakeMeta, DirectedStakeTarget},
    DirectedStakeWhitelist,
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
        &[DirectedStakeWhitelist::SEED, fixture.steward_config.pubkey().as_ref()],
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

/// Helper function to initialize directed stake meta
async fn initialize_directed_stake_meta(
    fixture: &TestFixture,
    total_stake_targets: u16,
) -> Pubkey {
    let directed_stake_meta = Pubkey::find_program_address(
        &[DirectedStakeMeta::SEED, fixture.steward_config.pubkey().as_ref()],
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
                directed_stake_meta,
                false,
            ),
            anchor_lang::solana_program::instruction::AccountMeta::new_readonly(
                sysvar::clock::id(),
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
        data: jito_steward::instruction::InitializeDirectedStakeMeta {
            total_stake_targets,
        }.data(),
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        fixture.ctx.borrow().last_blockhash,
    );

    fixture.submit_transaction_assert_success(tx).await;
    
    directed_stake_meta
}

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

    // Initialize the directed stake meta account
    initialize_directed_stake_meta(&fixture, 1).await;
    println!("Directed stake meta initialized");
    realloc_directed_stake_meta(&fixture).await;
    println!("Directed stake meta reallocated");

    fixture
}


/// Helper function to reallocate directed stake meta to proper size
async fn realloc_directed_stake_meta(fixture: &TestFixture) {
    let directed_stake_meta = Pubkey::find_program_address(
        &[DirectedStakeMeta::SEED, fixture.steward_config.pubkey().as_ref()],
        &jito_steward::id(),
    ).0;

    // Get the validator list address from the config
    let config: jito_steward::Config = fixture.load_and_deserialize(&fixture.steward_config.pubkey()).await;
    let validator_list = config.validator_list;

    // Calculate how many reallocations we need
    let mut num_reallocs = (DirectedStakeMeta::SIZE - jito_steward::constants::MAX_ALLOC_BYTES) / jito_steward::constants::MAX_ALLOC_BYTES + 1;
    let mut ixs = vec![];

    while num_reallocs > 0 {
        ixs.extend(vec![
            Instruction {
                program_id: jito_steward::id(),
                accounts: vec![
                    anchor_lang::solana_program::instruction::AccountMeta::new(
                        directed_stake_meta,
                        false,
                    ),
                    anchor_lang::solana_program::instruction::AccountMeta::new_readonly(
                        fixture.steward_config.pubkey(),
                        false,
                    ),
                    anchor_lang::solana_program::instruction::AccountMeta::new_readonly(
                        validator_list,
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
                data: jito_steward::instruction::ReallocDirectedStakeMeta {}.data(),
            };
            num_reallocs.min(10)
        ]);
        num_reallocs = num_reallocs.saturating_sub(10);
    }

    // Submit all reallocation instructions
    let tx = Transaction::new_signed_with_payer(
        &ixs,
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        fixture.ctx.borrow().last_blockhash,
    );
    fixture.submit_transaction_assert_success(tx).await;
}

/// Helper function to populate directed stake meta with target data
async fn populate_directed_stake_meta(
    fixture: &TestFixture,
    vote_pubkey: Pubkey,
    target_lamports: u64,
    staked_lamports: u64,
) {
    let directed_stake_meta = Pubkey::find_program_address(
        &[DirectedStakeMeta::SEED, fixture.steward_config.pubkey().as_ref()],
        &jito_steward::id(),
    ).0;

    // Create the target data
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

    // Write the data to the account using the context
    let mut ctx = fixture.ctx.borrow_mut();
    let mut account_data = borsh::to_vec(&meta).unwrap();
    
    // Add the Anchor discriminator at the beginning
    // The discriminator is the first 8 bytes of SHA256("account:DirectedStakeMeta")
    let discriminator = [0x8a, 0x5a, 0x5c, 0x5c, 0x5c, 0x5c, 0x5c, 0x5c]; // This is a placeholder - we need the actual discriminator
    let mut full_data = Vec::new();
    full_data.extend_from_slice(&discriminator);
    full_data.extend_from_slice(&account_data);
    
    let account = solana_sdk::account::Account {
        lamports: 1_000_000_000, // 1 SOL for rent
        data: full_data,
        owner: jito_steward::id(),
        executable: false,
        rent_epoch: 0,
    };
    ctx.set_account(&directed_stake_meta, &account.into());
}

/// Helper function to populate directed stake meta after initialization
async fn populate_directed_stake_meta_after_init(
    fixture: &TestFixture,
    vote_pubkey: Pubkey,
    target_lamports: u64,
    staked_lamports: u64,
) {
    println!("Populating directed stake meta after initialization");
    let directed_stake_meta_pubkey = Pubkey::find_program_address(
        &[DirectedStakeMeta::SEED, fixture.steward_config.pubkey().as_ref()],
        &jito_steward::id(),
    ).0;

    // Read the existing account to preserve the discriminator
    let ctx = fixture.ctx.borrow();
    let mut existing_account = ctx.banks_client
        .get_account(directed_stake_meta_pubkey)
        .await
        .unwrap()
        .unwrap();
    
    // The account should already have the discriminator set by initialize_directed_stake_meta
    // We just need to update the targets portion
    // Layout: discriminator (8) + epoch (8) + total_stake_targets (2) + uploaded_stake_targets (2) + _padding0 (132) = 152 bytes header
    
    let header_size = 8 + 8 + 2 + 2 + 132; // 152 bytes
    
    // Create a single target
    let target = DirectedStakeTarget {
        vote_pubkey,
        total_target_lamports: target_lamports,
        total_staked_lamports: staked_lamports,
        _padding0: [0; 64],
    };
    
    // Serialize the target and write it to the correct position
    let target_bytes = borsh::to_vec(&target).unwrap();
    existing_account.data[header_size..header_size + target_bytes.len()].copy_from_slice(&target_bytes);
    
    // Update uploaded_stake_targets to 1 (at offset 8 + 8 + 2 = 18)
    existing_account.data[18..20].copy_from_slice(&1u16.to_le_bytes());
    
    drop(ctx);
    fixture.ctx.borrow_mut().set_account(&directed_stake_meta_pubkey, &existing_account.into());
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
    
    // Populate the account data manually (after initialization sets discriminator)
    populate_directed_stake_meta_after_init(&fixture, vote_pubkey, target_lamports, staked_lamports).await;

    println!("Directed stake meta populated... rebalancing");
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
            withdraw_authority: fixture.stake_accounts_for_validator(vote_pubkey).await.2, // Get proper withdraw authority
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
    
    // Populate the account data manually (after initialization sets discriminator)
    populate_directed_stake_meta_after_init(&fixture, vote_pubkey, target_lamports, staked_lamports).await;
    
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
            withdraw_authority: fixture.stake_accounts_for_validator(vote_pubkey).await.2, // Get proper withdraw authority
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
            withdraw_authority: fixture.stake_accounts_for_validator(vote_pubkey).await.2, // Get proper withdraw authority
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
            withdraw_authority: fixture.stake_accounts_for_validator(vote_pubkey).await.2, // Get proper withdraw authority
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
    
    println!("Expected failure (wrong state) - test passed");
}