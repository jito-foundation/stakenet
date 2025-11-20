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
    stake_pool_utils::ValidatorList, StewardStateAccount, StewardStateAccountV2,
    UpdateParametersArgs,
};
use jito_steward::{
    COMPUTE_DELEGATIONS, COMPUTE_SCORE, EPOCH_MAINTENANCE, REBALANCE_DIRECTED_COMPLETE,
};
use solana_program::sysvar;
use solana_program_test::*;
#[allow(deprecated)]
use solana_sdk::{
    clock::Clock, signature::Keypair, signer::Signer, system_program, transaction::Transaction,
};
use tests::steward_fixtures::{
    auto_add_validator, cluster_history_default, crank_compute_delegations,
    crank_compute_instant_unstake, crank_compute_score, crank_copy_directed_stake_targets,
    crank_directed_stake_permissions, crank_epoch_maintenance, crank_idle, crank_rebalance,
    crank_rebalance_directed, crank_stake_pool, crank_validator_history_accounts,
    instant_remove_validator, serialized_cluster_history_account, ExtraValidatorAccounts,
    FixtureDefaultAccounts, StateMachineFixtures, TestFixture, ValidatorEntry,
};
use validator_history::{ClusterHistory, ValidatorHistory};

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

#[tokio::test]
async fn test_cycle() {
    let mut fixture_accounts = FixtureDefaultAccounts::default();

    let unit_test_fixtures = Box::<StateMachineFixtures>::default();

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

    // Modify validator history account with desired values

    let mut fixture = TestFixture::new_from_accounts(fixture_accounts, HashMap::new()).await;
    let ctx = &fixture.ctx;

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
                mev_commission_range: Some(10), // Set to pass validation, where epochs starts at 0
                epoch_credits_range: Some(20),  // Set to pass validation, where epochs starts at 0
                commission_range: Some(20),     // Set to pass validation, where epochs starts at 0
                scoring_delinquency_threshold_ratio: Some(0.85),
                instant_unstake_delinquency_threshold_ratio: Some(0.70),
                mev_commission_bps_threshold: Some(1000),
                commission_threshold: Some(5),
                historical_commission_threshold: Some(50),
                num_delegation_validators: Some(200),
                scoring_unstake_cap_bps: Some(750),
                instant_unstake_cap_bps: Some(10),
                stake_deposit_unstake_cap_bps: Some(10),
                instant_unstake_epoch_progress: Some(0.90),
                compute_score_slot_range: Some(1000),
                instant_unstake_inputs_epoch_progress: Some(0.50),
                num_epochs_between_scoring: Some(2), // 2 epoch cycle
                minimum_stake_lamports: Some(5_000_000_000),
                minimum_voting_epochs: Some(0), // Set to pass validation, where epochs starts at 0
                compute_score_epoch_progress: Some(0.50),
                undirected_stake_floor_lamports: Some(0), // No undirected stake floor
                directed_stake_unstake_cap_bps: Some(10_000),
            }),
            None,
        )
        .await;

    let _steward: StewardStateAccountV2 =
        fixture.load_and_deserialize(&fixture.steward_state).await;
    let _directed_stake_meta = initialize_directed_stake_meta(&fixture).await;
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

    // Add validators to whitelist and directed_stake_meta before rebalancing
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
        crank_copy_directed_stake_targets(&fixture, extra_accounts.vote_account, 0).await;
    }

    crank_rebalance_directed(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1, 2],
    )
    .await;
    println!("Rebalance directed 1");

    {
        // Assert size of validators list
        let validator_list: ValidatorList = fixture
            .load_and_deserialize(&fixture.stake_pool_meta.validator_list)
            .await;
        assert_eq!(validator_list.validators.len(), 3);
        println!("Validator List Length: {}", validator_list.validators.len());
    }

    fixture.advance_num_slots(250_000).await;
    crank_idle(&fixture).await;

    crank_compute_score(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1, 2],
    )
    .await;

    crank_compute_delegations(&fixture).await;

    let clock: Clock = ctx.borrow_mut().banks_client.get_sysvar().await.unwrap();

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

    println!("Advancing epoch from {} (expected 20)", clock.epoch);

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

    println!("Rebalance directed 2");

    fixture.advance_num_slots(250_000).await;
    crank_idle(&fixture).await;

    // Update validator history values
    crank_validator_history_accounts(&fixture, &extra_validator_accounts, &[0, 1, 2]).await;

    // Not a scoring cycle, skip cranking scores and delegations
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

    println!("Advancing epoch from {} (expected 21)", clock.epoch);

    crank_stake_pool(&fixture).await;

    crank_epoch_maintenance(&fixture, None).await;

    crank_rebalance_directed(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1, 2],
    )
    .await;

    fixture.advance_num_slots(250_000).await;
    // Update validator history values
    crank_validator_history_accounts(&fixture, &extra_validator_accounts, &[0, 1, 2]).await;
    crank_idle(&fixture).await;
    crank_compute_score(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1, 2],
    )
    .await;

    let clock: Clock = ctx.borrow_mut().banks_client.get_sysvar().await.unwrap();
    let state_account: StewardStateAccountV2 =
        fixture.load_and_deserialize(&fixture.steward_state).await;
    let state = state_account.state;

    assert!(matches!(
        state.state_tag,
        jito_steward::StewardStateEnum::ComputeDelegations
    ));
    assert_eq!(state.current_epoch, clock.epoch);
    assert_eq!(state.next_cycle_epoch, clock.epoch + 2);
    assert_eq!(state.instant_unstake_total, 0);
    assert_eq!(state.scoring_unstake_total, 0);
    assert_eq!(state.stake_deposit_unstake_total, 0);
    assert_eq!(state.validators_added, 0);
    assert!(state.validators_to_remove.is_empty());
    assert_eq!(
        state.status_flags,
        COMPUTE_SCORE | REBALANCE_DIRECTED_COMPLETE | EPOCH_MAINTENANCE
    );

    // All other values are reset
    drop(fixture);
}

