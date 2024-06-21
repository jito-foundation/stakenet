use std::collections::HashMap;

use anchor_lang::{
    solana_program::{instruction::Instruction, pubkey::Pubkey, stake, sysvar},
    InstructionData, ToAccountMetas,
};
use jito_steward::{
    utils::{StakePool, ValidatorList},
    BitMask, Staker, StewardStateAccount, UpdateParametersArgs,
};
use solana_program_test::*;
use solana_sdk::{
    clock::Clock, compute_budget::ComputeBudgetInstruction, epoch_schedule::EpochSchedule,
    signature::Keypair, signer::Signer, system_program, transaction::Transaction,
};
use tests::steward_fixtures::{
    FixtureDefaultAccounts, StateMachineFixtures, TestFixture, ValidatorEntry,
};
use validator_history::ValidatorHistory;

pub struct ExtraValidatorAccounts {
    vote_account: Pubkey,
    validator_history_address: Pubkey,
    stake_account_address: Pubkey,
    transient_stake_account_address: Pubkey,
    withdraw_authority: Pubkey,
}

async fn _crank_stake_pool(fixture: &TestFixture) {
    let stake_pool: StakePool = fixture
        .load_and_deserialize(&fixture.stake_pool_meta.stake_pool)
        .await;
    let validator_list: ValidatorList = fixture
        .load_and_deserialize(&fixture.stake_pool_meta.validator_list)
        .await;
    let (initial_ixs, final_ixs) = spl_stake_pool::instruction::update_stake_pool(
        &spl_stake_pool::id(),
        &stake_pool.as_ref(),
        &validator_list.as_ref(),
        &fixture.stake_pool_meta.stake_pool,
        false,
    );

    let tx = Transaction::new_signed_with_payer(
        &initial_ixs,
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

    let tx = Transaction::new_signed_with_payer(
        &final_ixs,
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

async fn _crank_epoch_maintenance(fixture: &TestFixture, remove_indices: Option<&[usize]>) {
    let ctx = &fixture.ctx;
    // Epoch Maintenence
    if let Some(indices) = remove_indices {
        for i in indices {
            let ix = Instruction {
                program_id: jito_steward::id(),
                accounts: jito_steward::accounts::EpochMaintenance {
                    config: fixture.steward_config.pubkey(),
                    state_account: fixture.steward_state,
                    validator_list: fixture.stake_pool_meta.validator_list,
                    stake_pool: fixture.stake_pool_meta.stake_pool,
                }
                .to_account_metas(None),
                data: jito_steward::instruction::EpochMaintenance {
                    validator_index_to_remove: Some(*i),
                }
                .data(),
            };
            let blockhash = ctx.borrow_mut().get_new_latest_blockhash().await.unwrap();
            let tx = Transaction::new_signed_with_payer(
                &[ix],
                Some(&fixture.keypair.pubkey()),
                &[&fixture.keypair],
                blockhash,
            );
            fixture.submit_transaction_assert_success(tx).await;
        }
    } else {
        let ix = Instruction {
            program_id: jito_steward::id(),
            accounts: jito_steward::accounts::EpochMaintenance {
                config: fixture.steward_config.pubkey(),
                state_account: fixture.steward_state,
                validator_list: fixture.stake_pool_meta.validator_list,
                stake_pool: fixture.stake_pool_meta.stake_pool,
            }
            .to_account_metas(None),
            data: jito_steward::instruction::EpochMaintenance {
                validator_index_to_remove: None,
            }
            .data(),
        };

        let blockhash = ctx.borrow_mut().get_new_latest_blockhash().await.unwrap();
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&fixture.keypair.pubkey()),
            &[&fixture.keypair],
            blockhash,
        );
        fixture.submit_transaction_assert_success(tx).await;
    }
}

async fn _auto_add_validator(fixture: &TestFixture, extra_accounts: &ExtraValidatorAccounts) {
    let ctx = &fixture.ctx;

    let ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::AutoAddValidator {
            validator_history_account: extra_accounts.validator_history_address,
            steward_state: fixture.steward_state,
            config: fixture.steward_config.pubkey(),
            stake_pool_program: spl_stake_pool::id(),
            stake_pool: fixture.stake_pool_meta.stake_pool,
            staker: fixture.staker,
            reserve_stake: fixture.stake_pool_meta.reserve,
            withdraw_authority: extra_accounts.withdraw_authority,
            validator_list: fixture.stake_pool_meta.validator_list,
            stake_account: extra_accounts.stake_account_address,
            vote_account: extra_accounts.vote_account,
            rent: solana_sdk::sysvar::rent::id(),
            clock: solana_sdk::sysvar::clock::id(),
            stake_history: solana_sdk::sysvar::stake_history::id(),
            stake_config: stake::config::ID,
            system_program: system_program::id(),
            stake_program: stake::program::id(),
            signer: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::AutoAddValidatorToPool {}.data(),
    };
    let blockhash = ctx.borrow_mut().get_new_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        blockhash,
    );
    fixture.submit_transaction_assert_success(tx).await;
}

