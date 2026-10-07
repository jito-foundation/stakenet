#[cfg(test)]
mod tests {
    #![allow(clippy::await_holding_refcell_ref)]
    //! Scoring and instant unstake on validator history from after the alpenglow migration, where vote
    //! accounts earn vote reward lamports instead of vote credits.
    use std::collections::HashMap;

    use anchor_lang::{
        solana_program::{instruction::Instruction, pubkey::Pubkey},
        InstructionData, ToAccountMetas,
    };
    use jito_steward::{StewardStateAccount, StewardStateAccountV2, UpdateParametersArgs};
    use solana_program_test::*;
    use solana_sdk::{
        compute_budget::ComputeBudgetInstruction, signature::Keypair, signer::Signer,
        transaction::Transaction,
    };
    use tests::steward_fixtures::{
        auto_add_validator, crank_compute_delegations, crank_copy_directed_stake_targets,
        crank_directed_stake_permissions, crank_epoch_maintenance, crank_idle,
        crank_rebalance_directed, initialize_directed_stake_meta,
        serialized_validator_history_account, ExtraValidatorAccounts, FixtureDefaultAccounts,
        StateMachineFixtures, TestFixture, ValidatorEntry,
    };
    use validator_history::{ClusterHistory, ValidatorHistory};

    // 86_400 SOL of inflation per epoch, against which a validator holding 0.1% of the stake is
    // expected to earn 0.1% — vote and leader rewards together
    const REWARD_STAKE: u64 = 1_000_000_000_000;
    const TOTAL_REWARD_STAKE: u64 = 1_000 * REWARD_STAKE;
    const INFLATION_REWARDS: u64 = 86_400_000_000_000;
    const EXPECTED_LAMPORTS: u64 = INFLATION_REWARDS / 1_000;
    const TOTAL_BLOCKS: u64 = 1_000;

    /// The keeper packs 5 `compute_score` instructions into a 1.4M compute unit transaction
    const COMPUTE_UNITS_PER_INSTRUCTION: u64 = 1_400_000 / 5;

    /// Turns every epoch after the first into an alpenglow epoch in which the validator captured
    /// `votes(epoch)` out of `TOTAL_BLOCKS` of the rewards it was expected to earn. `votes` equal to
    /// `TOTAL_BLOCKS` is a flawless epoch; scoring sees no split between vote and leader rewards.
    fn to_alpenglow(validator: &mut ValidatorHistory, votes: impl Fn(u16) -> u64) {
        for entry in validator
            .history
            .arr_mut()
            .iter_mut()
            .filter(|entry| entry.epoch <= 20)
        {
            entry.epoch_stake_lamports = REWARD_STAKE;
            if entry.epoch > 0 {
                entry.reward_lamports = EXPECTED_LAMPORTS * votes(entry.epoch) / TOTAL_BLOCKS;
                entry.epoch_credits = entry.reward_lamports.min(u64::from(u32::MAX - 1)) as u32;
            }
        }
    }

    fn to_alpenglow_cluster(cluster: &mut ClusterHistory) {
        for entry in cluster
            .history
            .arr_mut()
            .iter_mut()
            .filter(|entry| entry.epoch <= 20)
        {
            entry.total_epoch_stake_lamports = TOTAL_REWARD_STAKE;
            entry.total_inflation_rewards = INFLATION_REWARDS;
            entry.is_alpenglow = (entry.epoch > 0) as u8;
        }
    }

    async fn simulate_and_submit(fixture: &TestFixture, instruction: Instruction) -> u64 {
        let blockhash = fixture
            .ctx
            .borrow_mut()
            .get_new_latest_blockhash()
            .await
            .unwrap();
        // Same compute budget as the keeper, and no extra heap
        let transaction = Transaction::new_signed_with_payer(
            &[
                ComputeBudgetInstruction::set_compute_unit_limit(1_400_000),
                instruction,
            ],
            Some(&fixture.keypair.pubkey()),
            &[&fixture.keypair],
            blockhash,
        );
        let simulation = fixture
            .ctx
            .borrow_mut()
            .banks_client
            .simulate_transaction(transaction.clone())
            .await
            .unwrap();
        let units_consumed = simulation.simulation_details.unwrap().units_consumed;
        fixture.submit_transaction_assert_success(transaction).await;
        units_consumed
    }