#[tokio::test]
async fn test_cycle_with_directed_stake_persistent_unstake_state() {
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

    // Modify validator history account with desired values

    let mut fixture = TestFixture::new_from_accounts(fixture_accounts, HashMap::new()).await;
    let ctx = &fixture.ctx;

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
                num_epochs_between_scoring: Some(1), // 1 epoch cycle
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
        crank_copy_directed_stake_targets(&fixture, extra_accounts.vote_account, 10_000_000_000)
            .await;
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

        for target in directed_stake_meta.targets.iter() {
            if target.vote_pubkey == Pubkey::default() {
                continue;
            }
            println!(
                "Staked lamports for validator {:?}: {:?}",
                target.vote_pubkey, target.total_staked_lamports
            );
            assert_eq!(target.total_staked_lamports, 10_000_000_000);
        }
    }
    fixture.advance_num_slots(250_000).await;
    crank_idle(&fixture).await;

    crank_compute_score(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1, 2],
    )
    .await;

    crank_compute_delegations(&fixture).await;

    let clock: Clock = ctx.borrow_mut().banks_client.get_sysvar().await.unwrap();

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

    println!("Advancing epoch from {} (expected 20)", clock.epoch);

    fixture.advance_num_epochs(1, 10).await;

    crank_stake_pool(&fixture).await;

    crank_epoch_maintenance(&fixture, None).await;

    for extra_accounts in extra_validator_accounts.iter() {
        crank_copy_directed_stake_targets(&fixture, extra_accounts.vote_account, 1).await;
    }

    crank_rebalance_directed(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0],
    )
    .await;

    {
        let directed_stake_meta: DirectedStakeMeta = fixture
            .load_and_deserialize(&fixture.directed_stake_meta)
            .await;
        println!(
            "Directed unstake total: {}",
            directed_stake_meta.directed_unstake_total
        );
        assert!(directed_stake_meta.directed_unstake_total > 9_000_000_000);
    }

    crank_rebalance_directed(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[1],
    )
    .await;

    {
        let directed_stake_meta: DirectedStakeMeta = fixture
            .load_and_deserialize(&fixture.directed_stake_meta)
            .await;

        assert!(directed_stake_meta.directed_unstake_total > 19_000_000_000);
    }

    crank_rebalance_directed(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[2],
    )
    .await;

    fixture.advance_num_epochs(1, 10).await;

    // Epoch maintenance can reset an incomplete state machine cycle
    crank_stake_pool(&fixture).await;
    crank_epoch_maintenance(&fixture, None).await;

    {
        let directed_stake_meta: Box<DirectedStakeMeta> = Box::new(
            fixture
                .load_and_deserialize(&fixture.directed_stake_meta)
                .await,
        );

        // Total is reset at epoch maintenance
        assert!(directed_stake_meta.directed_unstake_total == 0);
    }
    drop(fixture);
}

#[tokio::test]
async fn test_cycle_with_directed_stake_unstake_minimum_delegation() {
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

    // Modify validator history account with desired values

    let mut fixture = TestFixture::new_from_accounts(fixture_accounts, HashMap::new()).await;
    let ctx = &fixture.ctx;

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
                num_epochs_between_scoring: Some(2), // 2 epoch cycle
                minimum_stake_lamports: Some(5_000_000_000),
                minimum_voting_epochs: Some(0),
                compute_score_epoch_progress: Some(0.50),
                undirected_stake_floor_lamports: Some(0),
                directed_stake_unstake_cap_bps: Some(10_000),
            }),
            None,
        )
        .await;

    let _steward: StewardStateAccountV2 =
        fixture.load_and_deserialize(&fixture.steward_state).await;
    let _directed_stake_meta = initialize_directed_stake_meta(&fixture).await;
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
        crank_copy_directed_stake_targets(&fixture, extra_accounts.vote_account, 10_000_000_000)
            .await;
    }

    crank_rebalance_directed(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1, 2],
    )
    .await;

    let directed_stake_meta: DirectedStakeMeta =
        fixture.load_and_deserialize(&_directed_stake_meta).await;

    for target in directed_stake_meta.targets.iter() {
        if target.vote_pubkey == Pubkey::default() {
            continue;
        }
        println!(
            "Staked lamports for validator {:?}: {:?}",
            target.vote_pubkey, target.total_staked_lamports
        );
        assert_eq!(target.total_staked_lamports, 10_000_000_000);
    }

    fixture.advance_num_slots(250_000).await;
    crank_idle(&fixture).await;

    crank_compute_score(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1, 2],
    )
    .await;

    crank_compute_delegations(&fixture).await;

    let clock: Clock = ctx.borrow_mut().banks_client.get_sysvar().await.unwrap();

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

    println!("Advancing epoch from {} (expected 20)", clock.epoch);

    fixture.advance_num_epochs(1, 10).await;

    crank_stake_pool(&fixture).await;

    crank_epoch_maintenance(&fixture, None).await;

    for extra_accounts in extra_validator_accounts.iter() {
        crank_copy_directed_stake_targets(&fixture, extra_accounts.vote_account, 9_999_999_999)
            .await;
    }

    crank_rebalance_directed(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1, 2],
    )
    .await;

    let directed_stake_meta: DirectedStakeMeta =
        fixture.load_and_deserialize(&_directed_stake_meta).await;

    for target in directed_stake_meta.targets.iter() {
        if target.vote_pubkey == Pubkey::default() {
            continue;
        }
        println!(
            "Staked lamports for validator {:?}: {:?}",
            target.vote_pubkey, target.total_staked_lamports
        );
        // Minimum delegation will result in no change to stake
        assert!(target.staked_last_updated_epoch == 21);
        assert!(target.total_staked_lamports == 10_000_000_000);
    }

    drop(fixture);
}

