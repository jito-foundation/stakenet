use anchor_lang::{InstructionData, ToAccountMetas};
use solana_program::sysvar::clock::Clock;
use solana_program_test::*;
use solana_sdk::stake::{
    self, instruction as stake_instruction,
    state::{Authorized, Lockup, StakeStateV2},
};
use solana_sdk::{
    instruction::Instruction,
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
    hash: Hash,
) -> Pubkey {
    let _ = create_stake_account(ctx, payer, vote_account, stake_amount, hash).await;
    create_validator_history_account(ctx, payer, vote_account, validator_history_config, hash).await
}

#[allow(clippy::too_many_arguments, clippy::await_holding_refcell_ref)]
pub async fn create_validator_history_account(
    ctx: &Rc<RefCell<ProgramTestContext>>,
    payer: &Keypair,
    vote_account: &Pubkey,
    validator_history_config: &Pubkey,
    hash: Hash,
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
    let tx = Transaction::new_signed_with_payer(&ixs, Some(&payer.pubkey()), &[payer], hash);
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
    hash: Hash,
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
    let tx = Transaction::new_signed_with_payer(
        &instructions,
        Some(&payer.pubkey()),
        &[payer, &stake_account],
        hash,
    );
    ctx.borrow_mut()
        .banks_client
        .process_transaction(tx)
        .await
        .unwrap();
    stake_account.pubkey()
}

/// This test inserts monotonically decreasing stake amounts into the buffer, which is best case
/// scenario in terms of compute units consumed.
///
/// We have observed that CUs remain constant for every insertion regardless of buffer size or
/// length. Roughly 17_000 CUs.
#[tokio::test]
#[allow(clippy::too_many_arguments, clippy::await_holding_refcell_ref)]
async fn test_stake_buffer_insert_cu_limit_min() {
    let test = TestFixture::new().await;

    // Initialize validator history config and stake buffer accounts
    test.initialize_config().await;
    for tx in test
        .build_initialize_and_realloc_validator_stake_buffer_account_transaction()
        .into_iter()
    {
        let hash = test.fresh_blockhash().await;
        let tx = tx(hash);
        test.submit_transaction_assert_success(tx).await;
    }

    // Create several mock validator history accounts
    let num_validators = 10;
    let mut validator_accounts = Vec::new();
    for (i, vote_account) in test
        .additional_vote_accounts
        .clone()
        .iter()
        .enumerate()
        .take(num_validators)
    {
        // Set linearly decreasing stake amounts
        // such that we are always directly pushing to the end of the buffer,
        // simulating the optimal case.
        // Notice in the logs, that CUs per insert instruction are constant
        // as opposed to the lineary increasing test case where CUs are linearly increasing with every
        // sequential insert instruction.
        let stake_amount = (100 * 100_000_000) - i as u64;
        let hash = test.fresh_blockhash().await;
        let validator_history_address = create_validator_accounts(
            &test.ctx,
            &test.keypair,
            &test.validator_history_config,
            vote_account,
            stake_amount,
            hash,
        )
        .await;

        validator_accounts.push((*vote_account, validator_history_address));
    }
    // Advance epoch to finalize stake delegations
    test.advance_num_epochs(1).await;

    // Insert validators into stake buffer
    for (_vote_account_address, validator_history_address) in validator_accounts.iter() {
        let ix_data = validator_history::instruction::UpdateStakeBuffer {};
        let accounts = validator_history::accounts::UpdateStakeBuffer {
            config: test.validator_history_config,
            validator_stake_buffer_account: test.validator_stake_buffer_account,
            validator_history_account: *validator_history_address,
        };
        let metas = accounts.to_account_metas(None);
        let latest_blockhash = test.fresh_blockhash().await;
        let transaction = solana_sdk::transaction::Transaction::new_signed_with_payer(
            &[
                solana_sdk::compute_budget::ComputeBudgetInstruction::set_compute_unit_limit(
                    1_400_000,
                ),
                Instruction {
                    program_id: validator_history::id(),
                    accounts: metas,
                    data: ix_data.data(),
                },
            ],
            Some(&test.keypair.pubkey()),
            &[&test.keypair],
            latest_blockhash,
        );
        test.submit_transaction_assert_success(transaction).await;
    }

    // Deserialize buffer account
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

    // Assert total stake amount
    let base_stake_per_validator = 100 * 100_000_000;
    let sum_of_decrements = num_validators as u64 * (num_validators as u64 - 1) / 2; /* sum of arithmetic series */
    let expected_total_stake =
        num_validators as u64 * base_stake_per_validator - sum_of_decrements;
    assert_eq!(stake_buffer_account.length(), num_validators as u32);
    assert_eq!(stake_buffer_account.last_observed_epoch(), current_epoch);
    assert_eq!(stake_buffer_account.total_stake(), expected_total_stake);

    // Assert each entry
    for i in 0..stake_buffer_account.length() {
        let acc = stake_buffer_account.get(i as usize).unwrap();
        let expected = 100 * 100_000_000 - i as u64;
        println!("expected: {}", expected);
        println!("actual: {}", acc.stake_amount);
        assert!(acc.stake_amount == expected);
    }
}