async fn _crank_compute_score(
    fixture: &TestFixture,
    unit_test_fixtures: &StateMachineFixtures,
    extra_validator_accounts: &Vec<ExtraValidatorAccounts>,
    indices: &[usize],
) {
    let ctx = &fixture.ctx;

    for &i in indices {
        let ix = Instruction {
            program_id: jito_steward::id(),
            accounts: jito_steward::accounts::ComputeScore {
                config: fixture.steward_config.pubkey(),
                state_account: fixture.steward_state,
                validator_list: fixture.stake_pool_meta.validator_list,
                validator_history: extra_validator_accounts[i].validator_history_address,
                cluster_history: fixture.cluster_history_account,
                signer: fixture.keypair.pubkey(),
            }
            .to_account_metas(None),
            data: jito_steward::instruction::ComputeScore {
                validator_list_index: i,
            }
            .data(),
        };
        let blockhash = ctx.borrow_mut().get_new_latest_blockhash().await.unwrap();
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&fixture.keypair.pubkey()),
            &[&fixture.keypair],
            blockhash,
        );
        fixture.submit_transaction_assert_success(tx).await;
    }
}

async fn _crank_compute_delegations(fixture: &TestFixture) {
    let ctx = &fixture.ctx;
    let ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::ComputeDelegations {
            config: fixture.steward_config.pubkey(),
            state_account: fixture.steward_state,
            signer: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::ComputeDelegations {}.data(),
    };
    let blockhash = ctx.borrow_mut().get_new_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        blockhash,
    );
    fixture.submit_transaction_assert_success(tx).await;
}

async fn _crank_idle(fixture: &TestFixture) {
    let ctx = &fixture.ctx;

    let ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::Idle {
            config: fixture.steward_config.pubkey(),
            state_account: fixture.steward_state,
            signer: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::Idle {}.data(),
    };
    let blockhash = ctx.borrow_mut().get_new_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        blockhash,
    );
    fixture.submit_transaction_assert_success(tx).await;
}

async fn _crank_compute_instant_unstake(
    fixture: &TestFixture,
    unit_test_fixtures: &StateMachineFixtures,
    extra_validator_accounts: &Vec<ExtraValidatorAccounts>,
    indices: &[usize],
) {
    let ctx = &fixture.ctx;

    for &i in indices {
        let ix = Instruction {
            program_id: jito_steward::id(),
            accounts: jito_steward::accounts::ComputeInstantUnstake {
                config: fixture.steward_config.pubkey(),
                state_account: fixture.steward_state,
                validator_history: extra_validator_accounts[i].validator_history_address,
                validator_list: fixture.stake_pool_meta.validator_list,
                cluster_history: fixture.cluster_history_account,
                signer: fixture.keypair.pubkey(),
            }
            .to_account_metas(None),
            data: jito_steward::instruction::ComputeInstantUnstake {
                validator_list_index: i,
            }
            .data(),
        };
        let blockhash = ctx.borrow_mut().get_new_latest_blockhash().await.unwrap();
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&fixture.keypair.pubkey()),
            &[&fixture.keypair],
            blockhash,
        );
        fixture.submit_transaction_assert_success(tx).await;
    }
}

