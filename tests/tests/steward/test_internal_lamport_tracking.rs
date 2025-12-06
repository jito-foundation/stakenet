#![allow(clippy::await_holding_refcell_ref)]
use std::collections::HashMap;

#[allow(deprecated)]
use anchor_lang::{
    solana_program::{instruction::Instruction, pubkey::Pubkey, stake},
    InstructionData, ToAccountMetas,
};
use jito_steward::instructions::AuthorityType;
use jito_steward::state::directed_stake::DirectedStakeMeta;
use jito_steward::{
    stake_pool_utils::{StakePool, ValidatorList},
    StewardStateAccount, StewardStateAccountV2, UpdateParametersArgs,
};
use solana_program::sysvar;
use solana_program_test::*;
#[allow(deprecated)]
use solana_sdk::{
    signature::Keypair, signer::Signer, stake::state::StakeStateV2, transaction::Transaction,
};
use spl_associated_token_account::get_associated_token_address;
use spl_stake_pool::{find_withdraw_authority_program_address, minimum_delegation};
use tests::steward_fixtures::{
    auto_add_validator, crank_compute_delegations, crank_compute_instant_unstake,
    crank_compute_score, crank_copy_directed_stake_targets, crank_directed_stake_permissions,
    crank_epoch_maintenance, crank_idle, crank_rebalance, crank_rebalance_directed,
    crank_stake_pool, crank_validator_history_accounts_no_credits, ExtraValidatorAccounts,
    FixtureDefaultAccounts, StateMachineFixtures, TestFixture, ValidatorEntry,
};
use validator_history::ValidatorHistory;

async fn initialize_directed_stake_meta(fixture: &TestFixture) -> Pubkey {
    let directed_stake_meta = Pubkey::find_program_address(
        &[
            DirectedStakeMeta::SEED,
            fixture.steward_config.pubkey().as_ref(),
        ],
        &jito_steward::id(),
    )
    .0;

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
        &[set_whitelist_auth_ix, set_ticket_override_auth_ix, ix],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        fixture.ctx.borrow().last_blockhash,
    );

    fixture.submit_transaction_assert_success(tx).await;

    directed_stake_meta
}

/// Helper function to execute a spl_stake_pool::instruction::WithdrawStake instruction
async fn stake_deposit(
    fixture: &TestFixture,
    stake_account_address: Pubkey,
    reserve_stake_address: Pubkey,
    withdraw_authority: Pubkey,
    pool_mint: Pubkey,
    manager_fee_account: Pubkey,
    amount: u64,
) {
    // Create a user-owned stake account for receiving the withdrawn stake
    let user_stake_keypair = Keypair::new();
    let stake_rent = fixture.fetch_stake_rent().await;

    // Create the stake account with the system program then initialize with stake program
    let create_stake_account_ix = solana_sdk::system_instruction::create_account(
        &fixture.keypair.pubkey(),
        &user_stake_keypair.pubkey(),
        stake_rent,
        StakeStateV2::size_of() as u64,
        &stake::program::id(),
    );
    let initialize_stake_account_ix = stake::instruction::initialize(
        &user_stake_keypair.pubkey(),
        &stake::state::Authorized {
            staker: fixture.keypair.pubkey(),
            withdrawer: fixture.keypair.pubkey(),
        },
        &stake::state::Lockup::default(),
    );
    let transfer_ix = solana_sdk::system_instruction::transfer(
        &fixture.keypair.pubkey(),
        &user_stake_keypair.pubkey(),
        amount,
    );
    let deposit_ixns = spl_stake_pool::instruction::deposit_stake(
        &spl_stake_pool::id(),
        &fixture.stake_pool_meta.stake_pool,
        &fixture.stake_pool_meta.validator_list,
        &withdraw_authority,
        &user_stake_keypair.pubkey(),
        &fixture.keypair.pubkey(), // User-owned stake account to receive the withdrawn stake
        &stake_account_address,
        &reserve_stake_address,
        &spl_associated_token_account::get_associated_token_address(
            &fixture.keypair.pubkey(),
            &pool_mint,
        ),
        &manager_fee_account,
        &reserve_stake_address,
        &pool_mint,
        &spl_token::id(),
    );

    let mut ixns = vec![
        create_stake_account_ix,
        initialize_stake_account_ix,
        transfer_ix,
    ];
    ixns.extend(deposit_ixns);

    let tx = Transaction::new_signed_with_payer(
        &ixns,
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair, &user_stake_keypair],
        fixture.ctx.borrow().last_blockhash,
    );
    fixture.submit_transaction_assert_success(tx).await;
}

/// Helper function to execute a spl_stake_pool::instruction::WithdrawStake instruction
async fn stake_withdraw(
    fixture: &TestFixture,
    stake_account_address: Pubkey,
    withdraw_authority: Pubkey,
    pool_mint: Pubkey,
    manager_fee_account: Pubkey,
    amount: u64,
) {
    // Create a user-owned stake account for receiving the withdrawn stake
    let user_stake_keypair = Keypair::new();
    let stake_rent = fixture.fetch_stake_rent().await;

    // Create the stake account with the system program then initialize with stake program
    let create_stake_account_ix = solana_sdk::system_instruction::create_account(
        &fixture.keypair.pubkey(),
        &user_stake_keypair.pubkey(),
        stake_rent,
        StakeStateV2::size_of() as u64,
        &stake::program::id(),
    );

    let withdraw_ix = spl_stake_pool::instruction::withdraw_stake(
        &spl_stake_pool::id(),
        &fixture.stake_pool_meta.stake_pool,
        &fixture.stake_pool_meta.validator_list,
        &withdraw_authority,
        &stake_account_address,
        &user_stake_keypair.pubkey(), // User-owned stake account to receive the withdrawn stake
        &fixture.keypair.pubkey(),
        &fixture.keypair.pubkey(),
        &spl_associated_token_account::get_associated_token_address(
            &fixture.keypair.pubkey(),
            &pool_mint,
        ),
        &manager_fee_account,
        &pool_mint,
        &spl_token::id(),
        amount,
    );

    let tx = Transaction::new_signed_with_payer(
        &[create_stake_account_ix, withdraw_ix],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair, &user_stake_keypair],
        fixture.ctx.borrow().last_blockhash,
    );

    fixture.submit_transaction_assert_success(tx).await;
}