/// This test was used to max out the size of the stake buffer by measuring the consumption of
/// compute units when inserting into the buffer with the buffer size set to 50_000 validators.
///
/// Because this test inserts validators with monotonically increasing stake amounts, it forces the
/// insert instruction into the worst case on invocation.
///
/// We observed linearly increasing CUs up to the 50_000 element, maxing out at just over 700_000 CUs.
/// This is about half of the max CUs that a single transaction is permitted to consume, which is
/// sweet spot between maintaining a huge buffer allowing for growth of the protocol while still
/// remaining well within the CU bounds.
///
/// This test is now nerfed down to 10 validators, as it still serves as a useful integration test.
/// Scaling up to 50_000 validators take about 10 minutes to run ... which is not practical for CI
/// pipelines.
#[tokio::test]
#[allow(clippy::too_many_arguments, clippy::await_holding_refcell_ref)]
async fn test_stake_buffer_insert_until_cu_limit_max() {
    let test = TestFixture::new().await;

    // Initialize validator history config and stake buffer accounts
    test.initialize_config().await;
    for tx in test
        .build_initialize_and_realloc_validator_stake_buffer_account_transaction()
        .into_iter()
    {
        let hash = test.fresh_blockhash().await;
        let tx = tx(hash);
        test.submit_transaction_assert_success(tx).await;
    }

    // Create several mock validator history accounts
    let num_validators = 10;
    let mut validator_accounts = Vec::new();
    for (i, vote_account) in test
        .additional_vote_accounts
        .clone()
        .iter()
        .enumerate()
        .take(num_validators)
    {
        // Set linearly increasing stake amounts
        // such that we iterate the entire buffer onchain on every insert instruction, simulating
        // the worst cast scenario and guaranteeing that we have actually maxed out the buffer
        // size.
        let stake_amount = (10 * 100_000_000) + i as u64;
        let hash = test.fresh_blockhash().await;
        let validator_history_address = create_validator_accounts(
            &test.ctx,
            &test.keypair,
            &test.validator_history_config,
            vote_account,
            stake_amount,
            hash,
        )
        .await;

        validator_accounts.push((*vote_account, validator_history_address));
    }
    // Advance epoch to finalize stake delegations
    test.advance_num_epochs(1).await;

    // Insert validators into stake buffer
    for (_vote_account_address, validator_history_address) in validator_accounts.iter() {
        let ix_data = validator_history::instruction::UpdateStakeBuffer {};
        let accounts = validator_history::accounts::UpdateStakeBuffer {
            config: test.validator_history_config,
            validator_stake_buffer_account: test.validator_stake_buffer_account,
            validator_history_account: *validator_history_address,
        };
        let metas = accounts.to_account_metas(None);
        let latest_blockhash = test.fresh_blockhash().await;
        let transaction = solana_sdk::transaction::Transaction::new_signed_with_payer(
            &[
                solana_sdk::compute_budget::ComputeBudgetInstruction::set_compute_unit_limit(
                    1_400_000,
                ),
                Instruction {
                    program_id: validator_history::id(),
                    accounts: metas,
                    data: ix_data.data(),
                },
            ],
            Some(&test.keypair.pubkey()),
            &[&test.keypair],
            latest_blockhash,
        );
        test.submit_transaction_assert_success(transaction).await;
    }

    // Deserialize buffer account
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

    // Assert total stake amount
    let base_stake_per_validator = 10 * 100_000_000;
    let sum_of_increments = num_validators as u64 * (num_validators as u64 - 1) / 2; /* sum of arithmetic series */
    let expected_total_stake =
        num_validators as u64 * base_stake_per_validator + sum_of_increments;
    assert_eq!(stake_buffer_account.length(), num_validators as u32);
    assert_eq!(stake_buffer_account.last_observed_epoch(), current_epoch);
    assert_eq!(stake_buffer_account.total_stake(), expected_total_stake);

    // Assert each entry
    for i in 0..stake_buffer_account.length() {
        let acc = stake_buffer_account.get(i as usize).unwrap();
        let expected = 10 * 100_000_000 + (10 - i as u64 - 1);
        assert!(acc.stake_amount == expected);
    }
}