    #[tokio::test]
    async fn test_alpenglow_scoring_and_instant_unstake() {
        let mut fixture_accounts = FixtureDefaultAccounts::default();
        let mut unit_test_fixtures = Box::<StateMachineFixtures>::default();

        // Validator 0 earns everything it was expected to, validator 1 only 20% of it (and fails
        // the commission filters anyway), and validator 2 only half of it in epoch 10
        to_alpenglow(&mut unit_test_fixtures.validators[0], |_| 1_000);
        to_alpenglow(&mut unit_test_fixtures.validators[1], |_| 200);
        to_alpenglow(&mut unit_test_fixtures.validators[2], |epoch| {
            if epoch == 10 {
                500
            } else {
                1_000
            }
        });
        to_alpenglow_cluster(&mut unit_test_fixtures.cluster_history);

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
                    instant_unstake_epoch_progress: Some(0.90),
                    compute_score_slot_range: Some(1000),
                    instant_unstake_inputs_epoch_progress: Some(0.50),
                    num_epochs_between_scoring: Some(2),
                    minimum_stake_lamports: Some(5_000_000_000),
                    minimum_voting_epochs: Some(0),
                    compute_score_epoch_progress: Some(0.50),
                    undirected_stake_ceiling_lamports: Some(0),
                    directed_stake_unstake_cap_bps: Some(10_000),
                    jito_bam_minimum_epochs: Some(0),
                    jito_bam_window_epochs: Some(0),
                    alpenglow_migration_epoch: None,
                }),
                None,
            )
            .await;
        initialize_directed_stake_meta(&fixture).await;
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
        let blockhash = fixture
            .ctx
            .borrow_mut()
            .get_new_latest_blockhash()
            .await
            .unwrap();
        let tx = Transaction::new_signed_with_payer(
            &[set_meta_auth_ix],
            Some(&fixture.keypair.pubkey()),
            &[&fixture.keypair],
            blockhash,
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

        fixture.advance_num_slots(250_000).await;
        crank_idle(&fixture).await;

        for (i, extra_accounts) in extra_validator_accounts.iter().enumerate() {
            let compute_score_ix = Instruction {
                program_id: jito_steward::id(),
                accounts: jito_steward::accounts::ComputeScore {
                    config: fixture.steward_config.pubkey(),
                    state_account: fixture.steward_state,
                    validator_list: fixture.stake_pool_meta.validator_list,
                    validator_history: extra_accounts.validator_history_address,
                    cluster_history: fixture.cluster_history_account,
                }
                .to_account_metas(None),
                data: jito_steward::instruction::ComputeScore {
                    validator_list_index: i as u64,
                }
                .data(),
            };
            let units_consumed = simulate_and_submit(&fixture, compute_score_ix).await;
            println!("compute_score validator={i} units_consumed={units_consumed}");
            assert!(units_consumed < COMPUTE_UNITS_PER_INSTRUCTION);
        }

        let steward: StewardStateAccountV2 =
            fixture.load_and_deserialize(&fixture.steward_state).await;
        assert!(steward.state.scores[0] > 0);
        assert_eq!(steward.state.scores[1], 0);
        // Delinquent in alpenglow epoch 10
        assert_eq!(steward.state.scores[2], 0);
        assert!(steward.state.raw_scores[2] > 0);

        crank_compute_delegations(&fixture).await;
        fixture.advance_num_slots(160_000).await;
        crank_idle(&fixture).await;

        // Validator 0 earns only half of what it was expected to in epoch 19, the last complete
        // epoch, which is what instant unstake judges once the cluster is on alpenglow
        let mut validator = unit_test_fixtures.validators[0];
        to_alpenglow(
            &mut validator,
            |epoch| if epoch == 19 { 500 } else { 1_000 },
        );
        fixture.ctx.borrow_mut().set_account(
            &extra_validator_accounts[0].validator_history_address,
            &serialized_validator_history_account(validator).into(),
        );

        for (i, extra_accounts) in extra_validator_accounts.iter().enumerate() {
            let compute_instant_unstake_ix = Instruction {
                program_id: jito_steward::id(),
                accounts: jito_steward::accounts::ComputeInstantUnstake {
                    config: fixture.steward_config.pubkey(),
                    state_account: fixture.steward_state,
                    validator_history: extra_accounts.validator_history_address,
                    validator_list: fixture.stake_pool_meta.validator_list,
                    cluster_history: fixture.cluster_history_account,
                }
                .to_account_metas(None),
                data: jito_steward::instruction::ComputeInstantUnstake {
                    validator_list_index: i as u64,
                }
                .data(),
            };
            let units_consumed = simulate_and_submit(&fixture, compute_instant_unstake_ix).await;
            println!("compute_instant_unstake validator={i} units_consumed={units_consumed}");
        }

        let steward: StewardStateAccountV2 =
            fixture.load_and_deserialize(&fixture.steward_state).await;
        assert!(steward.state.instant_unstake.get(0).unwrap());
        assert!(!steward.state.instant_unstake.get(2).unwrap());
    }
}