#[tokio::test]
async fn test_cycle_with_directed_stake_unstake_cap() {
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

    // Modify validator history account with desired values

    let mut fixture = TestFixture::new_from_accounts(fixture_accounts, HashMap::new()).await;
    let ctx = &fixture.ctx;

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
                mev_commission_range: Some(10), // Set to pass validation, where epochs starts at 0
                epoch_credits_range: Some(20),  // Set to pass validation, where epochs starts at 0
                commission_range: Some(20),     // Set to pass validation, where epochs starts at 0
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
                num_epochs_between_scoring: Some(2), // 2 epoch cycle
                minimum_stake_lamports: Some(5_000_000_000),
                minimum_voting_epochs: Some(0), // Set to pass validation, where epochs starts at 0
                compute_score_epoch_progress: Some(0.50),
                undirected_stake_floor_lamports: Some(0),
                directed_stake_unstake_cap_bps: Some(100),
            }),
            None,
        )
        .await;

    let _steward: StewardStateAccountV2 =
        fixture.load_and_deserialize(&fixture.steward_state).await;
    let _directed_stake_meta = initialize_directed_stake_meta(&fixture).await;
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
        crank_copy_directed_stake_targets(&fixture, extra_accounts.vote_account, 10_000_000_000)
            .await;
    }

    crank_rebalance_directed(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1, 2],
    )
    .await;

    let directed_stake_meta: DirectedStakeMeta =
        fixture.load_and_deserialize(&_directed_stake_meta).await;

    for target in directed_stake_meta.targets.iter() {
        if target.vote_pubkey == Pubkey::default() {
            continue;
        }
        println!(
            "Staked lamports for validator {:?}: {:?}",
            target.vote_pubkey, target.total_staked_lamports
        );
        assert_eq!(target.total_staked_lamports, 10_000_000_000);
    }

    fixture.advance_num_slots(250_000).await;
    crank_idle(&fixture).await;

    crank_compute_score(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1, 2],
    )
    .await;

    crank_compute_delegations(&fixture).await;

    let clock: Clock = ctx.borrow_mut().banks_client.get_sysvar().await.unwrap();

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

    println!("Advancing epoch from {} (expected 20)", clock.epoch);

    fixture.advance_num_epochs(1, 10).await;

    crank_stake_pool(&fixture).await;

    crank_epoch_maintenance(&fixture, None).await;

    for extra_accounts in extra_validator_accounts.iter() {
        crank_copy_directed_stake_targets(&fixture, extra_accounts.vote_account, 0).await;
    }

    crank_rebalance_directed(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1, 2],
    )
    .await;

    let directed_stake_meta: DirectedStakeMeta =
        fixture.load_and_deserialize(&_directed_stake_meta).await;

    for target in directed_stake_meta.targets.iter() {
        if target.vote_pubkey == Pubkey::default() {
            continue;
        }
        println!(
            "Staked lamports for validator {:?}: {:?}",
            target.vote_pubkey, target.total_staked_lamports
        );
        assert!(target.staked_last_updated_epoch == 21);
        assert!(target.total_staked_lamports < 10_000_000_000);
        assert!(target.total_staked_lamports > 9_000_000_000);
    }

    drop(fixture);
}

#[tokio::test]
async fn test_cycle_with_directed_stake_noop_copy() {
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

    // Modify validator history account with desired values

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
                mev_commission_range: Some(10), // Set to pass validation, where epochs starts at 0
                epoch_credits_range: Some(20),  // Set to pass validation, where epochs starts at 0
                commission_range: Some(20),     // Set to pass validation, where epochs starts at 0
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
                num_epochs_between_scoring: Some(2), // 2 epoch cycle
                minimum_stake_lamports: Some(5_000_000_000),
                minimum_voting_epochs: Some(0), // Set to pass validation, where epochs starts at 0
                compute_score_epoch_progress: Some(0.50),
                undirected_stake_floor_lamports: Some(0),
                directed_stake_unstake_cap_bps: Some(10_000),
            }),
            None,
        )
        .await;

    let _steward: StewardStateAccountV2 =
        fixture.load_and_deserialize(&fixture.steward_state).await;
    let _directed_stake_meta = initialize_directed_stake_meta(&fixture).await;
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

    // Copy directed stake targets for a single validator to trigger an attempted increase
    for extra_accounts in extra_validator_accounts.iter() {
        crank_copy_directed_stake_targets(&fixture, extra_accounts.vote_account, 10_000_000_000)
            .await;
    }

    crank_rebalance_directed(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1, 2],
    )
    .await;

    let directed_stake_meta: DirectedStakeMeta =
        fixture.load_and_deserialize(&_directed_stake_meta).await;

    for target in directed_stake_meta.targets.iter() {
        if target.vote_pubkey == Pubkey::default() {
            continue;
        }
        println!(
            "Staked lamports for validator {:?}: {:?}",
            target.vote_pubkey, target.total_staked_lamports
        );
        println!(
            "Staked last updated epoch: {:?}",
            target.staked_last_updated_epoch
        );
        println!(
            "Target last updated epoch: {:?}",
            target.target_last_updated_epoch
        );
        // undirected floor
    }

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

    fixture.advance_num_epochs(1, 10).await;

    // Epoch maintenance can reset an incomplete state machine cycle
    crank_stake_pool(&fixture).await;
    crank_epoch_maintenance(&fixture, None).await;
    {
        let directed_stake_meta: DirectedStakeMeta =
            fixture.load_and_deserialize(&_directed_stake_meta).await;
        for target in directed_stake_meta.targets.iter() {
            if target.vote_pubkey == Pubkey::default() {
                continue;
            }
            // Staked lamports remain the same, but last updated epoch changes
            assert_eq!(target.total_staked_lamports, 10_000_000_000);
            assert_eq!(target.staked_last_updated_epoch, 21);
        }
    }

    crank_rebalance_directed(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1, 2],
    )
    .await;

    // State machine can progress despite no directed stake target changes
    crank_idle(&fixture).await;

    drop(fixture);
}

