use anchor_lang::{InstructionData, ToAccountMetas};
use solana_program::sysvar::clock::Clock;
use solana_program_test::*;
use solana_sdk::stake::{
    self, instruction as stake_instruction,
    state::{Authorized, Lockup, StakeStateV2},
};
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signer::{keypair::Keypair, Signer},
    system_instruction,
    transaction::Transaction,
};
use std::cell::RefCell;
use std::rc::Rc;

use solana_sdk::hash::Hash;

use tests::validator_history_fixtures::TestFixture;
use validator_history::constants::MAX_ALLOC_BYTES;
use validator_history::state::{ValidatorHistory, ValidatorStakeBuffer};

#[allow(clippy::too_many_arguments, clippy::await_holding_refcell_ref)]
pub async fn create_validator_accounts(
    ctx: &Rc<RefCell<ProgramTestContext>>,
    payer: &Keypair,
    validator_history_config: &Pubkey,
    vote_account: &Pubkey,
    stake_amount: u64,
) -> Pubkey {
    let _ = create_stake_account(ctx, payer, vote_account, stake_amount).await;
    let validator_history_account =
        create_validator_history_account(ctx, payer, vote_account, validator_history_config).await;
    validator_history_account
}

#[allow(clippy::too_many_arguments, clippy::await_holding_refcell_ref)]
pub async fn create_validator_history_account(
    ctx: &Rc<RefCell<ProgramTestContext>>,
    payer: &Keypair,
    vote_account: &Pubkey,
    validator_history_config: &Pubkey,
) -> Pubkey {
    let validator_history_account = Pubkey::find_program_address(
        &[ValidatorHistory::SEED, vote_account.as_ref()],
        &validator_history::id(),
    )
    .0;
    let instruction = Instruction {
        program_id: validator_history::id(),
        accounts: validator_history::accounts::InitializeValidatorHistoryAccount {
            validator_history_account,
            vote_account: *vote_account,
            system_program: anchor_lang::solana_program::system_program::id(),
            signer: payer.pubkey(),
        }
        .to_account_metas(None),
        data: validator_history::instruction::InitializeValidatorHistoryAccount {}.data(),
    };
    let mut ixs = vec![instruction];
    let num_reallocs = (ValidatorHistory::SIZE - MAX_ALLOC_BYTES) / MAX_ALLOC_BYTES + 1;
    ixs.extend(vec![
        Instruction {
            program_id: validator_history::id(),
            accounts: validator_history::accounts::ReallocValidatorHistoryAccount {
                validator_history_account,
                vote_account: *vote_account,
                config: *validator_history_config,
                system_program: anchor_lang::solana_program::system_program::id(),
                signer: payer.pubkey(),
            }
            .to_account_metas(None),
            data: validator_history::instruction::ReallocValidatorHistoryAccount {}.data(),
        };
        num_reallocs
    ]);
    let latest_blockhash = fresh_blockhash(ctx).await;
    let tx =
        Transaction::new_signed_with_payer(&ixs, Some(&payer.pubkey()), &[payer], latest_blockhash);
    ctx.borrow_mut().last_blockhash = latest_blockhash;
    ctx.borrow_mut()
        .banks_client
        .process_transaction(tx)
        .await
        .unwrap();
    validator_history_account
}

#[allow(clippy::too_many_arguments, clippy::await_holding_refcell_ref)]
pub async fn create_stake_account(
    ctx: &Rc<RefCell<ProgramTestContext>>,
    payer: &Keypair,
    vote_account: &Pubkey,
    stake_amount: u64,
) -> Pubkey {
    let stake_account = Keypair::new();
    let rent = ctx.borrow().banks_client.get_rent().await.unwrap();
    let stake_rent = rent.minimum_balance(StakeStateV2::size_of());
    let lamports_to_delegate = stake_amount + stake_rent;
    let authorized = Authorized {
        staker: payer.pubkey(),
        withdrawer: payer.pubkey(),
    };
    let lockup = Lockup::default();
    let instructions = vec![
        system_instruction::create_account(
            &payer.pubkey(),
            &stake_account.pubkey(),
            lamports_to_delegate,
            StakeStateV2::size_of() as u64,
            &stake::program::id(),
        ),
        stake_instruction::initialize(&stake_account.pubkey(), &authorized, &lockup),
        stake_instruction::delegate_stake(&stake_account.pubkey(), &payer.pubkey(), vote_account),
    ];
    let latest_blockhash = fresh_blockhash(ctx).await;
    let tx = Transaction::new_signed_with_payer(
        &instructions,
        Some(&payer.pubkey()),
        &[payer, &stake_account],
        latest_blockhash,
    );
    ctx.borrow_mut().last_blockhash = latest_blockhash;
    ctx.borrow_mut()
        .banks_client
        .process_transaction(tx)
        .await
        .unwrap();
    stake_account.pubkey()
}

#[allow(clippy::await_holding_refcell_ref)]
async fn fresh_blockhash(ctx: &Rc<RefCell<ProgramTestContext>>) -> Hash {
    ctx.borrow()
        .banks_client
        .get_latest_blockhash()
        .await
        .unwrap()
}