async fn _crank_rebalance(
    fixture: &TestFixture,
    unit_test_fixtures: &StateMachineFixtures,
    extra_validator_accounts: &Vec<ExtraValidatorAccounts>,
    indices: &[usize],
) {
    let ctx = &fixture.ctx;

    for &i in indices {
        let extra_accounts = &extra_validator_accounts[i];

        let ix = Instruction {
            program_id: jito_steward::id(),
            accounts: jito_steward::accounts::Rebalance {
                config: fixture.steward_config.pubkey(),
                state_account: fixture.steward_state,
                validator_history: extra_accounts.validator_history_address,
                stake_pool_program: spl_stake_pool::id(),
                stake_pool: fixture.stake_pool_meta.stake_pool,
                staker: fixture.staker,
                withdraw_authority: extra_accounts.withdraw_authority,
                validator_list: fixture.stake_pool_meta.validator_list,
                reserve_stake: fixture.stake_pool_meta.reserve,
                stake_account: extra_accounts.stake_account_address,
                transient_stake_account: extra_accounts.transient_stake_account_address,
                vote_account: extra_accounts.vote_account,
                system_program: system_program::id(),
                stake_program: stake::program::id(),
                rent: solana_sdk::sysvar::rent::id(),
                clock: solana_sdk::sysvar::clock::id(),
                stake_history: solana_sdk::sysvar::stake_history::id(),
                stake_config: stake::config::ID,
                signer: fixture.keypair.pubkey(),
            }
            .to_account_metas(None),
            data: jito_steward::instruction::Rebalance {
                validator_list_index: i,
            }
            .data(),
        };
        let blockhash = ctx.borrow_mut().get_new_latest_blockhash().await.unwrap();
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&fixture.keypair.pubkey()),
            &[&fixture.keypair],
            blockhash,
        );
        fixture.submit_transaction_assert_success(tx).await;
    }
}

async fn _copy_vote_account(
    fixture: &TestFixture,
    extra_validator_accounts: &Vec<ExtraValidatorAccounts>,
    index: usize,
) {
    let ctx = &fixture.ctx;

    let ix = Instruction {
        program_id: validator_history::id(),
        accounts: validator_history::accounts::CopyVoteAccount {
            validator_history_account: extra_validator_accounts[index].validator_history_address,
            vote_account: extra_validator_accounts[index].vote_account,
            signer: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: validator_history::instruction::CopyVoteAccount {}.data(),
    };
    let blockhash = ctx.borrow_mut().get_new_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        blockhash,
    );
    fixture.submit_transaction_assert_success(tx).await;
}

async fn _update_stake_history(
    fixture: &TestFixture,
    extra_validator_accounts: &Vec<ExtraValidatorAccounts>,
    index: usize,
    epoch: u64,
    lamports: u64,
    rank: u32,
    is_superminority: bool,
) {
    let ctx = &fixture.ctx;

    let ix = Instruction {
        program_id: validator_history::id(),
        accounts: validator_history::accounts::UpdateStakeHistory {
            validator_history_account: extra_validator_accounts[index].validator_history_address,
            vote_account: extra_validator_accounts[index].vote_account,
            config: fixture.validator_history_config,
            oracle_authority: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: validator_history::instruction::UpdateStakeHistory {
            epoch,
            is_superminority,
            lamports,
            rank,
        }
        .data(),
    };
    let blockhash = ctx.borrow_mut().get_new_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        blockhash,
    );
    fixture.submit_transaction_assert_success(tx).await;
}

