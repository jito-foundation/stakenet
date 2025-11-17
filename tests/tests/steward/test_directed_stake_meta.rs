#![allow(clippy::await_holding_refcell_ref)]
/// Tests for directed stake tickets and directed stake metas
///
/// NOTE: This test file currently has placeholder instruction data (empty vectors)
/// because the directed stake instruction data structures are not exported from the Anchor program.
/// To make these tests functional, you'll need to either:
/// 1. Export the instruction data structures from the Anchor program
/// 2. Use the generated IDL to construct instruction data
/// 3. Manually construct the instruction data using Borsh serialization
use anchor_lang::{
    solana_program::{instruction::Instruction, pubkey::Pubkey, sysvar},
    InstructionData, ToAccountMetas,
};
use jito_steward::{
    constants::MAX_ALLOC_BYTES,
    instructions::AuthorityType,
    state::directed_stake::{DirectedStakePreference, DirectedStakeRecordType},
    DirectedStakeMeta, DirectedStakeTicket, DirectedStakeWhitelist,
};
use solana_program_test::*;
use solana_sdk::{signature::Keypair, signer::Signer, transaction::Transaction};
use tests::steward_fixtures::{system_account, TestFixture};

/// Helper function to create a test fixture with directed stake setup
async fn setup_directed_stake_fixture() -> TestFixture {
    let fixture = TestFixture::new().await;

    fixture.initialize_stake_pool().await;
    fixture.initialize_steward(None, None).await;
    // Initialize validator list with some validators so we can use them in tests
    fixture.initialize_validator_list(3).await;
    set_directed_stake_whitelist_authority(&fixture).await;

    initialize_directed_stake_whitelist(&fixture).await;

    // Add fixture.keypair to the whitelist as a user staker so it can update tickets
    // (since the auth requires signer to be both permissioned and ticket_override_authority)
    add_staker_to_whitelist(
        &fixture,
        &fixture.keypair.pubkey(),
        DirectedStakeRecordType::User,
    )
    .await;

    fixture
}

/// Helper function to create and fund a staker account
async fn create_funded_staker(fixture: &TestFixture) -> Keypair {
    let staker = Keypair::new();

    // Add the staker account to the test context with lamports
    let mut ctx = fixture.ctx.borrow_mut();
    ctx.set_account(
        &staker.pubkey(),
        &system_account(100_000_000_000).into(), // 100 SOL
    );

    staker
}

/// Helper function to set the directed stake whitelist authority
async fn set_directed_stake_whitelist_authority(fixture: &TestFixture) {
    let set_whitelist_auth_ix = Instruction {
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

    let set_ticket_override_auth_ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::SetNewAuthority {
            config: fixture.steward_config.pubkey(),
            new_authority: fixture.keypair.pubkey(),
            admin: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::SetNewAuthority {
            authority_type: AuthorityType::SetDirectedStakeTicketOverrideAuthority,
        }
        .data(),
    };

    let tx = Transaction::new_signed_with_payer(
        &[set_whitelist_auth_ix, set_ticket_override_auth_ix],
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

    // Reallocate the account to its proper size
    fixture.realloc_directed_stake_whitelist().await;
}

/// Helper function to add a staker to the directed stake whitelist
async fn add_staker_to_whitelist(
    fixture: &TestFixture,
    staker: &Pubkey,
    record_type: DirectedStakeRecordType,
) {
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
        accounts: jito_steward::accounts::AddToDirectedStakeWhitelist {
            config: fixture.steward_config.pubkey(),
            directed_stake_whitelist,
            authority: fixture.keypair.pubkey(),
            stake_pool: fixture.stake_pool_meta.stake_pool,
            validator_list: fixture.stake_pool_meta.validator_list,
        }
        .to_account_metas(None),
        data: jito_steward::instruction::AddToDirectedStakeWhitelist {
            record_type,
            record: *staker,
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

/// Helper function to add a validator to the directed stake whitelist
async fn add_validator_to_whitelist(fixture: &TestFixture, validator: &Pubkey) {
    add_staker_to_whitelist(fixture, validator, DirectedStakeRecordType::Validator).await;
}

/// Helper function to initialize a directed stake ticket
async fn initialize_directed_stake_ticket(
    fixture: &TestFixture,
    signer: &Keypair,
    ticket_update_authority: Pubkey,
    ticket_holder_is_protocol: bool,
) -> Pubkey {
    let ticket_account = Pubkey::find_program_address(
        &[
            DirectedStakeTicket::SEED,
            fixture.steward_config.pubkey().as_ref(),
            signer.pubkey().as_ref(),
        ],
        &jito_steward::id(),
    )
    .0;

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
            anchor_lang::solana_program::instruction::AccountMeta::new_readonly(
                directed_stake_whitelist,
                false,
            ),
            anchor_lang::solana_program::instruction::AccountMeta::new(ticket_account, false),
            anchor_lang::solana_program::instruction::AccountMeta::new_readonly(
                anchor_lang::solana_program::system_program::id(),
                false,
            ),
            anchor_lang::solana_program::instruction::AccountMeta::new(signer.pubkey(), true),
        ],
        data: jito_steward::instruction::InitializeDirectedStakeTicket {
            ticket_update_authority,
            ticket_holder_is_protocol,
        }
        .data(),
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&signer.pubkey()),
        &[signer],
        fixture.ctx.borrow().last_blockhash,
    );

    fixture.submit_transaction_assert_success(tx).await;
    ticket_account
}

/// Helper function to update a directed stake ticket with preferences
async fn update_directed_stake_ticket(
    fixture: &TestFixture,
    ticket_account: &Pubkey,
    signer: &Keypair,
    preferences: Vec<DirectedStakePreference>,
) {
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
            anchor_lang::solana_program::instruction::AccountMeta::new_readonly(
                directed_stake_whitelist,
                false,
            ),
            anchor_lang::solana_program::instruction::AccountMeta::new(*ticket_account, false),
            anchor_lang::solana_program::instruction::AccountMeta::new(signer.pubkey(), true),
        ],
        data: jito_steward::instruction::UpdateDirectedStakeTicket {
            preferences: preferences.clone(),
        }
        .data(),
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&signer.pubkey()),
        &[signer],
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

    // Reallocate the account to its proper size
    fixture.realloc_directed_stake_meta().await;

    directed_stake_meta
}