/// Helper function to deposit SOL directly into the stake pool reserve
async fn sol_deposit(fixture: &TestFixture, amount: u64) {
    let stake_pool: StakePool = fixture
        .load_and_deserialize(&fixture.stake_pool_meta.stake_pool)
        .await;
    let (withdraw_authority, _) = find_withdraw_authority_program_address(
        &spl_stake_pool::id(),
        &fixture.stake_pool_meta.stake_pool,
    );
    let user_ata = get_associated_token_address(&fixture.keypair.pubkey(), &stake_pool.pool_mint);
    let deposit_sol_ix = spl_stake_pool::instruction::deposit_sol(
        &spl_stake_pool::id(),
        &fixture.stake_pool_meta.stake_pool,
        &withdraw_authority,
        &stake_pool.reserve_stake,
        &fixture.keypair.pubkey(), // depositor
        &user_ata,
        &stake_pool.manager_fee_account,
        &user_ata,
        &stake_pool.pool_mint,
        &spl_token::id(),
        amount,
    );

    let tx = Transaction::new_signed_with_payer(
        &[deposit_sol_ix],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        fixture
            .ctx
            .borrow_mut()
            .get_new_latest_blockhash()
            .await
            .unwrap(),
    );
    fixture.submit_transaction_assert_success(tx).await;
}

/// Helper function to withdraw SOL directly from the stake pool reserve
async fn sol_withdraw(fixture: &TestFixture, amount: u64) {
    let stake_pool: StakePool = fixture
        .load_and_deserialize(&fixture.stake_pool_meta.stake_pool)
        .await;
    let (withdraw_authority, _) = find_withdraw_authority_program_address(
        &spl_stake_pool::id(),
        &fixture.stake_pool_meta.stake_pool,
    );
    let user_ata = get_associated_token_address(&fixture.keypair.pubkey(), &stake_pool.pool_mint);
    let withdraw_sol_ix = spl_stake_pool::instruction::withdraw_sol(
        &spl_stake_pool::id(),
        &fixture.stake_pool_meta.stake_pool,
        &withdraw_authority,
        &fixture.keypair.pubkey(), // recipient
        &user_ata,
        &stake_pool.reserve_stake,
        &fixture.keypair.pubkey(),
        &stake_pool.manager_fee_account,
        &stake_pool.pool_mint,
        &spl_token::id(),
        amount,
    );

    let tx = Transaction::new_signed_with_payer(
        &[withdraw_sol_ix],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        fixture
            .ctx
            .borrow_mut()
            .get_new_latest_blockhash()
            .await
            .unwrap(),
    );
    fixture.submit_transaction_assert_success(tx).await;
}

#[tokio::test]
async fn test_internal_lamport_tracking_basic() {
    let mut fixture_accounts = FixtureDefaultAccounts::default();

    let unit_test_fixtures = StateMachineFixtures::default();

    // Note that these parameters are overriden in initialize_steward, just included here for completeness
    fixture_accounts.steward_config.parameters = unit_test_fixtures.config.parameters;

    fixture_accounts.validators = (0..3)
        .map(|i| ValidatorEntry {
            validator_history: unit_test_fixtures.validators[i],
            vote_account: unit_test_fixtures.vote_accounts[i].clone(),
            vote_address: unit_test_fixtures.validators[i].vote_account,
        })
        .collect();
    fixture_accounts.cluster_history = unit_test_fixtures.cluster_history;

    let mut fixture = TestFixture::new_from_accounts(fixture_accounts, HashMap::new()).await;

    fixture.steward_config = Keypair::new();
    fixture.steward_state = Pubkey::find_program_address(
        &[
            StewardStateAccount::SEED,
            fixture.steward_config.pubkey().as_ref(),
        ],
        &jito_steward::id(),
    )
    .0;

    fixture.advance_num_epochs(20, 10).await;
    fixture.initialize_stake_pool().await;
    fixture
        .initialize_steward(
            Some(UpdateParametersArgs {
                mev_commission_range: Some(10),
                epoch_credits_range: Some(20),
                commission_range: Some(20),
                scoring_delinquency_threshold_ratio: Some(0.85),
                instant_unstake_delinquency_threshold_ratio: Some(0.70),
                mev_commission_bps_threshold: Some(1000),
                commission_threshold: Some(5),
                historical_commission_threshold: Some(50),
                num_delegation_validators: Some(200),
                scoring_unstake_cap_bps: Some(750),
                instant_unstake_cap_bps: Some(10),
                stake_deposit_unstake_cap_bps: Some(10),
                instant_unstake_epoch_progress: Some(0.9),
                compute_score_slot_range: Some(1000),
                instant_unstake_inputs_epoch_progress: Some(0.50),
                num_epochs_between_scoring: Some(50),
                minimum_stake_lamports: Some(5_000_000_000),
                minimum_voting_epochs: Some(0),
                compute_score_epoch_progress: Some(0.50),
                undirected_stake_floor_lamports: Some(0),
                directed_stake_unstake_cap_bps: Some(10_000),
            }),
            None,
        )
        .await;

    let _steward: Box<StewardStateAccountV2> =
        Box::new(fixture.load_and_deserialize(&fixture.steward_state).await);

    fixture.directed_stake_meta = initialize_directed_stake_meta(&fixture).await;
    fixture.realloc_directed_stake_meta().await;

    // Set first validator directed stake lamports to target
    let account_info = fixture.get_account(&fixture.directed_stake_meta).await;
    let mut directed_stake_meta: DirectedStakeMeta = fixture
        .load_and_deserialize(&fixture.directed_stake_meta)
        .await;
    directed_stake_meta.directed_stake_lamports[0] = 1_000_000_000;

    // Serialize with discriminator using bytemuck (zero-copy account)
    // Get discriminator from existing account (first 8 bytes)
    let discriminator = &account_info.data[..8];
    let mut account_data = Vec::new();
    account_data.extend_from_slice(discriminator);
    account_data.extend_from_slice(bytemuck::bytes_of(&directed_stake_meta));

    let account = solana_sdk::account::Account {
        lamports: account_info.lamports,
        data: account_data,
        owner: account_info.owner,
        executable: account_info.executable,
        rent_epoch: account_info.rent_epoch,
    };

    fixture
        .ctx
        .borrow_mut()
        .set_account(&fixture.directed_stake_meta, &account.into());

    let mut extra_validator_accounts = vec![];
    for i in 0..unit_test_fixtures.validators.len() {
        let vote_account = unit_test_fixtures.validator_list[i].vote_account_address;
        let (validator_history_address, _) = Pubkey::find_program_address(
            &[ValidatorHistory::SEED, vote_account.as_ref()],
            &validator_history::id(),
        );

        let (stake_account_address, transient_stake_account_address, withdraw_authority) =
            fixture.stake_accounts_for_validator(vote_account).await;

        extra_validator_accounts.push(ExtraValidatorAccounts {
            vote_account,
            validator_history_address,
            stake_account_address,
            transient_stake_account_address,
            withdraw_authority,
        })
    }

    crank_epoch_maintenance(&fixture, None).await;

    // Auto add validator - adds to validator list
    for extra_accounts in extra_validator_accounts.iter() {
        auto_add_validator(&fixture, extra_accounts).await;
    }

    // Set up directed stake permissions (whitelist authority, add validators and staker to whitelist)
    crank_directed_stake_permissions(&fixture, &extra_validator_accounts).await;

    // Set the directed stake meta upload authority to the signer
    let set_meta_auth_ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::SetNewAuthority {
            config: fixture.steward_config.pubkey(),
            new_authority: fixture.keypair.pubkey(),
            admin: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::SetNewAuthority {
            authority_type:
                jito_steward::instructions::AuthorityType::SetDirectedStakeMetaUploadAuthority,
        }
        .data(),
    };

    let tx = Transaction::new_signed_with_payer(
        &[set_meta_auth_ix],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        fixture
            .ctx
            .borrow_mut()
            .get_new_latest_blockhash()
            .await
            .unwrap(),
    );

    fixture.submit_transaction_assert_success(tx).await;

    // Copy directed stake targets for the validators
    for extra_accounts in extra_validator_accounts.iter() {
        crank_copy_directed_stake_targets(&fixture, extra_accounts.vote_account, 1_000_000_000)
            .await;
    }

    // Perform a single rebalance directed
    crank_rebalance_directed(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1, 2],
    )
    .await;

    let stake_rent = fixture.fetch_stake_rent().await;
    let stake_program_minimum = fixture.fetch_minimum_delegation().await;
    let pool_minimum_delegation = minimum_delegation(stake_program_minimum);

    // Perform state checks on validator_lamport_balance and directed_stake_lamports
    let steward_state: StewardStateAccountV2 =
        fixture.load_and_deserialize(&fixture.steward_state).await;
    let directed_stake_meta: DirectedStakeMeta = fixture
        .load_and_deserialize(&fixture.directed_stake_meta)
        .await;
    let validator_list: ValidatorList = fixture
        .load_and_deserialize(&fixture.stake_pool_meta.validator_list)
        .await;
    {
        // Check accounting for each validator
        for (validator_list_index, extra_accounts) in extra_validator_accounts.iter().enumerate() {
            // Find the directed_stake_meta_index for this validator
            let Some(directed_stake_meta_index) =
                directed_stake_meta.get_target_index(&extra_accounts.vote_account)
            else {
                continue;
            };

            // Get the actual stake from the validator list
            let validator_stake_info = &validator_list.validators[validator_list_index];
            let total_stake_lamports = u64::from(validator_stake_info.active_stake_lamports)
                .saturating_sub(stake_rent)
                .saturating_sub(pool_minimum_delegation);

            // Get the tracked values from state
            let validator_lamport_balance = steward_state.state.validator_lamport_balances
                [validator_list_index]
                .saturating_sub(stake_rent)
                .saturating_sub(pool_minimum_delegation);
            let directed_stake_lamports =
                directed_stake_meta.directed_stake_lamports[validator_list_index];

            let target_directed_stake =
                directed_stake_meta.targets[directed_stake_meta_index].total_staked_lamports;
            let undirected_stake = validator_lamport_balance
                .saturating_sub(directed_stake_lamports)
                .saturating_sub(stake_rent)
                .saturating_sub(pool_minimum_delegation);

            // Directed stake lamports and target directed stake should always be equal
            assert!(directed_stake_lamports == target_directed_stake);
            // Undirected stake should be equal to the total stake lamports minus the directed stake lamports
            assert!(
                undirected_stake == total_stake_lamports.saturating_sub(directed_stake_lamports)
            );
            // Since we have not delegated any undirected stake, directed stake lamports should equal the validator lamport balance
            assert!(directed_stake_lamports == validator_lamport_balance);
        }
    }
    drop(fixture);
}