async fn _copy_cluster_info(fixture: &TestFixture) {
    let ctx = &fixture.ctx;

    let ix = Instruction {
        program_id: validator_history::id(),
        accounts: validator_history::accounts::CopyClusterInfo {
            cluster_history_account: fixture.cluster_history_account,
            slot_history: sysvar::slot_history::id(),
            signer: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: validator_history::instruction::CopyClusterInfo {}.data(),
    };
    let blockhash = ctx.borrow_mut().get_new_latest_blockhash().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[
            ComputeBudgetInstruction::request_heap_frame(1024 * 256),
            ComputeBudgetInstruction::set_compute_unit_limit(1_400_000),
            ix,
        ],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        blockhash,
    );
    fixture.submit_transaction_assert_success(tx).await;
}

async fn _crank_validator_history_accounts(
    fixture: &TestFixture,
    extra_validator_accounts: &Vec<ExtraValidatorAccounts>,
    indices: &[usize],
) {
    let clock: Clock = fixture
        .ctx
        .borrow_mut()
        .banks_client
        .get_sysvar()
        .await
        .unwrap();
    for &i in indices {
        fixture
            .ctx
            .borrow_mut()
            .increment_vote_account_credits(&extra_validator_accounts[i].vote_account, 1000);
        _copy_vote_account(&fixture, &extra_validator_accounts, i).await;
        // only field that's relevant to score is is_superminority
        _update_stake_history(
            &fixture,
            &extra_validator_accounts,
            i,
            clock.epoch,
            1_000_000,
            1_000,
            false,
        )
        .await;
    }
    _copy_cluster_info(&fixture).await;
}

#[tokio::test]
async fn test_cycle() {
    let mut fixture_accounts = FixtureDefaultAccounts::default();

    let unit_test_fixtures = StateMachineFixtures::default();

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
    fixture.staker = Pubkey::find_program_address(
        &[Staker::SEED, fixture.steward_config.pubkey().as_ref()],
        &jito_steward::id(),
    )
    .0;

    fixture.advance_num_epochs(20, 10).await;
    fixture.initialize_stake_pool().await;
    fixture
        .initialize_config(Some(UpdateParametersArgs {
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
        }))
        .await;
    fixture.initialize_steward_state().await;
    let steward: StewardStateAccount = fixture.load_and_deserialize(&fixture.steward_state).await;

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

    // Auto add validator - adds to validator list
    for i in 0..unit_test_fixtures.validators.len() {
        let extra_accounts = &extra_validator_accounts[i];
        _auto_add_validator(&fixture, extra_accounts).await;
    }

    _crank_epoch_maintenance(&fixture, None).await;

    _crank_compute_score(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1, 2],
    )
    .await;

    _crank_compute_delegations(&fixture).await;

    let epoch_schedule: EpochSchedule = ctx.borrow_mut().banks_client.get_sysvar().await.unwrap();
    let clock: Clock = ctx.borrow_mut().banks_client.get_sysvar().await.unwrap();

    _crank_idle(&fixture).await;

    _crank_compute_instant_unstake(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1, 2],
    )
    .await;

    _crank_rebalance(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1, 2],
    )
    .await;

    fixture.advance_num_epochs(1, 10).await;

    _crank_stake_pool(&fixture).await;

    _crank_epoch_maintenance(&fixture, None).await;

    _crank_idle(&fixture).await;

    // Advance to instant_unstake_inputs_epoch_progress
    fixture
        .advance_num_epochs(0, epoch_schedule.get_slots_in_epoch(clock.epoch) / 2 + 1)
        .await;

    // Update validator history values
    _crank_validator_history_accounts(&fixture, &extra_validator_accounts, &[0, 1, 2]).await;

    _crank_compute_instant_unstake(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1, 2],
    )
    .await;

    _crank_rebalance(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1, 2],
    )
    .await;

    fixture.advance_num_epochs(1, 10).await;

    _crank_stake_pool(&fixture).await;

    _crank_epoch_maintenance(&fixture, None).await;

    // Update validator history values
    _crank_validator_history_accounts(&fixture, &extra_validator_accounts, &[0, 1, 2]).await;

    // In new cycle
    _crank_compute_score(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1, 2],
    )
    .await;

    let clock: Clock = ctx.borrow_mut().banks_client.get_sysvar().await.unwrap();
    let state_account: StewardStateAccount =
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
    assert_eq!(state.validators_to_remove.is_empty(), true);
    assert_eq!(state.compute_delegations_completed, false.into());
    assert_eq!(state.rebalance_completed, false.into());
    assert_eq!(state.checked_validators_removed_from_list, false.into());
    // All other values are reset

    drop(fixture);
}