#[tokio::test]
async fn test_cycle_with_directed_stake_partial_copy() {
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

    // Modify validator history account with desired values

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
                mev_commission_range: Some(10), // Set to pass validation, where epochs starts at 0
                epoch_credits_range: Some(20),  // Set to pass validation, where epochs starts at 0
                commission_range: Some(20),     // Set to pass validation, where epochs starts at 0
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
                num_epochs_between_scoring: Some(2), // 2 epoch cycle
                minimum_stake_lamports: Some(5_000_000_000),
                minimum_voting_epochs: Some(0), // Set to pass validation, where epochs starts at 0
                compute_score_epoch_progress: Some(0.50),
                undirected_stake_floor_lamports: Some(0),
                directed_stake_unstake_cap_bps: Some(10_000),
            }),
            None,
        )
        .await;

    let _steward: StewardStateAccountV2 =
        fixture.load_and_deserialize(&fixture.steward_state).await;
    let _directed_stake_meta = initialize_directed_stake_meta(&fixture).await;
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

    // Copy directed stake targets for a single validator to trigger an attempted increase
    for extra_accounts in extra_validator_accounts.iter() {
        crank_copy_directed_stake_targets(&fixture, extra_accounts.vote_account, 10_000_000_000)
            .await;
    }

    crank_rebalance_directed(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1, 2],
    )
    .await;

    let directed_stake_meta_before: DirectedStakeMeta =
        fixture.load_and_deserialize(&_directed_stake_meta).await;

    for target in directed_stake_meta_before.targets.iter() {
        if target.vote_pubkey == Pubkey::default() {
            continue;
        }
        // Target last updated and stake last updated should be the current epoch, 20
        assert_eq!(target.target_last_updated_epoch, 20);
        assert_eq!(target.staked_last_updated_epoch, 20);
    }

    let total_starget_lamports: u64 = directed_stake_meta_before
        .targets
        .iter()
        .map(|t| t.total_staked_lamports)
        .sum();

    assert_eq!(total_starget_lamports, 30_000_000_000);

    fixture.advance_num_epochs(1, 10).await;
    crank_stake_pool(&fixture).await;
    crank_epoch_maintenance(&fixture, None).await;

    // Only copy for the first extra validator to trigger partial update
    crank_copy_directed_stake_targets(
        &fixture,
        extra_validator_accounts[0].vote_account,
        20_000_000_000,
    )
    .await;

    crank_rebalance_directed(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1, 2],
    )
    .await;
    {
        let directed_stake_meta: DirectedStakeMeta =
            fixture.load_and_deserialize(&_directed_stake_meta).await;
        for target in directed_stake_meta.targets.iter() {
            if target.vote_pubkey == Pubkey::default() {
                continue;
            }
            // All staked lamports should be up-to-date due to directed rebalance
            assert_eq!(target.staked_last_updated_epoch, 21);
        }

        // Sum of target staked lamports should account for new partial copy increase
        let total_staked_lamports: u64 = directed_stake_meta
            .targets
            .iter()
            .map(|t| t.total_staked_lamports)
            .sum();

        assert_eq!(total_staked_lamports, 40_000_000_000);
    }
    // State machine can progress despite no directed stake target changes
    crank_idle(&fixture).await;

    drop(fixture);
}

#[tokio::test]
async fn test_cycle_with_directed_stake_undirected_floor() {
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

    // Modify validator history account with desired values

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
                mev_commission_range: Some(10), // Set to pass validation, where epochs starts at 0
                epoch_credits_range: Some(20),  // Set to pass validation, where epochs starts at 0
                commission_range: Some(20),     // Set to pass validation, where epochs starts at 0
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
                num_epochs_between_scoring: Some(2), // 2 epoch cycle
                minimum_stake_lamports: Some(5_000_000_000),
                minimum_voting_epochs: Some(0), // Set to pass validation, where epochs starts at 0
                compute_score_epoch_progress: Some(0.50),
                undirected_stake_floor_lamports: Some(u64::MAX), // Set high floor to disable undirected
                // stake increases
                directed_stake_unstake_cap_bps: Some(10_000),
            }),
            None,
        )
        .await;

    let _steward: StewardStateAccountV2 =
        fixture.load_and_deserialize(&fixture.steward_state).await;
    let _directed_stake_meta = initialize_directed_stake_meta(&fixture).await;
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

    // Copy directed stake targets for the validators to trigger an attempted increase
    for extra_accounts in extra_validator_accounts.iter() {
        crank_copy_directed_stake_targets(&fixture, extra_accounts.vote_account, 10_000_000_000)
            .await;
    }

    crank_rebalance_directed(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1, 2],
    )
    .await;

    let directed_stake_meta: DirectedStakeMeta =
        fixture.load_and_deserialize(&_directed_stake_meta).await;

    for target in directed_stake_meta.targets.iter() {
        if target.vote_pubkey == Pubkey::default() {
            continue;
        }
        println!(
            "Staked lamports for validator {:?}: {:?}",
            target.vote_pubkey, target.total_staked_lamports
        );
        assert_eq!(target.staked_last_updated_epoch, 20);
        assert_eq!(target.total_staked_lamports, 0); // No directed stake increases due to high
                                                     // undirected floor
    }

    drop(fixture);
}

#[tokio::test]
async fn test_cycle_with_directed_stake_increase_minimum_delegation() {
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

    // Modify validator history account with desired values

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
                mev_commission_range: Some(10), // Set to pass validation, where epochs starts at 0
                epoch_credits_range: Some(20),  // Set to pass validation, where epochs starts at 0
                commission_range: Some(20),     // Set to pass validation, where epochs starts at 0
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
                num_epochs_between_scoring: Some(2), // 2 epoch cycle
                minimum_stake_lamports: Some(5_000_000_000),
                minimum_voting_epochs: Some(0), // Set to pass validation, where epochs starts at 0
                compute_score_epoch_progress: Some(0.50),
                undirected_stake_floor_lamports: Some(0),
                directed_stake_unstake_cap_bps: Some(10_000),
            }),
            None,
        )
        .await;

    let _steward: StewardStateAccountV2 =
        fixture.load_and_deserialize(&fixture.steward_state).await;
    let _directed_stake_meta = initialize_directed_stake_meta(&fixture).await;
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

    for extra_accounts in extra_validator_accounts.iter() {
        crank_copy_directed_stake_targets(&fixture, extra_accounts.vote_account, 1).await;
    }

    crank_rebalance_directed(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1, 2],
    )
    .await;

    {
        let directed_stake_meta: DirectedStakeMeta =
            fixture.load_and_deserialize(&_directed_stake_meta).await;

        for target in directed_stake_meta.targets.iter() {
            if target.vote_pubkey == Pubkey::default() {
                continue;
            }
            println!(
                "Target last updated epoch: {:?}",
                target.staked_last_updated_epoch
            );
            println!(
                "Staked lamports for validator {:?}: {:?}",
                target.vote_pubkey, target.total_staked_lamports
            );
        }
    }

    drop(fixture);
}