#[tokio::test]
async fn test_internal_lamport_tracking_with_withdraw() {
    let mut fixture_accounts = FixtureDefaultAccounts::default();

    let unit_test_fixtures = StateMachineFixtures::default();

    // Note that these parameters are overriden in initialize_steward, just included here for completeness
    fixture_accounts.steward_config.parameters = unit_test_fixtures.config.parameters;

    fixture_accounts.validators = (0..3)
        .map(|i| ValidatorEntry {
            validator_history: unit_test_fixtures.validators[i],
            vote_account: unit_test_fixtures.vote_accounts[i].clone(),
            vote_address: unit_test_fixtures.validators[i].vote_account,
        })
        .collect();
    fixture_accounts.cluster_history = unit_test_fixtures.cluster_history;

    let mut fixture = TestFixture::new_from_accounts(fixture_accounts, HashMap::new()).await;

    fixture.steward_config = Keypair::new();
    fixture.steward_state = Pubkey::find_program_address(
        &[
            StewardStateAccount::SEED,
            fixture.steward_config.pubkey().as_ref(),
        ],
        &jito_steward::id(),
    )
    .0;

    fixture.advance_num_epochs(20, 10).await;
    fixture.initialize_stake_pool().await;
    fixture
        .initialize_steward(
            Some(UpdateParametersArgs {
                mev_commission_range: Some(10),
                epoch_credits_range: Some(20),
                commission_range: Some(20),
                scoring_delinquency_threshold_ratio: Some(0.85),
                instant_unstake_delinquency_threshold_ratio: Some(0.70),
                mev_commission_bps_threshold: Some(1000),
                commission_threshold: Some(5),
                historical_commission_threshold: Some(50),
                num_delegation_validators: Some(200),
                scoring_unstake_cap_bps: Some(750),
                instant_unstake_cap_bps: Some(10),
                stake_deposit_unstake_cap_bps: Some(10),
                instant_unstake_epoch_progress: Some(0.9),
                compute_score_slot_range: Some(1000),
                instant_unstake_inputs_epoch_progress: Some(0.50),
                num_epochs_between_scoring: Some(50),
                minimum_stake_lamports: Some(1000),
                minimum_voting_epochs: Some(0),
                compute_score_epoch_progress: Some(0.50),
                undirected_stake_floor_lamports: Some(0),
                directed_stake_unstake_cap_bps: Some(10_000),
            }),
            None,
        )
        .await;

    let _steward: Box<StewardStateAccountV2> =
        Box::new(fixture.load_and_deserialize(&fixture.steward_state).await);

    fixture.directed_stake_meta = initialize_directed_stake_meta(&fixture).await;
    fixture.realloc_directed_stake_meta().await;

    let mut extra_validator_accounts = vec![];
    for i in 0..unit_test_fixtures.validators.len() {
        let vote_account = unit_test_fixtures.validator_list[i].vote_account_address;
        let (validator_history_address, _) = Pubkey::find_program_address(
            &[ValidatorHistory::SEED, vote_account.as_ref()],
            &validator_history::id(),
        );

        let (stake_account_address, transient_stake_account_address, withdraw_authority) =
            fixture.stake_accounts_for_validator(vote_account).await;

        extra_validator_accounts.push(ExtraValidatorAccounts {
            vote_account,
            validator_history_address,
            stake_account_address,
            transient_stake_account_address,
            withdraw_authority,
        })
    }

    crank_epoch_maintenance(&fixture, None).await;

    for extra_accounts in extra_validator_accounts.iter() {
        auto_add_validator(&fixture, extra_accounts).await;
    }

    crank_directed_stake_permissions(&fixture, &extra_validator_accounts).await;

    let set_meta_auth_ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::SetNewAuthority {
            config: fixture.steward_config.pubkey(),
            new_authority: fixture.keypair.pubkey(),
            admin: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::SetNewAuthority {
            authority_type:
                jito_steward::instructions::AuthorityType::SetDirectedStakeMetaUploadAuthority,
        }
        .data(),
    };

    let tx = Transaction::new_signed_with_payer(
        &[set_meta_auth_ix],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        fixture
            .ctx
            .borrow_mut()
            .get_new_latest_blockhash()
            .await
            .unwrap(),
    );

    fixture.submit_transaction_assert_success(tx).await;

    // Copy directed stake targets for the validators
    for extra_accounts in extra_validator_accounts.iter() {
        crank_copy_directed_stake_targets(&fixture, extra_accounts.vote_account, 16_000_000_000)
            .await;
    }

    // Perform a single rebalance directed
    crank_rebalance_directed(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1, 2],
    )
    .await;

    let stake_rent = fixture.fetch_stake_rent().await;
    let stake_program_minimum = fixture.fetch_minimum_delegation().await;
    let pool_minimum_delegation = minimum_delegation(stake_program_minimum);

    fixture.advance_num_epochs(1, 10).await;
    crank_stake_pool(&fixture).await;
    crank_epoch_maintenance(&fixture, None).await;

    fixture.advance_num_epochs(1, 10).await;
    crank_stake_pool(&fixture).await;
    crank_epoch_maintenance(&fixture, None).await;

    let stake_pool: StakePool = fixture
        .load_and_deserialize(&fixture.stake_pool_meta.stake_pool)
        .await;

    let stake_account = fixture
        .get_account(&extra_validator_accounts[0].stake_account_address)
        .await;
    let stake_account_lamport_balance = stake_account.lamports;
    let min_remaining = stake_rent + pool_minimum_delegation; // From your logs; in code, use stake::get_rent_exempt() + MINIMUM_ACTIVE_STAKE
    let max_withdraw_lamports = stake_account_lamport_balance.saturating_sub(min_remaining);
    let token_withdraw_amount = stake_pool.calc_pool_tokens_for_deposit(max_withdraw_lamports);
    let withdraw_amount = stake_pool.calc_lamports_withdraw_amount(token_withdraw_amount.unwrap());

    // Empty the reserve: withdraw reserve balance minus (stake_rent + minimum_delegation)
    // This gives us clean numbers for our assertions
    {
        let reserve_stake_account = fixture.get_account(&stake_pool.reserve_stake).await;
        let reserve_balance = reserve_stake_account.lamports;
        let withdrawal_amount = reserve_balance
            .saturating_sub(stake_rent)
            .saturating_sub(pool_minimum_delegation);

        if withdrawal_amount > 0 {
            sol_withdraw(&fixture, withdrawal_amount).await;
        }
    }

    stake_withdraw(
        &fixture,
        extra_validator_accounts[0].stake_account_address,
        extra_validator_accounts[0].withdraw_authority,
        stake_pool.pool_mint,
        stake_pool.manager_fee_account,
        withdraw_amount.unwrap(),
    )
    .await;

    crank_rebalance_directed(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1, 2],
    )
    .await;

    fixture.advance_num_epochs(1, 10).await;
    crank_stake_pool(&fixture).await;
    crank_epoch_maintenance(&fixture, None).await;

    // Withdraw should count against the directed stake applied and reflect in the validator list lamport balance
    {
        let directed_stake_meta: DirectedStakeMeta = fixture
            .load_and_deserialize(&fixture.directed_stake_meta)
            .await;
        let directed_stake_applied_lamports = directed_stake_meta.targets[0].total_staked_lamports;
        let steward_state: StewardStateAccountV2 =
            fixture.load_and_deserialize(&fixture.steward_state).await;
        let validator_lamport_balance = steward_state.state.validator_lamport_balances[0];
        let directed_stake_lamports = directed_stake_meta.directed_stake_lamports[0];
        let validator_list: ValidatorList = fixture
            .load_and_deserialize(&fixture.stake_pool_meta.validator_list)
            .await;
        let validator_list_lamport_balance = validator_list.validators[0].active_stake_lamports;
        assert!(validator_lamport_balance == (stake_rent + pool_minimum_delegation));
        assert!(validator_lamport_balance == u64::from_le_bytes(validator_list_lamport_balance.0));
        assert!(directed_stake_lamports == 0);
        assert!(directed_stake_applied_lamports == 0);
    }

    drop(fixture);
}