// #[tokio::test(flavor = "current_thread")]
// #[allow(clippy::too_many_arguments, clippy::await_holding_refcell_ref)]
// async fn test_stake_buffer_insert() {
//     let test = TestFixture::new().await;
//
//     // Initialize validator history config and stake buffer accounts
//     test.initialize_config().await;
//     test.submit_transaction_assert_success(
//         test.build_initialize_and_realloc_validator_stake_buffer_account_transaction(),
//     )
//     .await;
//
//     // Create several mock validator history accounts with different stake amounts
//     let num_validators = 5;
//     let mut validator_accounts = Vec::new();
//     for (i, vote_account) in test
//         .additional_vote_accounts
//         .clone()
//         .into_iter()
//         .take(num_validators)
//         .enumerate()
//     {
//         // Simulate different stake amounts and ensure some are superminority
//         let stake_amount = (100 - i) as u64 * 100_000_000; // Decreasing stake
//
//         let validator_history_address = create_validator_accounts(
//             &test.ctx,
//             &test.keypair,
//             &test.validator_history_config,
//             &vote_account,
//             stake_amount,
//         )
//         .await;
//
//         validator_accounts.push((vote_account, validator_history_address));
//     }
//     // Advance epoch to finalize stake delegations
//     test.advance_num_epochs(1).await;
//
//     // Insert validators into stake buffer
//     for (vote_account_address, validator_history_address) in validator_accounts {
//         let ix_data = validator_history::instruction::UpdateStakeBuffer {};
//
//         let accounts = validator_history::accounts::UpdateStakeBuffer {
//             config: test.validator_history_config,
//             validator_stake_buffer_account: test.validator_stake_buffer_account,
//             validator_history_account: validator_history_address,
//         };
//
//         let mut metas = accounts.to_account_metas(None);
//         metas.push(AccountMeta::new_readonly(vote_account_address, false));
//
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
//
//         test.submit_transaction_assert_success(transaction).await;
//     }
//
//     // Assert the state of the ValidatorStakeBuffer
//     let stake_buffer_account: ValidatorStakeBuffer = test
//         .load_and_deserialize(&test.validator_stake_buffer_account)
//         .await;
//     let current_epoch = test
//         .ctx
//         .borrow_mut()
//         .banks_client
//         .get_sysvar::<Clock>()
//         .await
//         .unwrap()
//         .epoch;
//
//     assert_eq!(stake_buffer_account.length(), num_validators as u32);
//     assert_eq!(stake_buffer_account.last_observed_epoch(), current_epoch);
//     assert!(stake_buffer_account.total_stake() > 0);
//
//     // Verify individual entries are inserted and sorted by stake amount (descending)
//     for i in 0..num_validators {
//         let expected_stake = (100 - i) as u64 * 100_000_000;
//         let entry = stake_buffer_account.get_by_index(i).unwrap();
//         assert_eq!(entry.stake_amount, expected_stake);
//     }
// }

#[tokio::test(flavor = "current_thread")]
#[allow(clippy::too_many_arguments, clippy::await_holding_refcell_ref)]
async fn test_stake_buffer_insert_until_cu_limit_max() {
    let test = TestFixture::new().await;

    // Initialize validator history config and stake buffer accounts
    test.initialize_config().await;
    test.submit_transaction_assert_success(
        test.build_initialize_and_realloc_validator_stake_buffer_account_transaction(),
    )
    .await;

    // Create several mock validator history accounts
    let num_validators = test.additional_vote_accounts.len();
    let mut validator_accounts = Vec::new();
    for (_i, vote_account) in test.additional_vote_accounts.clone().iter().enumerate() {
        // Set linearly increasing stake amounts
        // such that we iterate the entire buffer onchain on every insert instruction, simulating
        // the worst cast scenario and guaranteeing that we have actually maxed out the buffer
        // size.
        let stake_amount = (10 * 100_000_000); // + i as u64;
        let validator_history_address = create_validator_accounts(
            &test.ctx,
            &test.keypair,
            &test.validator_history_config,
            vote_account,
            stake_amount,
        )
        .await;

        validator_accounts.push((*vote_account, validator_history_address));
    }
    // Advance epoch to finalize stake delegations
    test.advance_num_epochs(1).await;

    // Insert validators into stake buffer
    for (vote_account_address, validator_history_address) in validator_accounts {
        let ix_data = validator_history::instruction::UpdateStakeBuffer {};
        let accounts = validator_history::accounts::UpdateStakeBuffer {
            config: test.validator_history_config,
            validator_stake_buffer_account: test.validator_stake_buffer_account,
            validator_history_account: validator_history_address,
        };

        let mut metas = accounts.to_account_metas(None);
        metas.push(AccountMeta::new_readonly(vote_account_address, false));

        let latest_blockhash = fresh_blockhash(&test.ctx).await;
        let transaction = solana_sdk::transaction::Transaction::new_signed_with_payer(
            &[Instruction {
                program_id: validator_history::id(),
                accounts: metas,
                data: ix_data.data(),
            }],
            Some(&test.keypair.pubkey()),
            &[&test.keypair],
            latest_blockhash,
        );
        test.ctx.borrow_mut().last_blockhash = latest_blockhash;
        test.submit_transaction_assert_success(transaction).await;
    }

    // Assert the state of the ValidatorStakeBuffer
    let stake_buffer_account: ValidatorStakeBuffer = test
        .load_and_deserialize(&test.validator_stake_buffer_account)
        .await;
    let current_epoch = test
        .ctx
        .borrow_mut()
        .banks_client
        .get_sysvar::<Clock>()
        .await
        .unwrap()
        .epoch;

    let expected_total_stake = num_validators as u64 * 10 * 100_000_000;
    println!("Actual total_stake: {}", stake_buffer_account.total_stake());
    println!("Expected total_stake: {}", expected_total_stake);
    println!("buffer length: {}", stake_buffer_account.length());

    assert_eq!(stake_buffer_account.length(), num_validators as u32);
    assert_eq!(stake_buffer_account.last_observed_epoch(), current_epoch);
    assert!(stake_buffer_account.total_stake() == expected_total_stake);
    assert!(false);
}
