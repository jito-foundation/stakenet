use anchor_lang::{InstructionData, ToAccountMetas};
use borsh::BorshSerialize;
use solana_program::sysvar::{clock::Clock, epoch_schedule::EpochSchedule};
use solana_program::vote::{
    self as solana_vote_program, instruction as vote_instruction,
    state::{VoteInit, VoteState},
};
use solana_program_test::*;
use solana_sdk::stake::{
    self, instruction as stake_instruction,
    state::{Authorized, Lockup, StakeStateV2},
};
use solana_sdk::{
    account::Account,
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signer::{keypair::Keypair, Signer},
    system_instruction, sysvar,
    transaction::Transaction,
};
use std::cell::RefCell;
use std::rc::Rc;
use tests::validator_history_fixtures::TestFixture;
use validator_history::state::{ValidatorHistory, ValidatorStakeBuffer};
use validator_history::ValidatorHistoryEntry;

// Helper function to create and set up a validator with a history account
#[allow(clippy::too_many_arguments, clippy::await_holding_refcell_ref)]
pub async fn create_and_setup_validator_accounts(
    ctx: &Rc<RefCell<ProgramTestContext>>,
    payer: &Keypair,
    index: u32,
    stake_amount: u64,
    is_superminority: bool,
) -> (Pubkey, Pubkey) {
    let vote_account = Keypair::new();
    let validator_history_address = Pubkey::find_program_address(
        &[ValidatorHistory::SEED, vote_account.pubkey().as_ref()],
        &validator_history::id(),
    )
    .0;

    let rent = ctx.borrow().banks_client.get_rent().await.unwrap();
    let vote_rent = rent.minimum_balance(VoteState::size_of());

    // Create and initialize vote account
    let vote_init = VoteInit {
        node_pubkey: payer.pubkey(),
        authorized_voter: payer.pubkey(),
        authorized_withdrawer: payer.pubkey(),
        commission: 0,
    };
    let mut instructions = vec![
        system_instruction::create_account(
            &payer.pubkey(),
            &vote_account.pubkey(),
            vote_rent,
            VoteState::size_of() as u64,
            &solana_vote_program::program::id(),
        ),
        Instruction::new_with_bincode(
            solana_vote_program::program::id(),
            &vote_instruction::VoteInstruction::InitializeAccount(vote_init),
            vec![
                AccountMeta::new(vote_account.pubkey(), false),
                AccountMeta::new_readonly(sysvar::rent::id(), false),
                AccountMeta::new_readonly(sysvar::clock::id(), false),
                AccountMeta::new_readonly(vote_init.node_pubkey, true),
            ],
        ),
    ];

    let tx = Transaction::new_signed_with_payer(
        &instructions,
        Some(&payer.pubkey()),
        &[payer, &vote_account],
        ctx.borrow().last_blockhash,
    );
    ctx.borrow_mut()
        .banks_client
        .process_transaction(tx)
        .await
        .unwrap();

    // Create and delegate stake account
    let stake_account = Keypair::new();
    let stake_rent = rent.minimum_balance(StakeStateV2::size_of());
    let lamports_to_delegate = stake_amount + stake_rent;

    let authorized = Authorized {
        staker: payer.pubkey(),
        withdrawer: payer.pubkey(),
    };
    let lockup = Lockup::default();
    instructions = vec![
        system_instruction::create_account(
            &payer.pubkey(),
            &stake_account.pubkey(),
            lamports_to_delegate,
            StakeStateV2::size_of() as u64,
            &stake::program::id(),
        ),
        stake_instruction::initialize(&stake_account.pubkey(), &authorized, &lockup),
        stake_instruction::delegate_stake(
            &stake_account.pubkey(),
            &payer.pubkey(),
            &vote_account.pubkey(),
        ),
    ];
    let tx = Transaction::new_signed_with_payer(
        &instructions,
        Some(&payer.pubkey()),
        &[payer, &stake_account],
        ctx.borrow().last_blockhash,
    );
    ctx.borrow_mut()
        .banks_client
        .process_transaction(tx)
        .await
        .unwrap();

    let mut validator_history = ValidatorHistory {
        struct_version: 0,
        vote_account: vote_account.pubkey(),
        index,
        bump: 0,
        _padding0: [0; 7],
        last_ip_timestamp: 0,
        last_version_timestamp: 0,
        _padding1: [0; 232],
        history: Default::default(),
    };
    validator_history.history.push(ValidatorHistoryEntry {
        activated_stake_lamports: stake_amount,
        is_superminority: is_superminority as u8,
        ..Default::default()
    });

    let mut validator_history_data = Vec::new();
    validator_history
        .serialize(&mut validator_history_data)
        .unwrap();

    ctx.borrow_mut().set_account(
        &validator_history_address,
        &Account {
            lamports: 1_000_000_000,
            data: validator_history_data,
            owner: validator_history::id(),
            ..Default::default()
        }
        .into(),
    );

    (vote_account.pubkey(), validator_history_address)
}