#[tokio::test]
async fn test_cycle_with_directed_stake_targets() {
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

    // Modify validator history account with desired values

    let mut fixture = TestFixture::new_from_accounts(fixture_accounts, HashMap::new()).await;
    let ctx = &fixture.ctx;

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
                mev_commission_range: Some(10), // Set to pass validation, where epochs starts at 0
                epoch_credits_range: Some(20),  // Set to pass validation, where epochs starts at 0
                commission_range: Some(20),     // Set to pass validation, where epochs starts at 0
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
                num_epochs_between_scoring: Some(2), // 2 epoch cycle
                minimum_stake_lamports: Some(5_000_000_000),
                minimum_voting_epochs: Some(0), // Set to pass validation, where epochs starts at 0
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
        // Each target should have staked_last_updated_epoch set to 20
        for target in directed_stake_meta.targets.iter() {
            if target.vote_pubkey == Pubkey::default() {
                continue;
            }
            println!(
                "Staked last updated epoch for validator {:?}: {:?}",
                target.vote_pubkey, target.staked_last_updated_epoch
            );
        }

        // Each target should have total_staked_lamports set to 9999000000
        for target in directed_stake_meta.targets.iter() {
            if target.vote_pubkey == Pubkey::default() {
                continue;
            }
            println!(
                "Target last updated epoch: {:?}",
                target.staked_last_updated_epoch
            );
            println!(
                "Staked lamports for validator {:?}: {:?}",
                target.vote_pubkey, target.total_staked_lamports
            );
        }
    }

    {
        // Assert size of validators list
        let validator_list: ValidatorList = fixture
            .load_and_deserialize(&fixture.stake_pool_meta.validator_list)
            .await;
        assert_eq!(validator_list.validators.len(), 3);
    }

    fixture.advance_num_slots(250_000).await;
    crank_idle(&fixture).await;

    crank_compute_score(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1, 2],
    )
    .await;

    crank_compute_delegations(&fixture).await;

    let clock: Clock = ctx.borrow_mut().banks_client.get_sysvar().await.unwrap();

    crank_idle(&fixture).await;

    fixture.advance_num_slots(160_000).await;

    crank_idle(&fixture).await;

    crank_validator_history_accounts(&fixture, &extra_validator_accounts, &[0, 1, 2]).await;

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

    println!("Advancing epoch from {} (expecting 20)", clock.epoch);

    fixture.advance_num_epochs(1, 10).await;

    crank_stake_pool(&fixture).await;

    crank_epoch_maintenance(&fixture, None).await;

    for extra_accounts in extra_validator_accounts.iter() {
        crank_copy_directed_stake_targets(&fixture, extra_accounts.vote_account, 2_000_000_000)
            .await;
    }

    crank_rebalance_directed(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1, 2],
    )
    .await;
    println!("Rebalance directed 2");
    {
        let directed_stake_meta: DirectedStakeMeta = fixture
            .load_and_deserialize(&fixture.directed_stake_meta)
            .await;

        // Each target should have staked_last_updated_epoch set to 21
        for target in directed_stake_meta.targets.iter() {
            if target.vote_pubkey == Pubkey::default() {
                continue;
            }
            println!(
                "Target last updated epoch: {:?}",
                target.staked_last_updated_epoch
            );
            println!(
                "Staked last updated epoch for validator {:?}: {:?}",
                target.vote_pubkey, target.staked_last_updated_epoch
            );
            //assert_eq!(target.staked_last_updated_epoch, 21);
        }

        // Each target should have total_staked_lamports set to 9999000000
        for target in directed_stake_meta.targets.iter() {
            if target.vote_pubkey == Pubkey::default() {
                continue;
            }
            println!(
                "Staked lamports for validator {:?}: {:?}",
                target.vote_pubkey, target.total_staked_lamports
            );
            //assert_eq!(target.total_staked_lamports, 9999000000);
        }
    }

    fixture.advance_num_slots(250_000).await;

    crank_idle(&fixture).await;

    // Update validator history values
    crank_validator_history_accounts(&fixture, &extra_validator_accounts, &[0, 1, 2]).await;

    // Not a scoring cycle, skip cranking scores and delegations
    fixture.advance_num_slots(170_000).await;
    crank_idle(&fixture).await;

    println!("Cranked validator history account");

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

    println!("Advancing epoch from {}", clock.epoch);

    crank_stake_pool(&fixture).await;

    crank_epoch_maintenance(&fixture, None).await;

    // Update validator history values
    crank_validator_history_accounts(&fixture, &extra_validator_accounts, &[0, 1, 2]).await;

    // Copy targets with half balance to force a decrease rebalance
    for extra_accounts in extra_validator_accounts.iter() {
        crank_copy_directed_stake_targets(&fixture, extra_accounts.vote_account, 5_000_000).await;
    }

    crank_rebalance_directed(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1, 2],
    )
    .await;
    println!("Rebalance directed 3");

    {
        let directed_stake_meta: DirectedStakeMeta = fixture
            .load_and_deserialize(&fixture.directed_stake_meta)
            .await;

        // Each target should have staked_last_updated_epoch set to 22
        for target in directed_stake_meta.targets.iter() {
            if target.vote_pubkey == Pubkey::default() {
                continue;
            }
            println!(
                "Target last updated epoch: {:?}",
                target.staked_last_updated_epoch
            );
            println!(
                "Staked last updated epoch for validator {:?}: {:?}",
                target.vote_pubkey, target.staked_last_updated_epoch
            );
            //assert_eq!(target.staked_last_updated_epoch, 22);
        }

        // Each target should have total_staked_lamports set to 9999000000
        for target in directed_stake_meta.targets.iter() {
            if target.vote_pubkey == Pubkey::default() {
                continue;
            }
            println!(
                "Staked lamports for validator {:?}: {:?}",
                target.vote_pubkey, target.total_staked_lamports
            );
            //assert_eq!(target.total_staked_lamports, 9999000000);
        }
    }

    let clock: Clock = ctx.borrow_mut().banks_client.get_sysvar().await.unwrap();
    let state_account: Box<StewardStateAccountV2> =
        Box::new(fixture.load_and_deserialize(&fixture.steward_state).await);
    let state = Box::new(state_account.state);

    assert!(matches!(
        state.state_tag,
        jito_steward::StewardStateEnum::Idle
    ));
    assert_eq!(state.current_epoch, clock.epoch);
    assert_eq!(state.next_cycle_epoch, clock.epoch);
    assert_eq!(state.instant_unstake_total, 0);
    assert_eq!(state.scoring_unstake_total, 0);
    assert_eq!(state.stake_deposit_unstake_total, 0);
    assert_eq!(state.validators_added, 0);
    assert!(state.validators_to_remove.is_empty());
    assert_eq!(
        state.status_flags,
        COMPUTE_SCORE | REBALANCE_DIRECTED_COMPLETE | EPOCH_MAINTENANCE | COMPUTE_DELEGATIONS
    );

    // All other values are reset

    drop(fixture);
}