#[tokio::test]
async fn test_internal_lamport_tracking_with_withdraw_remainder() {
    let mut fixture_accounts = FixtureDefaultAccounts::default();

    let unit_test_fixtures = StateMachineFixtures::default();

    // Note that these parameters are overriden in initialize_steward, just included here for completeness
    fixture_accounts.steward_config.parameters = unit_test_fixtures.config.parameters;

    fixture_accounts.validators = (0..3)
        .map(|i| ValidatorEntry {
            validator_history: unit_test_fixtures.validators[i],
            vote_account: unit_test_fixtures.vote_accounts[i].clone(),
            vote_address: unit_test_fixtures.validators[i].vote_account,
        })
        .collect();
    fixture_accounts.cluster_history = unit_test_fixtures.cluster_history;

    let mut fixture = TestFixture::new_from_accounts(fixture_accounts, HashMap::new()).await;

    fixture.steward_config = Keypair::new();
    fixture.steward_state = Pubkey::find_program_address(
        &[
            StewardStateAccount::SEED,
            fixture.steward_config.pubkey().as_ref(),
        ],
        &jito_steward::id(),
    )
    .0;

    fixture.advance_num_epochs(20, 10).await;
    fixture.initialize_stake_pool().await;
    fixture
        .initialize_steward(
            Some(UpdateParametersArgs {
                mev_commission_range: Some(10),
                epoch_credits_range: Some(20),
                commission_range: Some(20),
                scoring_delinquency_threshold_ratio: Some(0.0),
                instant_unstake_delinquency_threshold_ratio: Some(0.70),
                mev_commission_bps_threshold: Some(1000),
                commission_threshold: Some(5),
                historical_commission_threshold: Some(50),
                num_delegation_validators: Some(200),
                scoring_unstake_cap_bps: Some(750),
                instant_unstake_cap_bps: Some(0),
                stake_deposit_unstake_cap_bps: Some(0),
                instant_unstake_epoch_progress: Some(0.9),
                compute_score_slot_range: Some(1000),
                instant_unstake_inputs_epoch_progress: Some(0.50),
                num_epochs_between_scoring: Some(1),
                minimum_stake_lamports: Some(1),
                minimum_voting_epochs: Some(0),
                compute_score_epoch_progress: Some(0.50),
                undirected_stake_floor_lamports: Some(0),
                directed_stake_unstake_cap_bps: Some(10_000),
            }),
            None,
        )
        .await;

    {
        let mut _steward: StewardStateAccountV2 =
            fixture.load_and_deserialize(&fixture.steward_state).await;
    }

    fixture.directed_stake_meta = initialize_directed_stake_meta(&fixture).await;
    fixture.realloc_directed_stake_meta().await;

    let mut extra_validator_accounts = vec![];
    for i in 0..unit_test_fixtures.validators.len() {
        let vote_account = unit_test_fixtures.validator_list[i].vote_account_address;
        let (validator_history_address, _) = Pubkey::find_program_address(
            &[ValidatorHistory::SEED, vote_account.as_ref()],
            &validator_history::id(),
        );

        let (stake_account_address, transient_stake_account_address, withdraw_authority) =
            fixture.stake_accounts_for_validator(vote_account).await;

        extra_validator_accounts.push(ExtraValidatorAccounts {
            vote_account,
            validator_history_address,
            stake_account_address,
            transient_stake_account_address,
            withdraw_authority,
        })
    }

    crank_epoch_maintenance(&fixture, None).await;

    for extra_accounts in extra_validator_accounts.iter() {
        auto_add_validator(&fixture, extra_accounts).await;
    }

    crank_directed_stake_permissions(&fixture, &extra_validator_accounts).await;

    let set_meta_auth_ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::SetNewAuthority {
            config: fixture.steward_config.pubkey(),
            new_authority: fixture.keypair.pubkey(),
            admin: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::SetNewAuthority {
            authority_type:
                jito_steward::instructions::AuthorityType::SetDirectedStakeMetaUploadAuthority,
        }
        .data(),
    };

    let tx = Transaction::new_signed_with_payer(
        &[set_meta_auth_ix],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        fixture
            .ctx
            .borrow_mut()
            .get_new_latest_blockhash()
            .await
            .unwrap(),
    );

    fixture.submit_transaction_assert_success(tx).await;

    // Copy directed stake targets for the validators
    for extra_accounts in extra_validator_accounts.iter() {
        crank_copy_directed_stake_targets(&fixture, extra_accounts.vote_account, 0).await;
    }

    // Initial rebalance directed, no targets have been set
    crank_rebalance_directed(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1, 2],
    )
    .await;

    {
        let directed_stake_meta: DirectedStakeMeta = fixture
            .load_and_deserialize(&fixture.directed_stake_meta)
            .await;
        let directed_stake_applied_lamports = directed_stake_meta.targets[0].total_staked_lamports;
        let steward_state: StewardStateAccountV2 =
            fixture.load_and_deserialize(&fixture.steward_state).await;

        let stake_pool: StakePool = fixture
            .load_and_deserialize(&fixture.stake_pool_meta.stake_pool)
            .await;
        let reserve_balance = fixture
            .get_account(&stake_pool.reserve_stake)
            .await
            .lamports;
        assert!(reserve_balance == 49990151360u64);

        // All validators should have the minimum balance of 3282880 lamports and no directed stake
        for i in 0..3 {
            let validator_lamport_balance = steward_state.state.validator_lamport_balances[i];
            let directed_stake_lamports = directed_stake_meta.directed_stake_lamports[i];
            let validator_list: ValidatorList = fixture
                .load_and_deserialize(&fixture.stake_pool_meta.validator_list)
                .await;
            let validator_list_lamport_balance = validator_list.validators[i].active_stake_lamports;
            assert!(validator_lamport_balance == 3282880u64);
            assert!(validator_list_lamport_balance.0 == 3282880u64.to_le_bytes());
            assert!(directed_stake_lamports == 0);
            assert!(directed_stake_applied_lamports == 0);
            drop(validator_list);
        }
    }

    fixture.advance_num_slots(250_000).await;
    crank_idle(&fixture).await;
    crank_validator_history_accounts_no_credits(&fixture, &extra_validator_accounts, &[0, 1, 2])
        .await;
    crank_compute_score(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1, 2],
    )
    .await;
    crank_compute_delegations(&fixture).await;

    fixture.advance_num_slots(160_000).await;
    crank_idle(&fixture).await;

    crank_compute_instant_unstake(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1, 2],
    )
    .await;
    crank_rebalance(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1, 2],
    )
    .await;

    for extra_accounts in extra_validator_accounts.iter() {
        crank_copy_directed_stake_targets(&fixture, extra_accounts.vote_account, 1_000_000_000)
            .await;
    }

    let stake_rent = fixture.fetch_stake_rent().await;
    let stake_program_minimum = fixture.fetch_minimum_delegation().await;
    let pool_minimum_delegation = minimum_delegation(stake_program_minimum);

    fixture.advance_num_epochs(1, 10).await;
    crank_stake_pool(&fixture).await;
    crank_epoch_maintenance(&fixture, None).await;

    {
        let directed_stake_meta: Box<DirectedStakeMeta> = Box::new(
            fixture
                .load_and_deserialize(&fixture.directed_stake_meta)
                .await,
        );
        let directed_stake_applied_lamports = directed_stake_meta.targets[0].total_staked_lamports;
        let steward_state: Box<StewardStateAccountV2> =
            Box::new(fixture.load_and_deserialize(&fixture.steward_state).await);

        let stake_pool: Box<StakePool> = Box::new(
            fixture
                .load_and_deserialize(&fixture.stake_pool_meta.stake_pool)
                .await,
        );
        let reserve_balance = fixture
            .get_account(&stake_pool.reserve_stake)
            .await
            .lamports;
        assert!(reserve_balance == 6848640u64);

        // Assert index 0 has 24997217120 validator list balance
        let validator_list: Box<ValidatorList> = Box::new(
            fixture
                .load_and_deserialize(&fixture.stake_pool_meta.validator_list)
                .await,
        );
        // Validator at index 0 should have roughly half of the original 49990151360u64 reserve
        let validator_list_lamport_balance = validator_list.validators[0].active_stake_lamports;
        assert!(validator_list_lamport_balance.0 == 24997217120u64.to_le_bytes());

        // Index 1 has no undirected stake, does not have a valid score
        let validator_lamport_balance = steward_state.state.validator_lamport_balances[1];
        assert!(validator_lamport_balance == 3282880u64);

        assert!(directed_stake_applied_lamports == 0);
    }

    // Deposit SOL into the pool
    sol_deposit(&fixture, 3_000_000_000).await;

    // Assert reserve balance is reflects the deposit
    {
        let stake_pool: StakePool = fixture
            .load_and_deserialize(&fixture.stake_pool_meta.stake_pool)
            .await;
        let reserve_balance = fixture
            .get_account(&stake_pool.reserve_stake)
            .await
            .lamports;
        assert!(reserve_balance == 6848640u64 + 3_000_000_000u64);
    }

    crank_rebalance_directed(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1, 2],
    )
    .await;

    {
        let directed_stake_meta: Box<DirectedStakeMeta> = Box::new(
            fixture
                .load_and_deserialize(&fixture.directed_stake_meta)
                .await,
        );
        let directed_stake_applied_lamports = directed_stake_meta.targets[0].total_staked_lamports;
        let steward_state: Box<StewardStateAccountV2> =
            Box::new(fixture.load_and_deserialize(&fixture.steward_state).await);
        let previous_validator_lamport_balance = 24997217120u64;
        let validator_lamport_balance = steward_state.state.validator_lamport_balances[0];
        let directed_stake_lamports = directed_stake_meta.directed_stake_lamports[0];
        let validator_list: Box<ValidatorList> = Box::new(
            fixture
                .load_and_deserialize(&fixture.stake_pool_meta.validator_list)
                .await,
        );
        let validator_list_lamport_balance = validator_list.validators[0].active_stake_lamports;
        assert!(directed_stake_applied_lamports == 1_000_000_000u64);
        assert!(directed_stake_lamports == directed_stake_applied_lamports);
        assert!(validator_lamport_balance == previous_validator_lamport_balance + 1_000_000_000u64);
        // Directed rebalance increase will not reflect in the validator list balance until the next stake pool update
        assert!(
            validator_list_lamport_balance.0 == (previous_validator_lamport_balance).to_le_bytes()
        );
    }

    let stake_pool: StakePool = fixture
        .load_and_deserialize(&fixture.stake_pool_meta.stake_pool)
        .await;
    stake_withdraw(
        &fixture,
        extra_validator_accounts[0].stake_account_address,
        extra_validator_accounts[0].withdraw_authority,
        stake_pool.pool_mint,
        stake_pool.manager_fee_account,
        3_000_000_000,
    )
    .await;

    fixture.advance_num_slots(250_000).await;
    crank_idle(&fixture).await;
    crank_validator_history_accounts_no_credits(&fixture, &extra_validator_accounts, &[0, 1, 2])
        .await;
    crank_compute_score(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1, 2],
    )
    .await;
    crank_compute_delegations(&fixture).await;

    fixture.advance_num_slots(160_000).await;
    crank_idle(&fixture).await;

    crank_compute_instant_unstake(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1, 2],
    )
    .await;
    crank_rebalance(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1, 2],
    )
    .await;

    fixture.advance_num_epochs(1, 10).await;
    crank_stake_pool(&fixture).await;
    crank_epoch_maintenance(&fixture, None).await;

    // SOL Withdraw the reserve balance minus (stake_rent + minimum_delegation)
    // This will give us clean numbers for our assertions. Otherwise the validator
    // which was withdrawn will receive a directed increase when its balances are adjusted
    {
        let stake_pool: StakePool = fixture
            .load_and_deserialize(&fixture.stake_pool_meta.stake_pool)
            .await;
        let reserve_balance = fixture
            .get_account(&stake_pool.reserve_stake)
            .await
            .lamports;
        let withdrawal_amount = reserve_balance
            .saturating_sub(stake_rent)
            .saturating_sub(pool_minimum_delegation);
        sol_withdraw(&fixture, withdrawal_amount).await;
    }

    crank_rebalance_directed(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1, 2],
    )
    .await;

    {
        let directed_stake_meta: Box<DirectedStakeMeta> = Box::new(
            fixture
                .load_and_deserialize(&fixture.directed_stake_meta)
                .await,
        );
        let directed_stake_applied_lamports = directed_stake_meta.targets[0].total_staked_lamports;
        let steward_state: Box<StewardStateAccountV2> =
            Box::new(fixture.load_and_deserialize(&fixture.steward_state).await);
        let validator_lamport_balance = steward_state.state.validator_lamport_balances[0];
        let directed_stake_lamports = directed_stake_meta.directed_stake_lamports[0];
        let validator_list: Box<ValidatorList> = Box::new(
            fixture
                .load_and_deserialize(&fixture.stake_pool_meta.validator_list)
                .await,
        );
        let validator_list_lamport_balance = validator_list.validators[0].active_stake_lamports;
        let previous_validator_lamport_balance = 24997217120u64 + 1_000_000_000u64;
        assert!(
            validator_list_lamport_balance.0
                == (previous_validator_lamport_balance - 3_000_000_000u64).to_le_bytes()
        );
        assert!(validator_lamport_balance == previous_validator_lamport_balance - 3_000_000_000u64);
        assert!(directed_stake_applied_lamports == 0);
        assert!(directed_stake_lamports == directed_stake_applied_lamports);
    }
    drop(fixture);
}

