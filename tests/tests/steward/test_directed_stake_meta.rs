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
    set_directed_stake_whitelist_authority(&fixture).await;

    initialize_directed_stake_whitelist(&fixture).await;

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

/// Helper function to reallocate the directed stake whitelist to its proper size
async fn realloc_directed_stake_whitelist(fixture: &TestFixture) {
    let directed_stake_whitelist = Pubkey::find_program_address(
        &[
            DirectedStakeWhitelist::SEED,
            fixture.steward_config.pubkey().as_ref(),
        ],
        &jito_steward::id(),
    )
    .0;

    // Get the validator list address from the config
    let config: jito_steward::Config = fixture
        .load_and_deserialize(&fixture.steward_config.pubkey())
        .await;
    let validator_list = config.validator_list;

    // Calculate how many reallocations we need (similar to realloc_steward_state)
    let mut num_reallocs = (DirectedStakeWhitelist::SIZE - MAX_ALLOC_BYTES) / MAX_ALLOC_BYTES + 1;
    let mut ixs = vec![];

    while num_reallocs > 0 {
        ixs.extend(vec![
            Instruction {
                program_id: jito_steward::id(),
                accounts: vec![
                    anchor_lang::solana_program::instruction::AccountMeta::new(
                        directed_stake_whitelist,
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
                data: jito_steward::instruction::ReallocDirectedStakeWhitelist {}.data(),
            };
            num_reallocs.min(10)
        ]);
        num_reallocs = num_reallocs.saturating_sub(10);
    }

    // Submit all reallocation instructions in batches
    let tx = Transaction::new_signed_with_payer(
        &ixs,
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
    realloc_directed_stake_whitelist(fixture).await;
}

/// Helper function to reallocate directed stake meta to proper size
async fn realloc_directed_stake_meta(fixture: &TestFixture) {
    let directed_stake_meta = Pubkey::find_program_address(
        &[
            DirectedStakeMeta::SEED,
            fixture.steward_config.pubkey().as_ref(),
        ],
        &jito_steward::id(),
    )
    .0;

    // Get the validator list address from the config
    let config: jito_steward::Config = fixture
        .load_and_deserialize(&fixture.steward_config.pubkey())
        .await;
    let validator_list = config.validator_list;

    // Calculate how many reallocations we need
    let mut num_reallocs = (DirectedStakeMeta::SIZE - MAX_ALLOC_BYTES) / MAX_ALLOC_BYTES + 1;
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
        accounts: vec![
            anchor_lang::solana_program::instruction::AccountMeta::new_readonly(
                fixture.steward_config.pubkey(),
                false,
            ),
            anchor_lang::solana_program::instruction::AccountMeta::new(
                directed_stake_whitelist,
                false,
            ),
            anchor_lang::solana_program::instruction::AccountMeta::new(
                fixture.keypair.pubkey(),
                true,
            ),
        ],
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
        &[DirectedStakeTicket::SEED, signer.pubkey().as_ref()],
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
    realloc_directed_stake_meta(fixture).await;

    directed_stake_meta
}

#[tokio::test]
async fn test_initialize_directed_stake_whitelist() {
    let fixture = setup_directed_stake_fixture().await;

    // Verify the whitelist was created
    let whitelist_account = Pubkey::find_program_address(
        &[
            DirectedStakeWhitelist::SEED,
            fixture.steward_config.pubkey().as_ref(),
        ],
        &jito_steward::id(),
    )
    .0;

    let whitelist: DirectedStakeWhitelist = fixture.load_and_deserialize(&whitelist_account).await;

    assert_eq!(whitelist.total_permissioned_user_stakers, 0);
    assert_eq!(whitelist.total_permissioned_protocol_stakers, 0);
    assert_eq!(whitelist.total_permissioned_validators, 0);
}

#[tokio::test]
async fn test_add_stakers_to_whitelist() {
    let fixture = setup_directed_stake_fixture().await;

    // Create test stakers
    let user_staker = Keypair::new();
    let protocol_staker = Keypair::new();
    let validator = Pubkey::new_unique();

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

    assert_eq!(whitelist.total_permissioned_user_stakers, 1);
    assert_eq!(whitelist.total_permissioned_protocol_stakers, 1);
    assert_eq!(whitelist.total_permissioned_validators, 1);

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
    let ticket_account = initialize_directed_stake_ticket(
        &fixture,
        &staker,
        staker.pubkey(), // ticket_update_authority
        false,           // ticket_holder_is_protocol
    )
    .await;

    // Create some test validators and add them to whitelist
    let validator1 = Pubkey::new_unique();
    let validator2 = Pubkey::new_unique();
    add_validator_to_whitelist(&fixture, &validator1).await;
    add_validator_to_whitelist(&fixture, &validator2).await;

    // Create preferences
    let preferences = vec![
        DirectedStakePreference::new(validator1, 6000), // 60%
        DirectedStakePreference::new(validator2, 4000), // 40%
    ];

    // Update the ticket with preferences
    update_directed_stake_ticket(&fixture, &ticket_account, &staker, preferences).await;

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

    // Initialize a directed stake ticket
    let ticket_account =
        initialize_directed_stake_ticket(&fixture, &staker, staker.pubkey(), false).await;

    // Create test validators and add them to whitelist
    let validator1 = Pubkey::new_unique();
    let validator2 = Pubkey::new_unique();
    add_validator_to_whitelist(&fixture, &validator1).await;
    add_validator_to_whitelist(&fixture, &validator2).await;

    // Test valid preferences (total = 10000 bps)
    let valid_preferences = vec![
        DirectedStakePreference::new(validator1, 6000),
        DirectedStakePreference::new(validator2, 4000),
    ];

    update_directed_stake_ticket(&fixture, &ticket_account, &staker, valid_preferences).await;

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

    // Create test validators
    let validator1 = Pubkey::new_unique();
    let validator2 = Pubkey::new_unique();
    let validator3 = Pubkey::new_unique();

    add_validator_to_whitelist(&fixture, &validator1).await;
    add_validator_to_whitelist(&fixture, &validator2).await;
    add_validator_to_whitelist(&fixture, &validator3).await;

    // Initialize tickets for all stakers
    let ticket1 =
        initialize_directed_stake_ticket(&fixture, &staker1, staker1.pubkey(), false).await;

    let ticket2 =
        initialize_directed_stake_ticket(&fixture, &staker2, staker2.pubkey(), false).await;

    let ticket3 = initialize_directed_stake_ticket(
        &fixture,
        &protocol_staker,
        protocol_staker.pubkey(),
        true,
    )
    .await;

    // Update each ticket with different preferences
    update_directed_stake_ticket(
        &fixture,
        &ticket1,
        &staker1,
        vec![DirectedStakePreference::new(validator1, 10000)],
    )
    .await;

    update_directed_stake_ticket(
        &fixture,
        &ticket2,
        &staker2,
        vec![
            DirectedStakePreference::new(validator2, 5000),
            DirectedStakePreference::new(validator3, 5000),
        ],
    )
    .await;

    update_directed_stake_ticket(
        &fixture,
        &ticket3,
        &protocol_staker,
        vec![
            DirectedStakePreference::new(validator1, 3000),
            DirectedStakePreference::new(validator2, 3000),
            DirectedStakePreference::new(validator3, 4000),
        ],
    )
    .await;

    // Verify all tickets were created and updated correctly
    let ticket1_data: DirectedStakeTicket = fixture.load_and_deserialize(&ticket1).await;
    let ticket2_data: DirectedStakeTicket = fixture.load_and_deserialize(&ticket2).await;
    let ticket3_data: DirectedStakeTicket = fixture.load_and_deserialize(&ticket3).await;

    assert_eq!(ticket1_data.num_preferences, 1);
    assert_eq!(ticket1_data.staker_preferences[0].vote_pubkey, validator1);
    assert_eq!(ticket1_data.staker_preferences[0].stake_share_bps, 10000);

    assert_eq!(ticket2_data.num_preferences, 2);
    assert_eq!(ticket2_data.staker_preferences[0].vote_pubkey, validator2);
    assert_eq!(ticket2_data.staker_preferences[0].stake_share_bps, 5000);
    assert_eq!(ticket2_data.staker_preferences[1].vote_pubkey, validator3);
    assert_eq!(ticket2_data.staker_preferences[1].stake_share_bps, 5000);

    assert_eq!(ticket3_data.num_preferences, 3);
    assert!(bool::from(ticket3_data.ticket_holder_is_protocol));
    assert!(ticket3_data.preferences_valid());
}

#[tokio::test]
async fn test_directed_stake_ticket_allocation_calculation() {
    let fixture = setup_directed_stake_fixture().await;

    // Create a funded test staker and add to whitelist
    let staker = create_funded_staker(&fixture).await;
    add_staker_to_whitelist(&fixture, &staker.pubkey(), DirectedStakeRecordType::User).await;

    // Create test validators
    let validator1 = Pubkey::new_unique();
    let validator2 = Pubkey::new_unique();
    add_validator_to_whitelist(&fixture, &validator1).await;
    add_validator_to_whitelist(&fixture, &validator2).await;

    // Initialize a directed stake ticket
    let ticket_account =
        initialize_directed_stake_ticket(&fixture, &staker, staker.pubkey(), false).await;

    // Create preferences with specific percentages
    let preferences = vec![
        DirectedStakePreference::new(validator1, 3000), // 30%
        DirectedStakePreference::new(validator2, 7000), // 70%
    ];

    update_directed_stake_ticket(&fixture, &ticket_account, &staker, preferences).await;

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