#[tokio::test]
async fn test_remove_validator_mid_epoch() {
    /*
      Tests that a validator removed at an arbitrary point in the cycle is not included in the current cycle's consideration,
      even though it is still in the validator list, and the next epoch, it is removed from the validator list.
    */

    let mut fixture_accounts = FixtureDefaultAccounts::default();

    let unit_test_fixtures = Box::<StateMachineFixtures>::default();

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
    let ctx = &fixture.ctx;

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
                mev_commission_range: Some(10), // Set to pass validation, where epochs starts at 0
                epoch_credits_range: Some(20),  // Set to pass validation, where epochs starts at 0
                commission_range: Some(20),     // Set to pass validation, where epochs starts at 0
                scoring_delinquency_threshold_ratio: Some(0.85),
                instant_unstake_delinquency_threshold_ratio: Some(0.70),
                mev_commission_bps_threshold: Some(1000),
                commission_threshold: Some(5),
                historical_commission_threshold: Some(50),
                num_delegation_validators: Some(200),
                scoring_unstake_cap_bps: Some(750),
                instant_unstake_cap_bps: Some(10),
                stake_deposit_unstake_cap_bps: Some(10),
                instant_unstake_epoch_progress: Some(0.00),
                compute_score_slot_range: Some(1000),
                instant_unstake_inputs_epoch_progress: Some(0.50),
                num_epochs_between_scoring: Some(2), // 2 epoch cycle
                minimum_stake_lamports: Some(5_000_000_000),
                minimum_voting_epochs: Some(0), // Set to pass validation, where epochs starts at 0
                compute_score_epoch_progress: Some(0.50),
                undirected_stake_floor_lamports: Some(10_000_000 * 1_000_000_000),
                directed_stake_unstake_cap_bps: Some(10_000),
            }),
            None,
        )
        .await;

    let _directed_stake_meta = initialize_directed_stake_meta(&fixture).await;
    fixture.realloc_directed_stake_meta().await;

    let mut extra_validator_accounts = vec![];
    for vote_account in unit_test_fixtures
        .validator_list
        .iter()
        .take(unit_test_fixtures.validators.len())
        .map(|v| v.vote_account_address)
    {
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
    // Auto add validator - adds validators 2 and 3
    for extra_accounts in extra_validator_accounts.iter().take(3) {
        auto_add_validator(&fixture, extra_accounts).await;
    }

    // Add validators to whitelist and directed_stake_meta before rebalancing
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

    for extra_accounts in extra_validator_accounts.iter().take(3) {
        crank_copy_directed_stake_targets(&fixture, extra_accounts.vote_account, 0).await;
    }

    crank_rebalance_directed(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1, 2],
    )
    .await;

    fixture.advance_num_slots(250_000).await;
    crank_idle(&fixture).await;

    crank_compute_score(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1, 2],
    )
    .await;

    crank_compute_delegations(&fixture).await;

    crank_compute_instant_unstake(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1],
    )
    .await;

    // Remove validator 2 in the middle of compute instant unstake
    let remove_validator_from_pool_ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::RemoveValidatorFromPool {
            config: fixture.steward_config.pubkey(),
            state_account: fixture.steward_state,
            stake_pool_program: spl_stake_pool::id(),
            stake_pool: fixture.stake_pool_meta.stake_pool,
            withdraw_authority: extra_validator_accounts[2].withdraw_authority,
            validator_list: fixture.stake_pool_meta.validator_list,
            stake_account: extra_validator_accounts[2].stake_account_address,
            transient_stake_account: extra_validator_accounts[2].transient_stake_account_address,
            clock: solana_sdk::sysvar::clock::id(),
            system_program: system_program::id(),
            stake_program: stake::program::id(),
            admin: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::RemoveValidatorFromPool {
            validator_list_index: 2,
        }
        .data(),
    };
    let blockhash = ctx.borrow_mut().get_new_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[remove_validator_from_pool_ix],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        blockhash,
    );
    fixture.submit_transaction_assert_success(tx).await;

    let state_account: StewardStateAccountV2 =
        fixture.load_and_deserialize(&fixture.steward_state).await;
    let state = state_account.state;
    assert!(matches!(
        state.state_tag,
        jito_steward::StewardStateEnum::ComputeInstantUnstake
    ));
    assert_eq!(state.validators_for_immediate_removal.count(), 1);
    assert!(state.validators_for_immediate_removal.get(2).unwrap());
    assert_eq!(state.num_pool_validators, 3);

    let validator_list: ValidatorList = fixture
        .load_and_deserialize(&fixture.stake_pool_meta.validator_list)
        .await;
    assert!(validator_list
        .validators
        .iter()
        .any(|v| v.vote_account_address == extra_validator_accounts[2].vote_account));
    assert!(validator_list.validators.len() == 3);
    println!("Stake Status: {:?}", validator_list.validators[2].status);

    // crank stake pool to remove validator from list
    crank_stake_pool(&fixture).await;

    let validator_list: ValidatorList = fixture
        .load_and_deserialize(&fixture.stake_pool_meta.validator_list)
        .await;
    assert!(!validator_list
        .validators
        .iter()
        .any(|v| v.vote_account_address == extra_validator_accounts[2].vote_account));
    assert!(validator_list.validators.len() == 2);

    instant_remove_validator(&fixture, 2).await;
    let state_account: StewardStateAccountV2 =
        fixture.load_and_deserialize(&fixture.steward_state).await;
    let state = state_account.state;
    assert!(matches!(
        state.state_tag,
        jito_steward::StewardStateEnum::ComputeInstantUnstake
    ));
    assert_eq!(state.validators_to_remove.count(), 0);
    assert_eq!(state.validators_for_immediate_removal.count(), 0);
    assert_eq!(state.num_pool_validators, 2);

    // Ensure validator history account for validator 0 is properly initialized
    // before calling ComputeInstantUnstake
    let _validator_history_address = fixture
        .initialize_validator_history_with_credits(extra_validator_accounts[0].vote_account, 0);

    // Ensure cluster history account is properly initialized
    let cluster_history_account =
        Pubkey::find_program_address(&[ClusterHistory::SEED], &validator_history::id()).0;
    let cluster_history = cluster_history_default();
    fixture.ctx.borrow_mut().set_account(
        &cluster_history_account,
        &serialized_cluster_history_account(cluster_history).into(),
    );

    // Update validator history accounts with current epoch data
    crank_validator_history_accounts(&fixture, &extra_validator_accounts, &[0, 1]).await;

    // Compute instant unstake transitions to Rebalance
    // Use validator 0 since validator 2 has been removed from the list
    crank_compute_instant_unstake(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0],
    )
    .await;

    crank_rebalance(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1],
    )
    .await;

    fixture.advance_num_epochs(1, 10).await;
    crank_stake_pool(&fixture).await;
    let validator_list: ValidatorList = fixture
        .load_and_deserialize(&fixture.stake_pool_meta.validator_list)
        .await;
    assert!(!validator_list
        .validators
        .iter()
        .any(|v| v.vote_account_address == extra_validator_accounts[2].vote_account));
    assert!(validator_list.validators.len() == 2);

    crank_epoch_maintenance(&fixture, None).await;
    let state_account: StewardStateAccountV2 =
        fixture.load_and_deserialize(&fixture.steward_state).await;
    let state = state_account.state;
    assert!(matches!(
        state.state_tag,
        jito_steward::StewardStateEnum::RebalanceDirected
    ));
    assert_eq!(state.validators_to_remove.count(), 0);
    assert_eq!(state.validators_for_immediate_removal.count(), 0);
    assert_eq!(state.num_pool_validators, 2);

    drop(fixture);
}