#[tokio::test]
async fn test_remove_validator_next_epoch() {
    /*
      Tests that a validator removed at an arbitrary point in the cycle is not included in the current cycle's consideration,
      even though it is still in the validator list, and the next epoch, it is removed from the validator list.
    */

    let mut fixture_accounts = FixtureDefaultAccounts::default();

    let unit_test_fixtures = StateMachineFixtures::default();

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
    fixture.staker = Pubkey::find_program_address(
        &[Staker::SEED, fixture.steward_config.pubkey().as_ref()],
        &jito_steward::id(),
    )
    .0;

    fixture.advance_num_epochs(20, 10).await;
    fixture.initialize_stake_pool().await;
    fixture
        .initialize_config(Some(UpdateParametersArgs {
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
        }))
        .await;
    fixture.initialize_steward_state().await;

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

    // Auto add validator - adds validators 2 and 3
    for i in 0..3 {
        let extra_accounts = &extra_validator_accounts[i];
        _auto_add_validator(&fixture, extra_accounts).await;
    }

    _crank_epoch_maintenance(&fixture, None).await;
    _crank_compute_score(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1, 2],
    )
    .await;

    _crank_compute_delegations(&fixture).await;

    _crank_idle(&fixture).await;

    _crank_compute_instant_unstake(
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
            steward_state: fixture.steward_state,
            stake_pool_program: spl_stake_pool::id(),
            stake_pool: fixture.stake_pool_meta.stake_pool,
            staker: fixture.staker,
            withdraw_authority: extra_validator_accounts[2].withdraw_authority,
            validator_list: fixture.stake_pool_meta.validator_list,
            stake_account: extra_validator_accounts[2].stake_account_address,
            transient_stake_account: extra_validator_accounts[2].transient_stake_account_address,
            clock: solana_sdk::sysvar::clock::id(),
            system_program: system_program::id(),
            stake_program: stake::program::id(),
            signer: fixture.keypair.pubkey(),
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

    let state_account: StewardStateAccount =
        fixture.load_and_deserialize(&fixture.steward_state).await;
    let state = state_account.state;
    assert!(matches!(
        state.state_tag,
        jito_steward::StewardStateEnum::ComputeInstantUnstake
    ));
    assert_eq!(state.validators_to_remove.count(), 1);
    assert_eq!(state.validators_to_remove.get(2).unwrap(), true);
    assert_eq!(state.num_pool_validators, 3);

    let validator_list: ValidatorList = fixture
        .load_and_deserialize(&fixture.stake_pool_meta.validator_list)
        .await;
    assert!(validator_list
        .validators
        .iter()
        .find(|v| v.vote_account_address == extra_validator_accounts[2].vote_account)
        .is_some());
    assert!(validator_list.validators.len() == 3);
    println!("Stake Status: {:?}", validator_list.validators[2].status);

    // Still need to crank?
    _crank_compute_instant_unstake(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[2],
    )
    .await;

    _crank_rebalance(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1, 2],
    )
    .await;

    fixture.advance_num_epochs(1, 10).await;
    _crank_stake_pool(&fixture).await;
    let validator_list: ValidatorList = fixture
        .load_and_deserialize(&fixture.stake_pool_meta.validator_list)
        .await;
    assert!(validator_list
        .validators
        .iter()
        .find(|v| v.vote_account_address == extra_validator_accounts[2].vote_account)
        .is_none());
    assert!(validator_list.validators.len() == 2);

    _crank_epoch_maintenance(&fixture, Some(&[2])).await;
    let state_account: StewardStateAccount =
        fixture.load_and_deserialize(&fixture.steward_state).await;
    let state = state_account.state;
    assert!(matches!(
        state.state_tag,
        jito_steward::StewardStateEnum::Idle
    ));
    assert_eq!(state.validators_to_remove.count(), 0);
    assert_eq!(state.num_pool_validators, 2);

    drop(fixture);
}