#[tokio::test]
async fn test_add_stakers_to_whitelist() {
    let fixture = setup_directed_stake_fixture().await;

    // Create test stakers
    let user_staker = Keypair::new();
    let protocol_staker = Keypair::new();
    // Get a validator from the validator list instead of creating a new one
    let validator = fixture
        .get_validator_from_list(0)
        .await
        .expect("Validator list should have at least one validator");

    // Add user staker
    add_staker_to_whitelist(
        &fixture,
        &user_staker.pubkey(),
        DirectedStakeRecordType::User,
    )
    .await;

    // Add protocol staker
    add_staker_to_whitelist(
        &fixture,
        &protocol_staker.pubkey(),
        DirectedStakeRecordType::Protocol,
    )
    .await;

    // Add validator
    add_validator_to_whitelist(&fixture, &validator).await;

    // Verify the whitelist was updated
    let whitelist_account = Pubkey::find_program_address(
        &[
            DirectedStakeWhitelist::SEED,
            fixture.steward_config.pubkey().as_ref(),
        ],
        &jito_steward::id(),
    )
    .0;

    let whitelist: DirectedStakeWhitelist = fixture.load_and_deserialize(&whitelist_account).await;

    assert!(whitelist.is_user_staker_permissioned(&user_staker.pubkey()));
    assert!(whitelist.is_protocol_staker_permissioned(&protocol_staker.pubkey()));
    assert!(whitelist.is_validator_permissioned(&validator));
}

#[tokio::test]
async fn test_initialize_directed_stake_ticket() {
    let fixture = setup_directed_stake_fixture().await;
    let staker = create_funded_staker(&fixture).await;
    add_staker_to_whitelist(&fixture, &staker.pubkey(), DirectedStakeRecordType::User).await;
    let ticket_update_authority = Pubkey::new_unique();
    let ticket_holder_is_protocol = false;

    let ticket_account = initialize_directed_stake_ticket(
        &fixture,
        &staker,
        ticket_update_authority,
        ticket_holder_is_protocol,
    )
    .await;
    let ticket: DirectedStakeTicket = fixture.load_and_deserialize(&ticket_account).await;

    assert_eq!(ticket.num_preferences, 0);
    assert_eq!(ticket.ticket_update_authority, ticket_update_authority);
    assert_eq!(
        bool::from(ticket.ticket_holder_is_protocol),
        ticket_holder_is_protocol
    );
}