/// Tests that a validator added at an arbitrary point during the cycle does not get included in the
/// current cycle's consideration, but is included in the next cycle's scoring after ComputeScores is run.
#[tokio::test]
async fn test_add_validator_next_cycle() {
    let mut fixture_accounts = FixtureDefaultAccounts::default();

    let unit_test_fixtures = Box::<StateMachineFixtures>::default();

    fixture_accounts.steward_config.parameters = unit_test_fixtures.config.parameters;

    fixture_accounts.validators = (0..3)
        .map(|i| ValidatorEntry {
            validator_history: unit_test_fixtures.validators[i],
            vote_account: unit_test_fixtures.vote_accounts[i].clone(),
            vote_address: unit_test_fixtures.validators[i].vote_account,
        })
        .collect();
    fixture_accounts.cluster_history = unit_test_fixtures.cluster_history;

    // Modify validator history account with desired values

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
                mev_commission_range: Some(10), // Set to pass validation, where epochs starts at 0
                epoch_credits_range: Some(20),  // Set to pass validation, where epochs starts at 0
                commission_range: Some(20),     // Set to pass validation, where epochs starts at 0
                scoring_delinquency_threshold_ratio: Some(0.85),
                instant_unstake_delinquency_threshold_ratio: Some(0.70),
                mev_commission_bps_threshold: Some(1000),
                commission_threshold: Some(5),
                historical_commission_threshold: Some(50),
                num_delegation_validators: Some(200),
                scoring_unstake_cap_bps: Some(750),
                instant_unstake_cap_bps: Some(10),
                stake_deposit_unstake_cap_bps: Some(10),
                instant_unstake_epoch_progress: Some(0.00),
                compute_score_slot_range: Some(1000),
                instant_unstake_inputs_epoch_progress: Some(0.50),
                num_epochs_between_scoring: Some(1), // 1 epoch cycle
                minimum_stake_lamports: Some(5_000_000_000),
                minimum_voting_epochs: Some(0), // Set to pass validation, where epochs starts at 0
                compute_score_epoch_progress: Some(0.50),
                undirected_stake_floor_lamports: Some(10_000_000 * 1_000_000_000),
                directed_stake_unstake_cap_bps: Some(10_000),
            }),
            None,
        )
        .await;
    let _directed_stake_meta = initialize_directed_stake_meta(&fixture).await;
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
    // Auto add validator - adds validators 2 and 3
    let mut added_validators = vec![];
    for extra_accounts in extra_validator_accounts.iter().take(2) {
        auto_add_validator(&fixture, extra_accounts).await;
        added_validators.push(extra_accounts.clone());
    }

    // Add validators to whitelist and directed_stake_meta before rebalancing
    // Only add validators that were actually added to the validator list
    crank_directed_stake_permissions(&fixture, &added_validators).await;

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

    for extra_accounts in extra_validator_accounts.iter().take(2) {
        crank_copy_directed_stake_targets(&fixture, extra_accounts.vote_account, 0).await;
    }

    crank_rebalance_directed(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1],
    )
    .await;

    fixture.advance_num_slots(250_000).await;
    crank_idle(&fixture).await;

    crank_compute_score(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1],
    )
    .await;

    // Add in validator 2 at random time
    auto_add_validator(&fixture, &extra_validator_accounts[2]).await;

    let validator_list: ValidatorList = fixture
        .load_and_deserialize(&fixture.stake_pool_meta.validator_list)
        .await;
    assert!(validator_list
        .validators
        .iter()
        .any(|v| v.vote_account_address == extra_validator_accounts[2].vote_account));
    assert!(validator_list.validators.len() == 3);

    // Ensure that num_pool_validators isn't updated but validators_added is
    let state_account: StewardStateAccountV2 =
        fixture.load_and_deserialize(&fixture.steward_state).await;

    let state = state_account.state;

    assert!(matches!(
        state.state_tag,
        jito_steward::StewardStateEnum::ComputeDelegations
    ));
    assert_eq!(state.validators_added, 1);
    assert_eq!(state.num_pool_validators, 2);

    crank_compute_delegations(&fixture).await;
    crank_idle(&fixture).await;
    crank_compute_instant_unstake(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1],
    )
    .await;
    crank_rebalance(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1],
    )
    .await;

    fixture.advance_num_epochs(1, 10).await;

    crank_stake_pool(&fixture).await;
    crank_epoch_maintenance(&fixture, None).await;

    let state_account: StewardStateAccountV2 =
        fixture.load_and_deserialize(&fixture.steward_state).await;
    let state = state_account.state;

    assert!(matches!(
        state.state_tag,
        jito_steward::StewardStateEnum::RebalanceDirected
    ));
    assert_eq!(state.validators_added, 1);
    assert_eq!(state.num_pool_validators, 2);

    crank_validator_history_accounts(&fixture, &extra_validator_accounts, &[0, 1, 2]).await;

    crank_rebalance_directed(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1],
    )
    .await;
    fixture.advance_num_slots(250_000).await;
    crank_idle(&fixture).await;
    // Ensure we're in the next cycle
    crank_compute_score(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0],
    )
    .await;

    // Ensure that num_pool_validators is updated and validators_added is reset
    let state_account: StewardStateAccountV2 =
        fixture.load_and_deserialize(&fixture.steward_state).await;
    let state = state_account.state;

    assert!(matches!(
        state.state_tag,
        jito_steward::StewardStateEnum::ComputeScores
    ));

    assert_eq!(state.validators_added, 0);
    assert!(state.validators_to_remove.is_empty());
    assert_eq!(state.num_pool_validators, 3);

    // Ensure we can crank the new validator
    crank_compute_score(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[2],
    )
    .await;

    drop(fixture);
}