#[tokio::test(flavor = "current_thread")]
#[allow(clippy::too_many_arguments, clippy::await_holding_refcell_ref)]
async fn test_stake_buffer_insert() {
    // Starting test_stake_buffer_insert
    let test = TestFixture::new().await;

    // Initialize validator history config and stake buffer accounts
    test.initialize_config().await;
    test.submit_transaction_assert_success(
        test.build_initialize_and_realloc_validator_stake_buffer_account_transaction(),
    )
    .await;

    // Create several mock validator history accounts with different stake amounts
    let num_validators = 5;
    let mut validator_accounts = Vec::new();
    for i in 0..num_validators {
        // Simulate different stake amounts and ensure some are superminority
        let stake_amount = (100 - i) as u64 * 1_000_000_000; // Decreasing stake
        let is_superminority = i < 2; // First two are superminority for testing

        let (vote_account_address, validator_history_address) =
            create_and_setup_validator_accounts(
                &test.ctx,
                &test.keypair,
                i as u32,
                stake_amount,
                is_superminority,
            )
            .await;

        validator_accounts.push((vote_account_address, validator_history_address));
    }

    // Fake advancing by one epoch without spawning any banks
    // (use your genesis_config's slot timing, e.g. 100 ms/slot)
    test.advance_clock(1 /* epochs */, 500 /* ms per slot */)
        .await;
    // test.advance_num_epochs(2).await;

    for (vote_account_address, validator_history_address) in validator_accounts {
        // Call update_stake_buffer instruction for this specific validator
        let ix_data = validator_history::instruction::UpdateStakeBuffer {};

        let accounts = validator_history::accounts::UpdateStakeBuffer {
            config: test.validator_history_config,
            validator_stake_buffer_account: test.validator_stake_buffer_account,
            validator_history_account: validator_history_address,
        };

        let mut metas = accounts.to_account_metas(None);
        metas.push(AccountMeta::new_readonly(vote_account_address, false));

        let transaction = solana_sdk::transaction::Transaction::new_signed_with_payer(
            &[Instruction {
                program_id: validator_history::id(),
                accounts: metas,
                data: ix_data.data(),
            }],
            Some(&test.keypair.pubkey()),
            &[&test.keypair],
            test.ctx.borrow().last_blockhash,
        );

        test.submit_transaction_assert_success(transaction).await;
    }

    // Assert the state of the ValidatorStakeBuffer
    let stake_buffer_account: ValidatorStakeBuffer = test
        .load_and_deserialize(&test.validator_stake_buffer_account)
        .await;

    // Fetch current epoch after all transactions
    let current_epoch = test
        .ctx
        .borrow_mut()
        .banks_client
        .get_sysvar::<Clock>()
        .await
        .unwrap()
        .epoch;

    assert_eq!(stake_buffer_account.length(), num_validators as u32);
    assert_eq!(stake_buffer_account.last_observed_epoch(), current_epoch);
    assert!(stake_buffer_account.total_stake() > 0);

    // Verify individual entries are inserted and sorted by stake amount (descending)
    for i in 0..num_validators {
        let expected_stake = (100 - i) as u64 * 1_000_000_000;
        let entry = stake_buffer_account.get_by_index(i).unwrap();
        assert_eq!(entry.stake_amount, expected_stake);
    }
}