#[tokio::test]
#[allow(clippy::too_many_arguments, clippy::await_holding_refcell_ref)]
async fn test_copy_stake_info() {
    let test = TestFixture::new().await;

    // Initialize validator history config and stake buffer accounts
    test.initialize_config().await;
    for tx in test
        .build_initialize_and_realloc_validator_stake_buffer_account_transaction()
        .into_iter()
    {
        let hash = test.fresh_blockhash().await;
        let tx = tx(hash);
        test.submit_transaction_assert_success(tx).await;
    }

    // Create several mock validator history accounts
    let num_validators = 10;
    let mut validator_accounts = Vec::new();
    for (i, vote_account) in test
        .additional_vote_accounts
        .clone()
        .iter()
        .enumerate()
        .take(num_validators)
    {
        // Set linearly increasing stake amounts
        // such that we iterate the entire buffer onchain on every insert instruction, simulating
        // the worst cast scenario and guaranteeing that we have actually maxed out the buffer
        // size.
        let stake_amount = (10 * 100_000_000) + i as u64;
        let hash = test.fresh_blockhash().await;
        let validator_history_address = create_validator_accounts(
            &test.ctx,
            &test.keypair,
            &test.validator_history_config,
            vote_account,
            stake_amount,
            hash,
        )
        .await;

        validator_accounts.push((*vote_account, validator_history_address));
    }
    // Advance epoch to finalize stake delegations
    test.advance_num_epochs(1).await;

    // Insert validators into stake buffer
    for (_vote_account_address, validator_history_address) in validator_accounts.iter() {
        let ix_data = validator_history::instruction::UpdateStakeBuffer {};
        let accounts = validator_history::accounts::UpdateStakeBuffer {
            config: test.validator_history_config,
            validator_stake_buffer_account: test.validator_stake_buffer_account,
            validator_history_account: *validator_history_address,
        };
        let metas = accounts.to_account_metas(None);
        let latest_blockhash = test.fresh_blockhash().await;
        let transaction = solana_sdk::transaction::Transaction::new_signed_with_payer(
            &[
                solana_sdk::compute_budget::ComputeBudgetInstruction::set_compute_unit_limit(
                    1_400_000,
                ),
                Instruction {
                    program_id: validator_history::id(),
                    accounts: metas,
                    data: ix_data.data(),
                },
            ],
            Some(&test.keypair.pubkey()),
            &[&test.keypair],
            latest_blockhash,
        );
        test.submit_transaction_assert_success(transaction).await;
    }

    // Deserialize buffer account
    let stake_buffer_account: ValidatorStakeBuffer = test
        .load_and_deserialize(&test.validator_stake_buffer_account)
        .await;

    // Copy stake info from buffer into validator history accounts
    for (_, validator_history_address) in validator_accounts.iter() {
        // Build copy stake info instruction
        let instruction = Instruction {
            program_id: validator_history::id(),
            accounts: validator_history::accounts::CopyStakeInfo {
                validator_history_account: *validator_history_address,
                config: test.validator_history_config,
                validator_stake_buffer_account: test.validator_stake_buffer_account,
            }
            .to_account_metas(None),
            data: validator_history::instruction::CopyStakeInfo {}.data(),
        };
        // Pack transaction
        let hash = test.fresh_blockhash().await;
        let transaction = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&test.keypair.pubkey()),
            &[&test.keypair],
            hash,
        );
        test.submit_transaction_assert_success(transaction).await;
        // Assert values
        let account: ValidatorHistory = test.load_and_deserialize(validator_history_address).await;
        assert!(account.history.idx == 0);
        assert!(account.history.arr[0].epoch == 1);
        let (stake, rank, is_superminority) = stake_buffer_account
            .get_by_validator_index(account.index)
            .unwrap();
        let is_superminority = match is_superminority {
            true => 1,
            false => 0,
        };
        assert!(account.history.arr[0].activated_stake_lamports == stake);
        assert!(account.history.arr[0].is_superminority == is_superminority);
        assert!(account.history.arr[0].rank == rank);
    }

    // Advance epoch and try copying stake infos and assert they fail with stale buffer
    test.advance_num_epochs(1).await;

    // Try copying stake info
    for (_, validator_history_address) in validator_accounts.iter() {
        // Build copy stake info instruction
        let instruction = Instruction {
            program_id: validator_history::id(),
            accounts: validator_history::accounts::CopyStakeInfo {
                validator_history_account: *validator_history_address,
                config: test.validator_history_config,
                validator_stake_buffer_account: test.validator_stake_buffer_account,
            }
            .to_account_metas(None),
            data: validator_history::instruction::CopyStakeInfo {}.data(),
        };
        // Pack transaction
        let hash = test.fresh_blockhash().await;
        let transaction = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&test.keypair.pubkey()),
            &[&test.keypair],
            hash,
        );
        test.submit_transaction_assert_error(transaction, "EpochOutOfRange")
            .await;
    }

    // Now insert into the buffer again with the new epoch and assert that only after the last
    // insert does copy stake info succeed (buffer needs to be finalized)
    for (i, (_vote_account_address, validator_history_address)) in
        validator_accounts.iter().enumerate()
    {
        // Insert into buffer
        let ix_data = validator_history::instruction::UpdateStakeBuffer {};
        let accounts = validator_history::accounts::UpdateStakeBuffer {
            config: test.validator_history_config,
            validator_stake_buffer_account: test.validator_stake_buffer_account,
            validator_history_account: *validator_history_address,
        };
        let metas = accounts.to_account_metas(None);
        let latest_blockhash = test.fresh_blockhash().await;
        let transaction = solana_sdk::transaction::Transaction::new_signed_with_payer(
            &[
                solana_sdk::compute_budget::ComputeBudgetInstruction::set_compute_unit_limit(
                    1_400_000,
                ),
                Instruction {
                    program_id: validator_history::id(),
                    accounts: metas,
                    data: ix_data.data(),
                },
            ],
            Some(&test.keypair.pubkey()),
            &[&test.keypair],
            latest_blockhash,
        );
        test.submit_transaction_assert_success(transaction).await;
        // Build copy stake info instruction
        let instruction = Instruction {
            program_id: validator_history::id(),
            accounts: validator_history::accounts::CopyStakeInfo {
                validator_history_account: *validator_history_address,
                config: test.validator_history_config,
                validator_stake_buffer_account: test.validator_stake_buffer_account,
            }
            .to_account_metas(None),
            data: validator_history::instruction::CopyStakeInfo {}.data(),
        };
        // Pack transaction
        let hash = test.fresh_blockhash().await;
        let transaction = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&test.keypair.pubkey()),
            &[&test.keypair],
            hash,
        );
        if i == num_validators - 1 {
            // Last validator should succeed because buffer is finalized
            test.submit_transaction_assert_success(transaction).await;
        } else {
            test.submit_transaction_assert_error(transaction, "StakeBufferNotFinalized")
                .await;
        }
    }
}