#[tokio::test]
async fn test_internal_lamport_tracking_with_deposit() {
    let mut fixture_accounts = FixtureDefaultAccounts::default();

    let unit_test_fixtures = StateMachineFixtures::default();

    // Note that these parameters are overriden in initialize_steward, just included here for completeness
    fixture_accounts.steward_config.parameters = unit_test_fixtures.config.parameters;

    fixture_accounts.validators = (0..3)
        .map(|i| ValidatorEntry {
            validator_history: unit_test_fixtures.validators[i],
            vote_account: unit_test_fixtures.vote_accounts[i].clone(),
            vote_address: unit_test_fixtures.validators[i].vote_account,
        })
        .collect();
    fixture_accounts.cluster_history = unit_test_fixtures.cluster_history;

    let mut fixture = TestFixture::new_from_accounts(fixture_accounts, HashMap::new()).await;

    fixture.steward_config = Keypair::new();
    fixture.steward_state = Pubkey::find_program_address(
        &[
            StewardStateAccount::SEED,
            fixture.steward_config.pubkey().as_ref(),
        ],
        &jito_steward::id(),
    )
    .0;

    fixture.advance_num_epochs(20, 10).await;
    fixture.initialize_stake_pool().await;
    fixture
        .initialize_steward(
            Some(UpdateParametersArgs {
                mev_commission_range: Some(10),
                epoch_credits_range: Some(20),
                commission_range: Some(20),
                scoring_delinquency_threshold_ratio: Some(0.85),
                instant_unstake_delinquency_threshold_ratio: Some(0.70),
                mev_commission_bps_threshold: Some(1000),
                commission_threshold: Some(5),
                historical_commission_threshold: Some(50),
                num_delegation_validators: Some(200),
                scoring_unstake_cap_bps: Some(750),
                instant_unstake_cap_bps: Some(10),
                stake_deposit_unstake_cap_bps: Some(10),
                instant_unstake_epoch_progress: Some(0.9),
                compute_score_slot_range: Some(1000),
                instant_unstake_inputs_epoch_progress: Some(0.50),
                num_epochs_between_scoring: Some(50),
                minimum_stake_lamports: Some(5_000_000_000),
                minimum_voting_epochs: Some(0),
                compute_score_epoch_progress: Some(0.50),
                undirected_stake_floor_lamports: Some(0),
                directed_stake_unstake_cap_bps: Some(10_000),
            }),
            None,
        )
        .await;

    let _steward: Box<StewardStateAccountV2> =
        Box::new(fixture.load_and_deserialize(&fixture.steward_state).await);

    fixture.directed_stake_meta = initialize_directed_stake_meta(&fixture).await;
    fixture.realloc_directed_stake_meta().await;

    let mut extra_validator_accounts = vec![];
    for i in 0..unit_test_fixtures.validators.len() {
        let vote_account = unit_test_fixtures.validator_list[i].vote_account_address;
        let (validator_history_address, _) = Pubkey::find_program_address(
            &[ValidatorHistory::SEED, vote_account.as_ref()],
            &validator_history::id(),
        );

        let (stake_account_address, transient_stake_account_address, withdraw_authority) =
            fixture.stake_accounts_for_validator(vote_account).await;

        extra_validator_accounts.push(ExtraValidatorAccounts {
            vote_account,
            validator_history_address,
            stake_account_address,
            transient_stake_account_address,
            withdraw_authority,
        })
    }

    crank_epoch_maintenance(&fixture, None).await;

    // Auto add validator - adds to validator list
    for extra_accounts in extra_validator_accounts.iter() {
        auto_add_validator(&fixture, extra_accounts).await;
    }

    crank_directed_stake_permissions(&fixture, &extra_validator_accounts).await;

    // Set the directed stake meta upload authority to the signer
    let set_meta_auth_ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::SetNewAuthority {
            config: fixture.steward_config.pubkey(),
            new_authority: fixture.keypair.pubkey(),
            admin: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::SetNewAuthority {
            authority_type:
                jito_steward::instructions::AuthorityType::SetDirectedStakeMetaUploadAuthority,
        }
        .data(),
    };

    let tx = Transaction::new_signed_with_payer(
        &[set_meta_auth_ix],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        fixture
            .ctx
            .borrow_mut()
            .get_new_latest_blockhash()
            .await
            .unwrap(),
    );

    fixture.submit_transaction_assert_success(tx).await;

    for extra_accounts in extra_validator_accounts.iter() {
        crank_copy_directed_stake_targets(&fixture, extra_accounts.vote_account, 1_600_000_000_000)
            .await;
    }

    crank_rebalance_directed(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1, 2],
    )
    .await;

    let stake_rent = fixture.fetch_stake_rent().await;
    let stake_program_minimum = fixture.fetch_minimum_delegation().await;
    let pool_minimum_delegation = minimum_delegation(stake_program_minimum);

    let (stake_account_address, _bump_seed) = Pubkey::find_program_address(
        &[
            extra_validator_accounts[0].vote_account.as_ref(),
            fixture.stake_pool_meta.stake_pool.as_ref(),
        ],
        &spl_stake_pool::id(),
    );

    let stake_pool: StakePool = fixture
        .load_and_deserialize(&fixture.stake_pool_meta.stake_pool)
        .await;

    let pre_deposit_validator_lamport_balance = {
        let steward_state: StewardStateAccountV2 =
            fixture.load_and_deserialize(&fixture.steward_state).await;
        steward_state.state.validator_lamport_balances[0]
    };

    stake_deposit(
        &fixture,
        stake_account_address,
        stake_pool.reserve_stake,
        extra_validator_accounts[0].withdraw_authority,
        stake_pool.pool_mint,
        stake_pool.manager_fee_account,
        20_000_000_000,
    )
    .await;

    fixture.advance_num_epochs(1, 10).await;
    crank_stake_pool(&fixture).await;
    crank_epoch_maintenance(&fixture, None).await;

    // Empty the reserve: withdraw reserve balance minus (stake_rent + minimum_delegation)
    // This gives us clean numbers for our assertions
    {
        let reserve_stake_account = fixture.get_account(&stake_pool.reserve_stake).await;
        let reserve_balance = reserve_stake_account.lamports;
        let withdrawal_amount = reserve_balance
            .saturating_sub(stake_rent)
            .saturating_sub(pool_minimum_delegation);

        if withdrawal_amount > 0 {
            sol_withdraw(&fixture, withdrawal_amount).await;
        }
    }

    crank_rebalance_directed(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1, 2],
    )
    .await;

    // Now that directed stake targets have both been:
    // - Delegated to
    // - Remain below their directed stake target amount
    // We expect that:
    // - The stake deposit will be applied to the directed stake amount
    // - Validator lamport balance (total staked lamports) includes the same deposited amount
    {
        let directed_stake_meta: DirectedStakeMeta = fixture
            .load_and_deserialize(&fixture.directed_stake_meta)
            .await;
        let steward_state: StewardStateAccountV2 =
            fixture.load_and_deserialize(&fixture.steward_state).await;
        let validator_lamport_balance = steward_state.state.validator_lamport_balances[0];
        let expected_staked_lamports =
            validator_lamport_balance - stake_rent - pool_minimum_delegation;
        assert!(directed_stake_meta.targets[0].total_staked_lamports == expected_staked_lamports);
        // Stake account will absorb both the deposit value and the stake rent value
        assert!(
            validator_lamport_balance
                == pre_deposit_validator_lamport_balance + 20_000_000_000 + stake_rent
        );
    }
    drop(fixture);
}