#[tokio::test]
async fn test_directed_stake_large_target_low_reserve() {
    let mut fixture_accounts = FixtureDefaultAccounts::default();

    let unit_test_fixtures = Box::<StateMachineFixtures>::default();

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

    // Modify validator history account with desired values

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
                mev_commission_range: Some(10), // Set to pass validation, where epochs starts at 0
                epoch_credits_range: Some(20),  // Set to pass validation, where epochs starts at 0
                commission_range: Some(20),     // Set to pass validation, where epochs starts at 0
                scoring_delinquency_threshold_ratio: Some(0.85),
                instant_unstake_delinquency_threshold_ratio: Some(0.70),
                mev_commission_bps_threshold: Some(1000),
                commission_threshold: Some(5),
                historical_commission_threshold: Some(50),
                num_delegation_validators: Some(200),
                scoring_unstake_cap_bps: Some(750),
                instant_unstake_cap_bps: Some(10),
                stake_deposit_unstake_cap_bps: Some(10),
                instant_unstake_epoch_progress: Some(0.90),
                compute_score_slot_range: Some(1000),
                instant_unstake_inputs_epoch_progress: Some(0.50),
                num_epochs_between_scoring: Some(2), // 2 epoch cycle
                minimum_stake_lamports: Some(5_000_000_000),
                minimum_voting_epochs: Some(0), // Set to pass validation, where epochs starts at 0
                compute_score_epoch_progress: Some(0.50),
                undirected_stake_floor_lamports: Some(0), // No undirected stake floor
                directed_stake_unstake_cap_bps: Some(10_000),
            }),
            None,
        )
        .await;

    let _steward: StewardStateAccountV2 =
        fixture.load_and_deserialize(&fixture.steward_state).await;
    let _directed_stake_meta = initialize_directed_stake_meta(&fixture).await;
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

    // Add validators to whitelist and directed_stake_meta before rebalancing
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
        crank_copy_directed_stake_targets(
            &fixture,
            extra_accounts.vote_account,
            1_000_000_000_000_000,
        )
        .await;
    }

    crank_rebalance_directed(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1, 2],
    )
    .await;
    {
        let directed_stake_meta: DirectedStakeMeta =
            fixture.load_and_deserialize(&_directed_stake_meta).await;
        for target in directed_stake_meta.targets.iter() {
            if target.vote_pubkey == Pubkey::default() {
                continue;
            }
            println!(
                "Staked lamports for validator {:?}: {:?}",
                target.vote_pubkey, target.total_staked_lamports
            );
        }
        for target in directed_stake_meta.targets.iter() {
            if target.vote_pubkey == Pubkey::default() {
                continue;
            }
            println!(
                "Target last updated epoch: {:?}",
                target.staked_last_updated_epoch
            );
        }
    }
    drop(fixture);
}