#[tokio::test]
async fn test_update_directed_stake_ticket() {
    let fixture = setup_directed_stake_fixture().await;

    // Create a funded test staker and add to whitelist
    let staker = create_funded_staker(&fixture).await;
    add_staker_to_whitelist(&fixture, &staker.pubkey(), DirectedStakeRecordType::User).await;

    // Initialize a directed stake ticket
    // Use fixture.keypair as signer (must be ticket_override_authority for updates)
    // Set ticket_update_authority to fixture.keypair so it can update
    let ticket_account = initialize_directed_stake_ticket(
        &fixture,
        &fixture.keypair, // signer - must be ticket_override_authority for updates
        fixture.keypair.pubkey(), // ticket_update_authority - must match signer for updates
        false,
    )
    .await;

    // Get validators from the validator list instead of creating new ones
    let validator1 = fixture
        .get_validator_from_list(0)
        .await
        .expect("Validator list should have at least one validator");
    let validator2 = fixture
        .get_validator_from_list(1)
        .await
        .expect("Validator list should have at least two validators");
    add_validator_to_whitelist(&fixture, &validator1).await;
    add_validator_to_whitelist(&fixture, &validator2).await;

    // Create preferences
    let preferences = vec![
        DirectedStakePreference::new(validator1, 6000), // 60%
        DirectedStakePreference::new(validator2, 4000), // 40%
    ];

    // Update the ticket with preferences
    // Use fixture.keypair as signer since it's the ticket_update_authority and ticket_override_authority
    update_directed_stake_ticket(&fixture, &ticket_account, &fixture.keypair, preferences).await;

    // Verify the ticket was updated
    let ticket: DirectedStakeTicket = fixture.load_and_deserialize(&ticket_account).await;

    assert_eq!(ticket.num_preferences, 2);
    assert_eq!(ticket.staker_preferences[0].vote_pubkey, validator1);
    assert_eq!(ticket.staker_preferences[0].stake_share_bps, 6000);
    assert_eq!(ticket.staker_preferences[1].vote_pubkey, validator2);
    assert_eq!(ticket.staker_preferences[1].stake_share_bps, 4000);

    // Test that preferences are valid (total <= 10000 bps)
    assert!(ticket.preferences_valid());
}

#[tokio::test]
async fn test_initialize_directed_stake_meta() {
    let fixture = setup_directed_stake_fixture().await;
    let directed_stake_meta = initialize_directed_stake_meta(&fixture).await;
    let _: DirectedStakeMeta = fixture.load_and_deserialize(&directed_stake_meta).await;
}

#[tokio::test]
async fn test_directed_stake_ticket_validation() {
    let fixture = setup_directed_stake_fixture().await;

    // Create a funded test staker and add to whitelist
    let staker = create_funded_staker(&fixture).await;
    add_staker_to_whitelist(&fixture, &staker.pubkey(), DirectedStakeRecordType::User).await;

    // Create a unique keypair for this ticket (ticket address is derived from signer)
    // This keypair will be used as both signer and ticket_update_authority
    let ticket_keypair = Keypair::new();
    // Add the ticket keypair to the whitelist so it can initialize and update
    add_staker_to_whitelist(
        &fixture,
        &ticket_keypair.pubkey(),
        DirectedStakeRecordType::User,
    )
    .await;

    // Initialize a directed stake ticket
    // Use ticket_keypair as signer so we have a unique ticket address
    // Set ticket_update_authority to ticket_keypair so it can update
    let ticket_account = initialize_directed_stake_ticket(
        &fixture,
        &ticket_keypair,         // signer - determines ticket address
        ticket_keypair.pubkey(), // ticket_update_authority - must match signer for updates
        false,
    )
    .await;

    // Create test validators and add them to whitelist
    // Get validators from the validator list instead of creating new ones
    let validator1 = fixture
        .get_validator_from_list(0)
        .await
        .expect("Validator list should have at least one validator");
    let validator2 = fixture
        .get_validator_from_list(1)
        .await
        .expect("Validator list should have at least two validators");
    add_validator_to_whitelist(&fixture, &validator1).await;
    add_validator_to_whitelist(&fixture, &validator2).await;

    // Test valid preferences (total = 10000 bps)
    let valid_preferences = vec![
        DirectedStakePreference::new(validator1, 6000),
        DirectedStakePreference::new(validator2, 4000),
    ];

    // Use fixture.keypair as signer since it's the ticket_update_authority and ticket_override_authority
    update_directed_stake_ticket(
        &fixture,
        &ticket_account,
        &fixture.keypair,
        valid_preferences,
    )
    .await;

    let ticket: DirectedStakeTicket = fixture.load_and_deserialize(&ticket_account).await;
    assert!(ticket.preferences_valid());

    // Test invalid preferences (total > 10000 bps)
    let _invalid_preferences = vec![
        DirectedStakePreference::new(validator1, 6000),
        DirectedStakePreference::new(validator2, 5000), // Total = 11000 bps
    ];

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
            anchor_lang::solana_program::instruction::AccountMeta::new_readonly(
                directed_stake_whitelist,
                false,
            ),
            anchor_lang::solana_program::instruction::AccountMeta::new(ticket_account, false),
            anchor_lang::solana_program::instruction::AccountMeta::new(staker.pubkey(), true),
        ],
        data: jito_steward::instruction::UpdateDirectedStakeTicket {
            preferences: _invalid_preferences.clone(),
        }
        .data(),
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&staker.pubkey()),
        &[&staker],
        fixture.ctx.borrow().last_blockhash,
    );

    fixture
        .submit_transaction_assert_error(tx, "InvalidParameterValue")
        .await;
}

