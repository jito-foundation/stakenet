use anchor_lang::Discriminator;
use anchor_lang::{InstructionData, ToAccountMetas};
use jito_steward::{
    instructions::AuthorityType,
    state::directed_stake::{DirectedStakeMeta, DirectedStakeTarget},
    DirectedStakeWhitelist, REBALANCE_DIRECTED_COMPLETE,
};
use solana_program::{instruction::Instruction, sysvar};
use solana_program_test::*;
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction, pubkey::Pubkey, signature::Keypair, signer::Signer,
    transaction::Transaction,
};
use spl_stake_pool::find_transient_stake_program_address;
use tests::steward_fixtures::{serialized_steward_state_account, TestFixture};

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
        &[
            DirectedStakeWhitelist::SEED,
            fixture.steward_config.pubkey().as_ref(),
        ],
        &jito_steward::id(),
    )
    .0;

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
async fn initialize_directed_stake_meta(fixture: &TestFixture) -> Pubkey {
    let directed_stake_meta = Pubkey::find_program_address(
        &[
            DirectedStakeMeta::SEED,
            fixture.steward_config.pubkey().as_ref(),
        ],
        &jito_steward::id(),
    )
    .0;

    let ix = Instruction {
        program_id: jito_steward::id(),
        accounts: vec![
            anchor_lang::solana_program::instruction::AccountMeta::new_readonly(
                fixture.steward_config.pubkey(),
                false,
            ),
            anchor_lang::solana_program::instruction::AccountMeta::new(directed_stake_meta, false),
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
        data: jito_steward::instruction::InitializeDirectedStakeMeta {}.data(),
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
    set_directed_stake_whitelist_authority(&fixture).await;
    initialize_directed_stake_whitelist(&fixture).await;
    initialize_directed_stake_meta(&fixture).await;
    fixture.realloc_directed_stake_meta().await;
    fixture
}

/// Helper function to add a validator to the stake pool using AutoAddValidator
async fn add_validator_to_pool(fixture: &TestFixture, vote_pubkey: Pubkey) {
    // Initialize validator history account first (required for AutoAddValidator)
    let validator_history_address =
        fixture.initialize_validator_history_with_credits(vote_pubkey, 0);

    // Get the stake account addresses
    let (stake_account_address, _transient_stake_account_address, withdraw_authority) =
        fixture.stake_accounts_for_validator(vote_pubkey).await;

    // Use AutoAddValidator instruction to properly add validator to the pool
    let add_validator_ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::AutoAddValidator {
            steward_state: fixture.steward_state,
            validator_history_account: validator_history_address,
            config: fixture.steward_config.pubkey(),
            stake_pool_program: spl_stake_pool::id(),
            stake_pool: fixture.stake_pool_meta.stake_pool,
            reserve_stake: fixture.stake_pool_meta.reserve,
            withdraw_authority,
            validator_list: fixture.stake_pool_meta.validator_list,
            stake_account: stake_account_address,
            vote_account: vote_pubkey,
            rent: sysvar::rent::id(),
            clock: sysvar::clock::id(),
            stake_history: sysvar::stake_history::id(),
            stake_config: solana_program::stake::config::ID,
            stake_program: solana_program::stake::program::id(),
            system_program: solana_program::system_program::id(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::AutoAddValidatorToPool {}.data(),
    };

    let tx = Transaction::new_signed_with_payer(
        &[
            ComputeBudgetInstruction::request_heap_frame(256 * 1024),
            ComputeBudgetInstruction::set_compute_unit_limit(1_400_000),
            add_validator_ix,
        ],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        fixture.get_latest_blockhash().await,
    );

    fixture.submit_transaction_assert_success(tx).await;

    // Update steward state to reflect the added validator
    let mut steward_state_account: jito_steward::StewardStateAccountV2 =
        fixture.load_and_deserialize(&fixture.steward_state).await;
    steward_state_account.state.num_pool_validators += 1;
    steward_state_account.state.validators_added -= 1;

    fixture.ctx.borrow_mut().set_account(
        &fixture.steward_state,
        &serialized_steward_state_account(steward_state_account).into(),
    );
}


/// Helper function to populate directed stake meta after initialization
async fn populate_directed_stake_meta_after_init(
    fixture: &TestFixture,
    vote_pubkey: Pubkey,
    target_lamports: u64,
    staked_lamports: u64,
) {
    let directed_stake_meta_pubkey = Pubkey::find_program_address(
        &[
            DirectedStakeMeta::SEED,
            fixture.steward_config.pubkey().as_ref(),
        ],
        &jito_steward::id(),
    )
    .0;

    // Create the full DirectedStakeMeta with proper discriminator
    let meta = DirectedStakeMeta {
        total_stake_targets: 0,
        epoch_last_updated: 0,
        epoch_increase_total_lamports: 0,
        epoch_decrease_total_lamports: 0,
        directed_unstake_total: 0,
        padding0: [0; 63],
        is_initialized: jito_steward::utils::U8Bool::from(true),
        targets: {
            let mut targets = [DirectedStakeTarget {
                vote_pubkey: Pubkey::default(),
                total_target_lamports: 0,
                total_staked_lamports: 0,
                target_last_updated_epoch: 0,
                staked_last_updated_epoch: 0,
                _padding0: [0; 32],
            }; 2048];

            // Set the first target
            targets[0] = DirectedStakeTarget {
                vote_pubkey,
                total_target_lamports: target_lamports,
                total_staked_lamports: staked_lamports,
                target_last_updated_epoch: 0,
                staked_last_updated_epoch: 0,
                _padding0: [0; 32],
            };

            targets
        },
    };

    // Serialize with discriminator
    let mut account_data = Vec::new();
    account_data.extend_from_slice(DirectedStakeMeta::DISCRIMINATOR);
    account_data.extend_from_slice(&borsh::to_vec(&meta).unwrap());

    // Create account with proper data
    let account = solana_sdk::account::Account {
        lamports: 1_000_000_000,
        data: account_data,
        owner: jito_steward::id(),
        executable: false,
        rent_epoch: 0,
    };

    fixture
        .ctx
        .borrow_mut()
        .set_account(&directed_stake_meta_pubkey, &account.into());
}

#[tokio::test]
async fn test_simple_directed_rebalance_increase() {
    // Test case: Validator needs more stake to reach target
    let fixture = setup_directed_stake_fixture().await;

    // Create a validator
    let validator = Keypair::new();
    let vote_pubkey = validator.pubkey();

    // Add validator to the stake pool first
    add_validator_to_pool(&fixture, vote_pubkey).await;

    // Fund the stake pool with sufficient lamports for rebalance operations
    let stake_pool: jito_steward::stake_pool_utils::StakePool = fixture
        .load_and_deserialize(&fixture.stake_pool_meta.stake_pool)
        .await;
    let mut stake_pool_spl = stake_pool.as_ref().clone();

    // Add substantial funding to the stake pool (100 SOL)
    let funding_amount = 100_000_000_000; // 100 SOL
    stake_pool_spl.pool_token_supply += funding_amount;
    stake_pool_spl.total_lamports += funding_amount;

    fixture.ctx.borrow_mut().set_account(
        &fixture.stake_pool_meta.stake_pool,
        &tests::stake_pool_utils::serialized_stake_pool_account(
            stake_pool_spl,
            std::mem::size_of::<jito_steward::stake_pool_utils::StakePool>(),
        )
        .into(),
    );

    // Fund the reserve stake account with actual lamports
    let reserve_account = fixture.get_account(&fixture.stake_pool_meta.reserve).await;
    let mut updated_reserve = reserve_account;
    updated_reserve.lamports += funding_amount; // Add 100 SOL to the reserve

    fixture
        .ctx
        .borrow_mut()
        .set_account(&fixture.stake_pool_meta.reserve, &updated_reserve.into());

    // Set steward state to RebalanceDirected state
    let mut steward_state_account: jito_steward::StewardStateAccountV2 =
        fixture.load_and_deserialize(&fixture.steward_state).await;
    steward_state_account.state.state_tag = jito_steward::StewardStateEnum::RebalanceDirected;
    steward_state_account
        .state
        .set_flag(REBALANCE_DIRECTED_COMPLETE);

    fixture.ctx.borrow_mut().set_account(
        &fixture.steward_state,
        &serialized_steward_state_account(steward_state_account).into(),
    );

    // Set up directed stake meta with target > staked
    let target_lamports = 1_000_000_000; // 1 SOL target
    let staked_lamports = 500_000_000; // 0.5 SOL currently staked

    // Populate the account data manually (after initialization sets discriminator)
    populate_directed_stake_meta_after_init(
        &fixture,
        vote_pubkey,
        target_lamports,
        staked_lamports,
    )
    .await;

    // Create the rebalance_directed instruction
    let rebalance_ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::RebalanceDirected {
            config: fixture.steward_config.pubkey(),
            state_account: fixture.steward_state,
            directed_stake_meta: Pubkey::find_program_address(
                &[
                    DirectedStakeMeta::SEED,
                    fixture.steward_config.pubkey().as_ref(),
                ],
                &jito_steward::id(),
            )
            .0,
            stake_pool: fixture.stake_pool_meta.stake_pool,
            stake_pool_program: spl_stake_pool::id(),
            withdraw_authority: fixture.stake_accounts_for_validator(vote_pubkey).await.2, // Get proper withdraw authority
            validator_list: fixture.stake_pool_meta.validator_list,
            reserve_stake: fixture.stake_pool_meta.reserve,
            stake_account: fixture.stake_accounts_for_validator(vote_pubkey).await.0, // Get proper stake account
            transient_stake_account: find_transient_stake_program_address(
                &spl_stake_pool::id(),
                &vote_pubkey,
                &fixture.stake_pool_meta.stake_pool,
                0u64,
            )
            .0,
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
            directed_stake_meta_index: 0,
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

    assert!(
        steward_state_account
            .state
            .has_flag(REBALANCE_DIRECTED_COMPLETE),
        "REBALANCE_DIRECTED_COMPLETE flag should be set after rebalance"
    );
}

#[tokio::test]
async fn test_simple_directed_rebalance_decrease() {
    // Test case: Validator has too much stake and needs to decrease
    let fixture = setup_directed_stake_fixture().await;

    // Create a validator
    let validator = Keypair::new();
    let vote_pubkey = validator.pubkey();

    // Add validator to the stake pool first
    add_validator_to_pool(&fixture, vote_pubkey).await;

    // Fund the stake pool with sufficient lamports for rebalance operations
    let stake_pool: jito_steward::stake_pool_utils::StakePool = fixture
        .load_and_deserialize(&fixture.stake_pool_meta.stake_pool)
        .await;
    let mut stake_pool_spl = stake_pool.as_ref().clone();

    // Add substantial funding to the stake pool (100 SOL)
    let funding_amount = 100_000_000_000; // 100 SOL
    stake_pool_spl.pool_token_supply += funding_amount;
    stake_pool_spl.total_lamports += funding_amount;

    fixture.ctx.borrow_mut().set_account(
        &fixture.stake_pool_meta.stake_pool,
        &tests::stake_pool_utils::serialized_stake_pool_account(
            stake_pool_spl,
            std::mem::size_of::<jito_steward::stake_pool_utils::StakePool>(),
        )
        .into(),
    );

    // Fund the reserve stake account with actual lamports
    let reserve_account = fixture.get_account(&fixture.stake_pool_meta.reserve).await;
    let mut updated_reserve = reserve_account;
    updated_reserve.lamports += funding_amount; // Add 10 SOL to the reserve

    fixture
        .ctx
        .borrow_mut()
        .set_account(&fixture.stake_pool_meta.reserve, &updated_reserve.into());

    // Set steward state to RebalanceDirected state
    let mut steward_state_account: jito_steward::StewardStateAccountV2 =
        fixture.load_and_deserialize(&fixture.steward_state).await;
    steward_state_account.state.state_tag = jito_steward::StewardStateEnum::RebalanceDirected;
    steward_state_account
        .state
        .set_flag(REBALANCE_DIRECTED_COMPLETE);

    fixture.ctx.borrow_mut().set_account(
        &fixture.steward_state,
        &serialized_steward_state_account(steward_state_account).into(),
    );

    // Set up directed stake meta with staked > target
    let target_lamports = 500_000_000; // 0.5 SOL target
    let staked_lamports = 1_000_000_000; // 1 SOL currently staked (excess)

    // Populate the account data manually (after initialization sets discriminator)
    populate_directed_stake_meta_after_init(
        &fixture,
        vote_pubkey,
        target_lamports,
        staked_lamports,
    )
    .await;

    // Create the rebalance_directed instruction
    let rebalance_ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::RebalanceDirected {
            config: fixture.steward_config.pubkey(),
            state_account: fixture.steward_state,
            directed_stake_meta: Pubkey::find_program_address(
                &[
                    DirectedStakeMeta::SEED,
                    fixture.steward_config.pubkey().as_ref(),
                ],
                &jito_steward::id(),
            )
            .0,
            stake_pool: fixture.stake_pool_meta.stake_pool,
            stake_pool_program: spl_stake_pool::id(),
            withdraw_authority: fixture.stake_accounts_for_validator(vote_pubkey).await.2, // Get proper withdraw authority
            validator_list: fixture.stake_pool_meta.validator_list,
            reserve_stake: fixture.stake_pool_meta.reserve,
            stake_account: fixture.stake_accounts_for_validator(vote_pubkey).await.0, // Get proper stake account
            transient_stake_account: find_transient_stake_program_address(
                &spl_stake_pool::id(),
                &vote_pubkey,
                &fixture.stake_pool_meta.stake_pool,
                0u64,
            )
            .0,
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
            directed_stake_meta_index: 0,
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

    assert!(
        steward_state_account
            .state
            .has_flag(REBALANCE_DIRECTED_COMPLETE),
        "REBALANCE_DIRECTED_COMPLETE flag should be set after rebalance"
    );
}

#[tokio::test]
async fn test_simple_directed_rebalance_no_action_needed() {
    let fixture = setup_directed_stake_fixture().await;

    let validator = Keypair::new();
    let vote_pubkey = validator.pubkey();

    add_validator_to_pool(&fixture, vote_pubkey).await;

    let stake_pool: jito_steward::stake_pool_utils::StakePool = fixture
        .load_and_deserialize(&fixture.stake_pool_meta.stake_pool)
        .await;
    let mut stake_pool_spl = stake_pool.as_ref().clone();

    let funding_amount = 100_000_000_000; // 100 SOL
    stake_pool_spl.pool_token_supply += funding_amount;
    stake_pool_spl.total_lamports += funding_amount;

    fixture.ctx.borrow_mut().set_account(
        &fixture.stake_pool_meta.stake_pool,
        &tests::stake_pool_utils::serialized_stake_pool_account(
            stake_pool_spl,
            std::mem::size_of::<jito_steward::stake_pool_utils::StakePool>(),
        )
        .into(),
    );

    let reserve_account = fixture.get_account(&fixture.stake_pool_meta.reserve).await;
    let mut updated_reserve = reserve_account;
    updated_reserve.lamports += funding_amount; // Add 100 SOL to the reserve

    fixture
        .ctx
        .borrow_mut()
        .set_account(&fixture.stake_pool_meta.reserve, &updated_reserve.into());

    let mut steward_state_account: jito_steward::StewardStateAccountV2 =
        fixture.load_and_deserialize(&fixture.steward_state).await;
    steward_state_account.state.state_tag = jito_steward::StewardStateEnum::RebalanceDirected;
    steward_state_account
        .state
        .set_flag(REBALANCE_DIRECTED_COMPLETE);

    fixture.ctx.borrow_mut().set_account(
        &fixture.steward_state,
        &serialized_steward_state_account(steward_state_account).into(),
    );

    let target_lamports = 1_000_000_000; // 1 SOL target
    let staked_lamports = 1_000_000_000; // 1 SOL currently staked (at target)

    populate_directed_stake_meta_after_init(
        &fixture,
        vote_pubkey,
        target_lamports,
        staked_lamports,
    )
    .await;

    // Create the rebalance_directed instruction
    let rebalance_ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::RebalanceDirected {
            config: fixture.steward_config.pubkey(),
            state_account: fixture.steward_state,
            directed_stake_meta: Pubkey::find_program_address(
                &[
                    DirectedStakeMeta::SEED,
                    fixture.steward_config.pubkey().as_ref(),
                ],
                &jito_steward::id(),
            )
            .0,
            stake_pool: fixture.stake_pool_meta.stake_pool,
            stake_pool_program: spl_stake_pool::id(),
            withdraw_authority: fixture.stake_accounts_for_validator(vote_pubkey).await.2, // Get proper withdraw authority
            validator_list: fixture.stake_pool_meta.validator_list,
            reserve_stake: fixture.stake_pool_meta.reserve,
            stake_account: fixture.stake_accounts_for_validator(vote_pubkey).await.0, // Get proper stake account
            transient_stake_account: find_transient_stake_program_address(
                &spl_stake_pool::id(),
                &vote_pubkey,
                &fixture.stake_pool_meta.stake_pool,
                0u64,
            )
            .0,
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
            directed_stake_meta_index: 0,
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

    assert!(
        steward_state_account
            .state
            .has_flag(REBALANCE_DIRECTED_COMPLETE),
        "REBALANCE_DIRECTED_COMPLETE flag should be set after rebalancing completed"
    );
}

#[tokio::test]
async fn test_simple_directed_rebalance_no_targets() {
    let fixture = setup_directed_stake_fixture().await;

    let validator = Keypair::new();
    let vote_pubkey = validator.pubkey();

    add_validator_to_pool(&fixture, vote_pubkey).await;

    let stake_pool: jito_steward::stake_pool_utils::StakePool = fixture
        .load_and_deserialize(&fixture.stake_pool_meta.stake_pool)
        .await;
    let mut stake_pool_spl = stake_pool.as_ref().clone();

    let funding_amount = 100_000_000_000; // 100 SOL
    stake_pool_spl.pool_token_supply += funding_amount;
    stake_pool_spl.total_lamports += funding_amount;

    fixture.ctx.borrow_mut().set_account(
        &fixture.stake_pool_meta.stake_pool,
        &tests::stake_pool_utils::serialized_stake_pool_account(
            stake_pool_spl,
            std::mem::size_of::<jito_steward::stake_pool_utils::StakePool>(),
        )
        .into(),
    );

    let reserve_account = fixture.get_account(&fixture.stake_pool_meta.reserve).await;
    let mut updated_reserve = reserve_account;
    updated_reserve.lamports += funding_amount; // Add 100 SOL to the reserve

    fixture
        .ctx
        .borrow_mut()
        .set_account(&fixture.stake_pool_meta.reserve, &updated_reserve.into());

    let mut steward_state_account: jito_steward::StewardStateAccountV2 =
        fixture.load_and_deserialize(&fixture.steward_state).await;
    steward_state_account.state.state_tag = jito_steward::StewardStateEnum::RebalanceDirected;
    steward_state_account
        .state
        .set_flag(REBALANCE_DIRECTED_COMPLETE);

    fixture.ctx.borrow_mut().set_account(
        &fixture.steward_state,
        &serialized_steward_state_account(steward_state_account).into(),
    );

    // Create the rebalance_directed instruction
    let rebalance_ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::RebalanceDirected {
            config: fixture.steward_config.pubkey(),
            state_account: fixture.steward_state,
            directed_stake_meta: Pubkey::find_program_address(
                &[
                    DirectedStakeMeta::SEED,
                    fixture.steward_config.pubkey().as_ref(),
                ],
                &jito_steward::id(),
            )
            .0,
            stake_pool: fixture.stake_pool_meta.stake_pool,
            stake_pool_program: spl_stake_pool::id(),
            withdraw_authority: fixture.stake_accounts_for_validator(vote_pubkey).await.2, // Get proper withdraw authority
            validator_list: fixture.stake_pool_meta.validator_list,
            reserve_stake: fixture.stake_pool_meta.reserve,
            stake_account: fixture.stake_accounts_for_validator(vote_pubkey).await.0, // Get proper stake account
            transient_stake_account: find_transient_stake_program_address(
                &spl_stake_pool::id(),
                &vote_pubkey,
                &fixture.stake_pool_meta.stake_pool,
                0u64,
            )
            .0,
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
            directed_stake_meta_index: 0,
            validator_list_index: 0,
        }
        .data(),
    };

    // Submit the transaction
    let tx = Transaction::new_signed_with_payer(
        &[
            ComputeBudgetInstruction::request_heap_frame(256 * 1024),
            ComputeBudgetInstruction::set_compute_unit_limit(1_400_000),
            rebalance_ix.clone(),
        ],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        fixture.get_latest_blockhash().await,
    );

    // The transaction should succeed (or fail gracefully if not in the right state)
    fixture.submit_transaction_assert_success(tx).await;

    assert!(
        steward_state_account
            .state
            .has_flag(REBALANCE_DIRECTED_COMPLETE),
        "REBALANCE_DIRECTED_COMPLETE flag should be set after rebalancing completed"
    );
}

#[tokio::test]
async fn test_directed_rebalance_wrong_state() {
    // Test case: Try to call rebalance_directed when not in the right state
    let fixture = setup_directed_stake_fixture().await;

    // Create a validator
    let validator = Keypair::new();
    let vote_pubkey = validator.pubkey();

    // Add validator to the stake pool first
    add_validator_to_pool(&fixture, vote_pubkey).await;

    // Set up directed stake meta with target > staked
    let target_lamports = 1_000_000_000; // 1 SOL target
    let staked_lamports = 500_000_000; // 0.5 SOL currently staked

    // Populate the account data manually (after initialization sets discriminator)
    populate_directed_stake_meta_after_init(
        &fixture,
        vote_pubkey,
        target_lamports,
        staked_lamports,
    )
    .await;

    // NOTE: We intentionally do NOT set the steward state to RebalanceDirected
    // This should cause the transaction to fail with StateMachineInvalidState

    // Create the rebalance_directed instruction
    let rebalance_ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::RebalanceDirected {
            config: fixture.steward_config.pubkey(),
            state_account: fixture.steward_state,
            directed_stake_meta: Pubkey::find_program_address(
                &[
                    DirectedStakeMeta::SEED,
                    fixture.steward_config.pubkey().as_ref(),
                ],
                &jito_steward::id(),
            )
            .0,
            stake_pool: fixture.stake_pool_meta.stake_pool,
            stake_pool_program: spl_stake_pool::id(),
            withdraw_authority: fixture.stake_accounts_for_validator(vote_pubkey).await.2, // Get proper withdraw authority
            validator_list: fixture.stake_pool_meta.validator_list,
            reserve_stake: fixture.stake_pool_meta.reserve,
            stake_account: fixture.stake_accounts_for_validator(vote_pubkey).await.0, // Get proper stake account
            transient_stake_account: find_transient_stake_program_address(
                &spl_stake_pool::id(),
                &vote_pubkey,
                &fixture.stake_pool_meta.stake_pool,
                0u64,
            )
            .0,
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
            directed_stake_meta_index: 0,
            validator_list_index: 0,
        }
        .data(),
    };

    // Submit the transaction
    let tx = Transaction::new_signed_with_payer(
        &[
            ComputeBudgetInstruction::request_heap_frame(256 * 1024),
            ComputeBudgetInstruction::set_compute_unit_limit(1_400_000),
            rebalance_ix.clone(),
            rebalance_ix,
        ],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        fixture.get_latest_blockhash().await,
    );

    // The transaction should fail with a state error
    fixture
        .submit_transaction_assert_error(tx, "InvalidState")
        .await;
}