// #[tokio::test(flavor = "current_thread")]
// #[allow(clippy::too_many_arguments, clippy::await_holding_refcell_ref)]
// async fn test_stake_buffer_update_and_resort() {
//     let test = TestFixture::new().await;
//
//     test.initialize_config().await;
//     test.submit_transaction_assert_success(
//         test.build_initialize_and_realloc_validator_stake_buffer_account_transaction(),
//     )
//     .await;
//
//     let num_validators = 5;
//     let mut validator_accounts = Vec::new();
//
//     for i in 0..num_validators {
//         let stake_amount = (100 - i * 10) as u64 * 1_000_000_000;
//         let is_superminority = i < 2;
//
//         let (vote_account_address, validator_history_address) =
//             create_and_setup_validator_accounts(
//                 &test.ctx,
//                 &test.keypair,
//                 i as u32,
//                 stake_amount,
//                 is_superminority,
//             )
//             .await;
//
//         validator_accounts.push((vote_account_address, validator_history_address));
//     }
//
//     // Fake advancing by one epoch without spawning any banks
//     // (use your genesis_config's slot timing, e.g. 100 ms/slot)
//     test.advance_clock(1 /* epochs */, 100 /* ms per slot */)
//         .await;
//
//     for (vote_account_address, validator_history_address) in &validator_accounts {
//         let ix_data = validator_history::instruction::UpdateStakeBuffer {};
//         let accounts = validator_history::accounts::UpdateStakeBuffer {
//             config: test.validator_history_config,
//             validator_stake_buffer_account: test.validator_stake_buffer_account,
//             validator_history_account: *validator_history_address,
//         };
//         let mut metas = accounts.to_account_metas(None);
//         metas.push(AccountMeta::new_readonly(*vote_account_address, false));
//         let transaction = solana_sdk::transaction::Transaction::new_signed_with_payer(
//             &[Instruction {
//                 program_id: validator_history::id(),
//                 accounts: metas,
//                 data: ix_data.data(),
//             }],
//             Some(&test.keypair.pubkey()),
//             &[&test.keypair],
//             test.ctx.borrow().last_blockhash,
//         );
//         test.submit_transaction_assert_success(transaction).await;
//     }
//
//     // Update a validator's stake to change its rank
//     let validator_to_update_index = 3; // Initially has 70 stake
//     let new_stake_amount = 110 * 1_000_000_000; // New stake, should be rank 0
//
//     let (new_vote_account, new_validator_history) = create_and_setup_validator_accounts(
//         &test.ctx,
//         &test.keypair,
//         validator_to_update_index as u32,
//         new_stake_amount,
//         false,
//     )
//     .await;
//
//     // Fake advancing by one epoch without spawning any banks
//     // (use your genesis_config's slot timing, e.g. 100 ms/slot)
//     test.advance_clock(1 /* epochs */, 100 /* ms per slot */)
//         .await;
//
//     // Call update_stake_buffer for the updated validator
//     let ix_data = validator_history::instruction::UpdateStakeBuffer {};
//     let accounts = validator_history::accounts::UpdateStakeBuffer {
//         config: test.validator_history_config,
//         validator_stake_buffer_account: test.validator_stake_buffer_account,
//         validator_history_account: new_validator_history,
//     };
//     let mut metas = accounts.to_account_metas(None);
//     metas.push(AccountMeta::new_readonly(new_vote_account, false));
//     let transaction = solana_sdk::transaction::Transaction::new_signed_with_payer(
//         &[Instruction {
//             program_id: validator_history::id(),
//             accounts: metas,
//             data: ix_data.data(),
//         }],
//         Some(&test.keypair.pubkey()),
//         &[&test.keypair],
//         test.ctx.borrow().last_blockhash,
//     );
//     test.submit_transaction_assert_success(transaction).await;
//
//     // Assert the state of the ValidatorStakeBuffer after update
//     let stake_buffer_account: ValidatorStakeBuffer = test
//         .load_and_deserialize(&test.validator_stake_buffer_account)
//         .await;
//
//     // The number of validators is still the same, we just updated one.
//     assert_eq!(stake_buffer_account.length(), num_validators as u32);
//
//     // Verify the updated validator is now at rank 0
//     let top_entry = stake_buffer_account.get_by_index(0).unwrap();
//     assert_eq!(top_entry.stake_amount, new_stake_amount);
//     assert_eq!(top_entry.validator_id, validator_to_update_index as u32);
// }