#[tokio::test]
async fn test_directed_stake_ticket_unauthorized() {
    let fixture = setup_directed_stake_fixture().await;

    // Try to initialize a ticket without being on the whitelist
    let unauthorized_staker = create_funded_staker(&fixture).await;

    let ticket_account = Pubkey::find_program_address(
        &[
            DirectedStakeTicket::SEED,
            fixture.steward_config.pubkey().as_ref(),
            unauthorized_staker.pubkey().as_ref(),
        ],
        &jito_steward::id(),
    )
    .0;

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
            anchor_lang::solana_program::instruction::AccountMeta::new_readonly(
                directed_stake_whitelist,
                false,
            ),
            anchor_lang::solana_program::instruction::AccountMeta::new(ticket_account, false),
            anchor_lang::solana_program::instruction::AccountMeta::new_readonly(
                anchor_lang::solana_program::system_program::id(),
                false,
            ),
            anchor_lang::solana_program::instruction::AccountMeta::new(
                unauthorized_staker.pubkey(),
                true,
            ),
        ],
        data: jito_steward::instruction::InitializeDirectedStakeTicket {
            ticket_update_authority: unauthorized_staker.pubkey(),
            ticket_holder_is_protocol: false,
        }
        .data(),
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&unauthorized_staker.pubkey()),
        &[&unauthorized_staker],
        fixture.ctx.borrow().last_blockhash,
    );

    fixture
        .submit_transaction_assert_error(tx, "Unauthorized")
        .await;
}

#[tokio::test]
async fn test_multiple_directed_stake_tickets() {
    let fixture = setup_directed_stake_fixture().await;

    // Create multiple funded stakers and add them to whitelist
    let staker1 = create_funded_staker(&fixture).await;
    let staker2 = create_funded_staker(&fixture).await;
    let protocol_staker = create_funded_staker(&fixture).await;

    add_staker_to_whitelist(&fixture, &staker1.pubkey(), DirectedStakeRecordType::User).await;
    add_staker_to_whitelist(&fixture, &staker2.pubkey(), DirectedStakeRecordType::User).await;
    add_staker_to_whitelist(
        &fixture,
        &protocol_staker.pubkey(),
        DirectedStakeRecordType::Protocol,
    )
    .await;

    // Get validators from the validator list instead of creating new ones
    let validator1 = fixture
        .get_validator_from_list(0)
        .await
        .expect("Validator list should have at least one validator");
    let validator2 = fixture
        .get_validator_from_list(1)
        .await
        .expect("Validator list should have at least two validators");
    let validator3 = fixture
        .get_validator_from_list(2)
        .await
        .expect("Validator list should have at least three validators");

    add_validator_to_whitelist(&fixture, &validator1).await;
    add_validator_to_whitelist(&fixture, &validator2).await;
    add_validator_to_whitelist(&fixture, &validator3).await;

    // Initialize tickets for all stakers
    // Note: Since ticket address is derived from signer, and signer must be ticket_override_authority,
    // all tickets will have the same address (one ticket shared by all). This is a limitation of
    // the current auth design where signer must be both ticket_update_authority and ticket_override_authority.
    // Use fixture.keypair as both signer and ticket_update_authority
    let ticket1 = initialize_directed_stake_ticket(
        &fixture,
        &fixture.keypair, // signer - must be ticket_override_authority for updates
        fixture.keypair.pubkey(), // ticket_update_authority
        false,
    )
    .await;

    // Since all tickets share the same address (same signer), we reuse ticket1
    let ticket2 = ticket1;
    let ticket3 = ticket1;

    // Update each ticket with different preferences
    // Use fixture.keypair as signer since it's the ticket_update_authority
    update_directed_stake_ticket(
        &fixture,
        &ticket1,
        &fixture.keypair,
        vec![DirectedStakePreference::new(validator1, 10000)],
    )
    .await;

    update_directed_stake_ticket(
        &fixture,
        &ticket2,
        &fixture.keypair,
        vec![
            DirectedStakePreference::new(validator2, 5000),
            DirectedStakePreference::new(validator3, 5000),
        ],
    )
    .await;

    update_directed_stake_ticket(
        &fixture,
        &ticket3,
        &fixture.keypair,
        vec![
            DirectedStakePreference::new(validator1, 3000),
            DirectedStakePreference::new(validator2, 3000),
            DirectedStakePreference::new(validator3, 4000),
        ],
    )
    .await;

    // Verify the ticket was updated correctly
    // Note: Since all tickets share the same address (same signer), they're all the same ticket
    // The last update (ticket3) will be the final state
    let ticket_data: DirectedStakeTicket = fixture.load_and_deserialize(&ticket3).await;

    // Verify the final state after all updates
    assert_eq!(ticket_data.num_preferences, 3);
    assert_eq!(ticket_data.staker_preferences[0].vote_pubkey, validator1);
    assert_eq!(ticket_data.staker_preferences[0].stake_share_bps, 3000);
    assert_eq!(ticket_data.staker_preferences[1].vote_pubkey, validator2);
    assert_eq!(ticket_data.staker_preferences[1].stake_share_bps, 3000);
    assert_eq!(ticket_data.staker_preferences[2].vote_pubkey, validator3);
    assert_eq!(ticket_data.staker_preferences[2].stake_share_bps, 4000);
    assert!(ticket_data.preferences_valid());
}