#[tokio::test]
async fn test_add_validator_next_cycle() {
    /*
      Tests that a validator added at an arbitrary point during the cycle does not get included in the
      current cycle's consideration, but is included in the next cycle's scoring after ComputeScores is run.
    */

    let mut fixture_accounts = FixtureDefaultAccounts::default();

    let unit_test_fixtures = StateMachineFixtures::default();

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
    fixture.staker = Pubkey::find_program_address(
        &[Staker::SEED, fixture.steward_config.pubkey().as_ref()],
        &jito_steward::id(),
    )
    .0;

    fixture.advance_num_epochs(20, 10).await;
    fixture.initialize_stake_pool().await;
    fixture
        .initialize_config(Some(UpdateParametersArgs {
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
        }))
        .await;
    fixture.initialize_steward_state().await;

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

    // Auto add validator - adds validators 2 and 3
    for i in 0..2 {
        let extra_accounts = &extra_validator_accounts[i];
        _auto_add_validator(&fixture, extra_accounts).await;
    }

    _crank_epoch_maintenance(&fixture, None).await;
    _crank_compute_score(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1],
    )
    .await;

    // Add in validator 2 at random time
    _auto_add_validator(&fixture, &extra_validator_accounts[2]).await;

    let validator_list: ValidatorList = fixture
        .load_and_deserialize(&fixture.stake_pool_meta.validator_list)
        .await;
    assert!(validator_list
        .validators
        .iter()
        .find(|v| v.vote_account_address == extra_validator_accounts[2].vote_account)
        .is_some());
    assert!(validator_list.validators.len() == 3);

    // Ensure that num_pool_validators isn't updated but validators_added is
    let state_account: StewardStateAccount =
        fixture.load_and_deserialize(&fixture.steward_state).await;
    let state = state_account.state;

    assert!(matches!(
        state.state_tag,
        jito_steward::StewardStateEnum::ComputeDelegations
    ));
    assert_eq!(state.validators_added, 1);
    assert_eq!(state.num_pool_validators, 2);

    _crank_compute_delegations(&fixture).await;
    _crank_idle(&fixture).await;
    _crank_compute_instant_unstake(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1],
    )
    .await;
    _crank_rebalance(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0, 1],
    )
    .await;

    fixture.advance_num_epochs(1, 10).await;

    _crank_stake_pool(&fixture).await;
    _crank_epoch_maintenance(&fixture, None).await;

    let state_account: StewardStateAccount =
        fixture.load_and_deserialize(&fixture.steward_state).await;
    let state = state_account.state;

    assert!(matches!(
        state.state_tag,
        jito_steward::StewardStateEnum::Idle
    ));
    assert_eq!(state.validators_added, 1);
    assert_eq!(state.num_pool_validators, 2);

    _crank_validator_history_accounts(&fixture, &extra_validator_accounts, &[0, 1, 2]).await;

    // Ensure we're in the next cycle
    _crank_compute_score(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[0],
    )
    .await;

    // Ensure that num_pool_validators is updated and validators_added is reset
    let state_account: StewardStateAccount =
        fixture.load_and_deserialize(&fixture.steward_state).await;
    let state = state_account.state;

    assert!(matches!(
        state.state_tag,
        jito_steward::StewardStateEnum::ComputeScores
    ));

    assert_eq!(state.validators_added, 0);
    assert_eq!(state.validators_to_remove.is_empty(), true);
    assert_eq!(state.num_pool_validators, 3);

    // Ensure we can crank the new validator
    _crank_compute_score(
        &fixture,
        &unit_test_fixtures,
        &extra_validator_accounts,
        &[2],
    )
    .await;
}