#[tokio::test]
async fn test_internal_lamport_tracking_with_deposit_meeting_target() {
    let mut fixture_accounts = FixtureDefaultAccounts::default();

    let unit_test_fixtures = StateMachineFixtures::default();

    // Note that these parameters are overriden in initialize_steward, just included here for completeness
    fixture_accounts.steward_config.parameters = unit_test_fixtures.config.parameters;

    fixture_accounts.validators = (0..3)
        .map(|i| ValidatorEntry {
            validator_history: unit_test_fixtures.validators[i],
            vote_account: unit_test_fixtures.vote_accounts[i].clone(),
            vote_address: unit_test_fixtures.validators[i].vote_account,
        })
        .collect();
    fixture_accounts.cluster_history = unit_test_fixtures.cluster_history;

    let mut fixture = TestFixture::new_from_accounts(fixture_accounts, HashMap::new()).await;

    fixture.steward_config = Keypair::new();
    fixture.steward_state = Pubkey::find_program_address(
        &[
            StewardStateAccount::SEED,
            fixture.steward_config.pubkey().as_ref(),
        ],
        &jito_steward::id(),
    )
    .0;

    fixture.advance_num_epochs(20, 10).await;
    fixture.initialize_stake_pool().await;
    fixture
        .initialize_steward(
            Some(UpdateParametersArgs {
                mev_commission_range: Some(10),
                epoch_credits_range: Some(20),
                commission_range: Some(20),
                scoring_delinquency_threshold_ratio: Some(0.85),
                instant_unstake_delinquency_threshold_ratio: Some(0.70),
                mev_commission_bps_threshold: Some(1000),
                commission_threshold: Some(5),
                historical_commission_threshold: Some(50),
                num_delegation_validators: Some(200),
                scoring_unstake_cap_bps: Some(750),
                instant_unstake_cap_bps: Some(10),
                stake_deposit_unstake_cap_bps: Some(10),
                instant_unstake_epoch_progress: Some(0.9),
                compute_score_slot_range: Some(1000),
                instant_unstake_inputs_epoch_progress: Some(0.50),
                num_epochs_between_scoring: Some(50),
                minimum_stake_lamports: Some(5_000_000_000),
                minimum_voting_epochs: Some(0),
                compute_score_epoch_progress: Some(0.50),
                undirected_stake_floor_lamports: Some(0),
                directed_stake_unstake_cap_bps: Some(10_000),
            }),
            None,
        )
        .await;

    let _steward: Box<StewardStateAccountV2> =
        Box::new(fixture.load_and_deserialize(&fixture.steward_state).await);

    fixture.directed_stake_meta = initialize_directed_stake_meta(&fixture).await;
    fixture.realloc_directed_stake_meta().await;

    let mut extra_validator_accounts = vec![];
    for i in 0..unit_test_fixtures.validators.len() {
        let vote_account = unit_test_fixtures.validator_list[i].vote_account_address;
        let (validator_history_address, _) = Pubkey::find_program_address(
            &[ValidatorHistory::SEED, vote_account.as_ref()],
            &validator_history::id(),
        );

        let (stake_account_address, transient_stake_account_address, withdraw_authority) =
            fixture.stake_accounts_for_validator(vote_account).await;

        extra_validator_accounts.push(ExtraValidatorAccounts {
            vote_account,
            validator_history_address,
            stake_account_address,
            transient_stake_account_address,
            withdraw_authority,
        })
    }

    crank_epoch_maintenance(&fixture, None).await;

    for extra_accounts in extra_validator_accounts.iter() {
        auto_add_validator(&fixture, extra_accounts).await;
    }

    crank_directed_stake_permissions(&fixture, &extra_validator_accounts).await;

    let set_meta_auth_ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::SetNewAuthority {
            config: fixture.steward_config.pubkey(),
            new_authority: fixture.keypair.pubkey(),
            admin: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::SetNewAuthority {
            authority_type:
                jito_steward::instructions::AuthorityType::SetDirectedStakeMetaUploadAuthority,
        }
        .data(),
    };

    let tx = Transaction::new_signed_with_payer(
        &[set_meta_auth_ix],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        fixture
            .ctx
            .borrow_mut()
            .get_new_latest_blockhash()
            .await
            .unwrap(),
    );

    fixture.submit_transaction_assert_success(tx).await;

    for extra_accounts in extra_validator_accounts.iter() {
        crank_copy_directed_stake_targets(&fixture, extra_accounts.vote_account, 30000000000).await;
    }

    crank_rebalance_directed(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1, 2],
    )
    .await;

    let stake_rent = fixture.fetch_stake_rent().await;
    let stake_program_minimum = fixture.fetch_minimum_delegation().await;
    let pool_minimum_delegation = minimum_delegation(stake_program_minimum);

    let (stake_account_address, _bump_seed) = Pubkey::find_program_address(
        &[
            extra_validator_accounts[0].vote_account.as_ref(),
            fixture.stake_pool_meta.stake_pool.as_ref(),
        ],
        &spl_stake_pool::id(),
    );

    let stake_pool: StakePool = fixture
        .load_and_deserialize(&fixture.stake_pool_meta.stake_pool)
        .await;

    // Here check that: reserve has been completely delegated to directed stake targets
    {
        let reserve_stake_account = fixture.get_account(&stake_pool.reserve_stake).await;
        let available_reserve_lamports = reserve_stake_account.lamports - stake_rent;
        assert!(available_reserve_lamports == 0);
    }

    stake_deposit(
        &fixture,
        stake_account_address,
        stake_pool.reserve_stake,
        extra_validator_accounts[0].withdraw_authority,
        stake_pool.pool_mint,
        stake_pool.manager_fee_account,
        20_000_000_000,
    )
    .await;

    fixture.advance_num_epochs(1, 10).await;
    crank_stake_pool(&fixture).await;
    crank_epoch_maintenance(&fixture, None).await;

    crank_rebalance_directed(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1, 2],
    )
    .await;

    // Now that directed stake targets have both been:
    // - Delegated to
    // - Remain below their directed stake target amount
    // We expect that:
    // - The stake deposit will be applied to the directed stake amount
    // - Validator lamport balance (total staked lamports) includes the same deposited amount
    // - Although the deposit was greater than the directed stake target, the directed stake is not to exceed the target amount
    {
        let directed_stake_meta: DirectedStakeMeta = fixture
            .load_and_deserialize(&fixture.directed_stake_meta)
            .await;
        let steward_state: StewardStateAccountV2 =
            fixture.load_and_deserialize(&fixture.steward_state).await;
        assert!(directed_stake_meta.targets[0].total_staked_lamports == 30000000000);
        println!(
            "steward_state.state.validator_lamport_balances[0]: {:?}",
            steward_state.state.validator_lamport_balances[0]
        );
        println!(
            "directed_stake_meta.targets[0].total_staked_lamports: {:?}",
            directed_stake_meta.targets[0].total_staked_lamports
        );
        assert!(
            steward_state.state.validator_lamport_balances[0]
                == (directed_stake_meta.targets[0].total_staked_lamports
                    + stake_rent
                    + pool_minimum_delegation)
        );
    }

    drop(fixture);
}