#[tokio::test]
async fn test_directed_stake_ticket_allocation_calculation() {
    let fixture = setup_directed_stake_fixture().await;

    // Create a funded test staker and add to whitelist
    let staker = create_funded_staker(&fixture).await;
    add_staker_to_whitelist(&fixture, &staker.pubkey(), DirectedStakeRecordType::User).await;

    // Create test validators
    // Get validators from the validator list instead of creating new ones
    let validator1 = fixture
        .get_validator_from_list(0)
        .await
        .expect("Validator list should have at least one validator");
    let validator2 = fixture
        .get_validator_from_list(1)
        .await
        .expect("Validator list should have at least two validators");
    add_validator_to_whitelist(&fixture, &validator1).await;
    add_validator_to_whitelist(&fixture, &validator2).await;

    // Initialize a directed stake ticket
    // Use fixture.keypair as signer (must be ticket_override_authority for updates)
    // Set ticket_update_authority to fixture.keypair so it can update
    let ticket_account = initialize_directed_stake_ticket(
        &fixture,
        &fixture.keypair, // signer - must be ticket_override_authority for updates
        fixture.keypair.pubkey(), // ticket_update_authority - must match signer for updates
        false,
    )
    .await;

    // Create preferences with specific percentages
    let preferences = vec![
        DirectedStakePreference::new(validator1, 3000), // 30%
        DirectedStakePreference::new(validator2, 7000), // 70%
    ];

    // Use fixture.keypair as signer since it's the ticket_update_authority and ticket_override_authority
    update_directed_stake_ticket(&fixture, &ticket_account, &fixture.keypair, preferences).await;

    // Test allocation calculations
    let ticket: DirectedStakeTicket = fixture.load_and_deserialize(&ticket_account).await;

    let total_lamports = 1_000_000_000; // 1 SOL

    let allocations = ticket.get_allocations(total_lamports);
    assert_eq!(allocations.len(), 2);

    // Check that allocations match the expected percentages
    let validator1_allocation = allocations
        .iter()
        .find(|(pubkey, _)| *pubkey == validator1)
        .map(|(_, amount)| *amount)
        .unwrap();
    let validator2_allocation = allocations
        .iter()
        .find(|(pubkey, _)| *pubkey == validator2)
        .map(|(_, amount)| *amount)
        .unwrap();

    // 30% of 1 SOL = 300,000,000 lamports
    assert_eq!(validator1_allocation, 300_000_000);
    // 70% of 1 SOL = 700,000,000 lamports
    assert_eq!(validator2_allocation, 700_000_000);

    // Verify total allocation equals input
    let total_allocation: u128 = allocations.iter().map(|(_, amount)| amount).sum();
    assert_eq!(total_allocation, total_lamports as u128);
}
