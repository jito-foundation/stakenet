#![allow(clippy::await_holding_refcell_ref)]
use crate::{
    spl_stake_pool_cli,
    stake_pool_utils::{serialized_stake_pool_account, serialized_validator_list_account},
};
use anchor_lang::AccountDeserialize;
use anchor_lang::{
    prelude::SolanaSysvar,
    solana_program::{
        clock::Clock,
        pubkey::Pubkey,
        vote::state::{VoteInit, VoteState, VoteStateVersions},
    },
    AccountSerialize, AnchorSerialize, Discriminator, InstructionData, ToAccountMetas,
};
use borsh1::BorshSerialize;
use jito_steward::state::directed_stake::DirectedStakeMeta;
use jito_steward::EPOCH_MAINTENANCE;
use jito_steward::{
    bitmask::BitMask,
    constants::{MAX_VALIDATORS, SORTED_INDEX_DEFAULT, STAKE_POOL_WITHDRAW_SEED},
    instructions::AuthorityType,
    stake_pool_utils::{StakePool, ValidatorList},
    utils::get_validator_list_length,
    Config, Delegation, LargeBitMask, Parameters, StewardStateAccount, StewardStateAccountV2,
    StewardStateEnum, StewardStateV2, UpdateParametersArgs, UpdatePriorityFeeParametersArgs,
};
use solana_program_test::*;
#[allow(deprecated)]
use solana_sdk::{
    account::Account,
    compute_budget::ComputeBudgetInstruction,
    epoch_schedule::EpochSchedule,
    hash::Hash,
    instruction::Instruction,
    native_token::LAMPORTS_PER_SOL,
    rent::Rent,
    signature::Keypair,
    signer::Signer,
    stake::{
        self,
        state::{Lockup, StakeStateV2},
    },
    system_program, sysvar,
    transaction::Transaction,
};
use spl_stake_pool::{
    find_stake_program_address, find_transient_stake_program_address,
    find_withdraw_authority_program_address, minimum_delegation,
    state::{
        AccountType, Fee, FutureEpoch, StakeStatus, ValidatorList as SPLValidatorList,
        ValidatorStakeInfo,
    },
};
use std::{cell::RefCell, collections::HashMap, rc::Rc, str::FromStr, vec};
use validator_history::{
    self,
    constants::{MAX_ALLOC_BYTES, TVC_MULTIPLIER},
    CircBuf, CircBufCluster, ClusterHistory, ClusterHistoryEntry, MerkleRootUploadAuthority,
    ValidatorHistory, ValidatorHistoryEntry,
};

pub struct StakePoolMetadata {
    pub stake_pool_keypair: Keypair,
    pub stake_pool: Pubkey,
    pub validator_list_keypair: Keypair,
    pub validator_list: Pubkey,
    pub reserve_keypair: Keypair,
    pub reserve: Pubkey,
}

impl Default for StakePoolMetadata {
    fn default() -> Self {
        let stake_pool_keypair = Keypair::new();
        let stake_pool = stake_pool_keypair.pubkey();
        let validator_list_keypair = Keypair::new();
        let validator_list = validator_list_keypair.pubkey();
        let reserve_keypair = Keypair::new();
        let reserve = reserve_keypair.pubkey();

        Self {
            stake_pool_keypair,
            stake_pool,
            validator_list_keypair,
            validator_list,
            reserve_keypair,
            reserve,
        }
    }
}

pub struct TestFixture {
    pub ctx: Rc<RefCell<ProgramTestContext>>,
    pub stake_pool_meta: StakePoolMetadata,
    pub steward_config: Keypair,
    pub steward_state: Pubkey,
    pub cluster_history_account: Pubkey,
    pub validator_history_config: Pubkey,
    pub keypair: Keypair,
    pub directed_stake_meta: Pubkey,
}

impl TestFixture {
    /// Initializes test context with Steward and Stake Pool programs loaded, as well as
    /// a vote account and a system account for signing transactions.
    ///
    /// Returns a fixture with relevant account addresses and keypairs.
    pub async fn new() -> Self {
        let mut program = ProgramTest::new("jito_steward", jito_steward::ID, None);
        program.add_program("spl_stake_pool", spl_stake_pool::id(), None);
        program.set_compute_max_units(1_400_000);

        let steward_config = Keypair::new();
        let directed_stake_meta = Pubkey::find_program_address(
            &[DirectedStakeMeta::SEED, steward_config.pubkey().as_ref()],
            &jito_steward::id(),
        )
        .0;

        let stake_pool_meta = StakePoolMetadata::default();
        let steward_state = Pubkey::find_program_address(
            &[StewardStateAccount::SEED, steward_config.pubkey().as_ref()],
            &jito_steward::id(),
        )
        .0;
        let cluster_history_account =
            Pubkey::find_program_address(&[ClusterHistory::SEED], &validator_history::id()).0;

        let (validator_history_config, vhc_bump) = Pubkey::find_program_address(
            &[validator_history::state::Config::SEED],
            &validator_history::id(),
        );
        let keypair = Keypair::new();

        program.add_account(keypair.pubkey(), system_account(100_000_000_000));
        program.add_account(steward_config.pubkey(), system_account(100_000_000_000));

        program.add_account(
            validator_history_config,
            validator_history_config_account(vhc_bump, 1),
        );

        program.deactivate_feature(
            Pubkey::from_str("9onWzzvCzNC2jfhxxeqRgs5q7nFAAKpCUvkj6T6GJK9i").unwrap(),
        );

        let ctx = Rc::new(RefCell::new(program.start_with_context().await));

        Self {
            ctx,
            stake_pool_meta,
            steward_state,
            steward_config,
            validator_history_config,
            cluster_history_account,
            keypair,
            directed_stake_meta,
        }
    }

    /// Initializes test context with Steward and Stake Pool programs loaded, as well as
    /// a vote account and a system account for signing transactions.
    ///
    /// Returns a fixture with relevant account addresses and keypairs.
    pub async fn new_with_mainnet_mock_data(
        accounts_fixture: FixtureDefaultAccounts,
        additional_accounts: HashMap<Pubkey, Account>,
        last_update_epoch: u64,
    ) -> Self {
        let mut program = ProgramTest::new("jito_steward", jito_steward::ID, None);
        program.add_program("validator_history", validator_history::id(), None);
        program.add_program("spl_stake_pool", spl_stake_pool::id(), None);

        let validator_list_pubkey = accounts_fixture.stake_pool_meta.validator_list;
        let validator_list_account_data =
            std::fs::read("tests/fixtures/accounts/validator_list.bin").unwrap();
        let validator_list_account = Account {
            lamports: 5081753568,
            data: validator_list_account_data.clone(),
            owner: Pubkey::from_str_const("SPoo1Ku8WFXoNDMHPsrGSTSG1Y47rzgn41SLUNakuHy"),
            executable: false,
            rent_epoch: 18446744073709551615,
        };

        // Get the validator list count using the program's utility function
        let mut validator_list_data = validator_list_account_data.clone();
        let mut lamports = validator_list_account.lamports;
        let validator_list_account_info = anchor_lang::solana_program::account_info::AccountInfo {
            key: &validator_list_pubkey,
            is_signer: false,
            is_writable: false,
            lamports: std::rc::Rc::new(std::cell::RefCell::new(&mut lamports)),
            data: std::rc::Rc::new(std::cell::RefCell::new(&mut validator_list_data)),
            owner: &validator_list_account.owner,
            executable: false,
            rent_epoch: validator_list_account.rent_epoch,
        };
        let validator_list_count = get_validator_list_length(&validator_list_account_info)
            .expect("Failed to get validator list length");
        program.add_account(validator_list_pubkey, validator_list_account);

        let stake_pool_pubkey = accounts_fixture.stake_pool_meta.stake_pool;
        let stake_pool_account_data =
            std::fs::read("tests/fixtures/accounts/stake_pool.bin").unwrap();
        let stake_pool_deserialize =
            StakePool::try_deserialize(&mut stake_pool_account_data.as_slice()).unwrap();
        let mut spl_stake_pool = stake_pool_deserialize.as_ref().clone();
        spl_stake_pool.validator_list = validator_list_pubkey;
        spl_stake_pool.reserve_stake = accounts_fixture.stake_pool_meta.reserve;
        spl_stake_pool.last_update_epoch = last_update_epoch;

        // Create and set up the reserve stake account with 1 million SOL
        let reserve_stake_address = accounts_fixture.stake_pool_meta.reserve;
        let reserve_lamports = 1_000_000 * 1_000_000_000; // 1 million SOL

        // Get the withdraw authority for the stake pool (this will be the authorized staker/withdrawer)
        let (withdraw_authority, _) =
            find_withdraw_authority_program_address(&spl_stake_pool::id(), &stake_pool_pubkey);

        // Create an initialized but undelegated stake account
        // For an initialized stake account, we use the Initialized variant with Meta
        let stake_meta = stake::state::Meta {
            rent_exempt_reserve: 0, // Will be set by rent calculation
            authorized: stake::state::Authorized {
                staker: Pubkey::from_str_const("9BAmGVLGxzqct6bkgjWmKSv3BFB6iKYXNBQp8GWG1LDY"),
                withdrawer: withdraw_authority,
            },
            lockup: Lockup::default(),
        };
        let stake_state = StakeStateV2::Initialized(stake_meta);

        let reserve_stake_account = serialized_stake_account(stake_state, reserve_lamports);
        program.add_account(reserve_stake_address, reserve_stake_account);

        // Fix the withdraw authority bump seed to match what the program expects (STAKE_POOL_WITHDRAW_SEED = b"withdraw")
        let (_, correct_bump_seed) = Pubkey::find_program_address(
            &[stake_pool_pubkey.as_ref(), STAKE_POOL_WITHDRAW_SEED],
            &spl_stake_pool::id(),
        );
        spl_stake_pool.stake_withdraw_bump_seed = correct_bump_seed;
        spl_stake_pool.staker =
            Pubkey::from_str_const("9BAmGVLGxzqct6bkgjWmKSv3BFB6iKYXNBQp8GWG1LDY");
        let mut stake_pool_account_data = vec![];
        BorshSerialize::serialize(&spl_stake_pool, &mut stake_pool_account_data).unwrap();
        let stake_pool_account = Account {
            lamports: 2060816388,
            data: stake_pool_account_data.clone(),
            owner: Pubkey::from_str_const("SPoo1Ku8WFXoNDMHPsrGSTSG1Y47rzgn41SLUNakuHy"),
            executable: false,
            rent_epoch: 18446744073709551615,
        };
        program.add_account(stake_pool_pubkey, stake_pool_account);

        let steward_config_address = accounts_fixture.steward_config_keypair.pubkey();
        let steward_config_account_data =
            std::fs::read("tests/fixtures/accounts/steward_config.bin").unwrap();
        let steward_config_account = Account {
            lamports: 1000000000000,
            data: steward_config_account_data.clone(),
            owner: jito_steward::id(),
            executable: false,
            rent_epoch: 18446744073709551615,
        };
        program.add_account(steward_config_address, steward_config_account);

        let steward_state_address = Pubkey::find_program_address(
            &[StewardStateAccount::SEED, steward_config_address.as_ref()],
            &jito_steward::id(),
        )
        .0;
        let steward_state_account_data =
            std::fs::read("tests/fixtures/accounts/steward_state.bin").unwrap();
        let mut steward_state_deserialize =
            StewardStateAccountV2::try_deserialize(&mut steward_state_account_data.as_slice())
                .unwrap();
        steward_state_deserialize.state.clear_flags();
        steward_state_deserialize.state.set_flag(EPOCH_MAINTENANCE);
        steward_state_deserialize.state.state_tag = StewardStateEnum::RebalanceDirected;
        // Set current_epoch to match what the clock epoch will be after the test advances
        // The test calls advance_num_epochs(20, 10), so if clock starts at epoch 0, it will be at epoch 20
        steward_state_deserialize.state.current_epoch = last_update_epoch;
        // Set num_pool_validators and validators_added to match the validator list count
        // The check requires: num_pool_validators + validators_added == validators_in_list
        steward_state_deserialize.state.num_pool_validators = validator_list_count as u64;
        steward_state_deserialize.state.validators_added = 0;
        steward_state_deserialize.state.start_computing_scores_slot = 10;
        // Verify the equation will work: num_pool_validators + validators_added should equal validator_list_count
        assert_eq!(
                steward_state_deserialize.state.num_pool_validators as usize + steward_state_deserialize.state.validators_added as usize,
                validator_list_count,
                "num_pool_validators ({}) + validators_added ({}) should equal validator_list_count ({})",
                steward_state_deserialize.state.num_pool_validators,
                steward_state_deserialize.state.validators_added,
                validator_list_count
            );
        let mut steward_state_account_data = Vec::with_capacity(StewardStateAccountV2::SIZE);
        steward_state_account_data.extend_from_slice(StewardStateAccountV2::DISCRIMINATOR);
        steward_state_account_data
            .extend_from_slice(bytemuck::bytes_of(&steward_state_deserialize));

        // Verify the serialization worked by deserializing back
        let verify_deserialize =
            StewardStateAccountV2::try_deserialize(&mut steward_state_account_data.as_slice())
                .unwrap();
        assert_eq!(
            verify_deserialize.state.current_epoch, last_update_epoch,
            "current_epoch should be {} after serialization, but got {}",
            last_update_epoch, verify_deserialize.state.current_epoch
        );
        let steward_state_account = Account {
            lamports: 1000000000000,
            data: steward_state_account_data.clone(),
            owner: jito_steward::id(),
            executable: false,
            rent_epoch: 18446744073709551615,
        };
        program.add_account(steward_state_address, steward_state_account);

        for (key, account) in accounts_fixture.to_accounts_vec() {
            // Skip keys that are overriden by additional_accounts or the steward_state_address
            // (we already set it up with the correct current_epoch, num_pool_validators, and other fields above)
            // to_accounts_vec() would overwrite it with default values from the fixture
            if !additional_accounts.contains_key(&key) && key != steward_state_address {
                program.add_account(key, account);
            }
        }
        for (key, account) in additional_accounts {
            program.add_account(key, account);
        }

        program.deactivate_feature(
            Pubkey::from_str("9onWzzvCzNC2jfhxxeqRgs5q7nFAAKpCUvkj6T6GJK9i").unwrap(),
        );
        let ctx = Rc::new(RefCell::new(program.start_with_context().await));

        let steward_config_address = accounts_fixture.steward_config_keypair.pubkey();

        Self {
            ctx,
            stake_pool_meta: accounts_fixture.stake_pool_meta,
            directed_stake_meta: accounts_fixture.directed_stake_meta,
            steward_config: accounts_fixture.steward_config_keypair,
            steward_state: Pubkey::find_program_address(
                &[StewardStateAccount::SEED, steward_config_address.as_ref()],
                &jito_steward::id(),
            )
            .0,
            cluster_history_account: Pubkey::find_program_address(
                &[ClusterHistory::SEED],
                &validator_history::id(),
            )
            .0,
            validator_history_config: Pubkey::find_program_address(
                &[validator_history::state::Config::SEED],
                &validator_history::id(),
            )
            .0,
            keypair: accounts_fixture.keypair,
        }
    }

    pub async fn new_from_accounts(
        accounts_fixture: FixtureDefaultAccounts,
        additional_accounts: HashMap<Pubkey, Account>,
    ) -> Self {
        let mut program = ProgramTest::new("jito_steward", jito_steward::ID, None);
        program.add_program("validator_history", validator_history::id(), None);
        program.add_program("spl_stake_pool", spl_stake_pool::id(), None);

        for (key, account) in accounts_fixture.to_accounts_vec() {
            // Skip keys that are overriden by additional_accounts
            if !additional_accounts.contains_key(&key) {
                program.add_account(key, account);
            }
        }
        for (key, account) in additional_accounts {
            program.add_account(key, account);
        }

        program.deactivate_feature(
            Pubkey::from_str("9onWzzvCzNC2jfhxxeqRgs5q7nFAAKpCUvkj6T6GJK9i").unwrap(),
        );
        let ctx = Rc::new(RefCell::new(program.start_with_context().await));

        let steward_config_address = accounts_fixture.steward_config_keypair.pubkey();

        Self {
            ctx,
            stake_pool_meta: accounts_fixture.stake_pool_meta,
            directed_stake_meta: accounts_fixture.directed_stake_meta,
            steward_config: accounts_fixture.steward_config_keypair,
            steward_state: Pubkey::find_program_address(
                &[StewardStateAccount::SEED, steward_config_address.as_ref()],
                &jito_steward::id(),
            )
            .0,
            cluster_history_account: Pubkey::find_program_address(
                &[ClusterHistory::SEED],
                &validator_history::id(),
            )
            .0,
            validator_history_config: Pubkey::find_program_address(
                &[validator_history::state::Config::SEED],
                &validator_history::id(),
            )
            .0,
            keypair: accounts_fixture.keypair,
        }
    }

    pub async fn load_and_deserialize<T: anchor_lang::AccountDeserialize>(
        &self,
        address: &Pubkey,
    ) -> T {
        let ai = {
            let banks_client = self.ctx.borrow_mut().banks_client.clone();
            banks_client.get_account(*address).await.unwrap().unwrap()
        };

        T::try_deserialize(&mut ai.data.as_slice()).unwrap()
    }

    pub async fn get_sysvar<T: SolanaSysvar>(&self) -> T {
        let sysvar = {
            let banks_client = self.ctx.borrow_mut().banks_client.clone();
            banks_client.get_sysvar().await.unwrap()
        };

        sysvar
    }

    pub async fn get_account(&self, address: &Pubkey) -> Account {
        let account = {
            let banks_client = self.ctx.borrow_mut().banks_client.clone();
            banks_client.get_account(*address).await.unwrap().unwrap()
        };

        account
    }

    pub async fn simulate_stake_pool_update(&self) {
        let stake_pool: StakePool = self
            .load_and_deserialize(&self.stake_pool_meta.stake_pool)
            .await;

        let mut stake_pool_spl = stake_pool.as_ref().clone();

        let current_epoch = self
            .ctx
            .borrow_mut()
            .banks_client
            .get_sysvar::<Clock>()
            .await
            .unwrap()
            .epoch;

        stake_pool_spl.last_update_epoch = current_epoch;

        self.ctx.borrow_mut().set_account(
            &self.stake_pool_meta.stake_pool,
            &serialized_stake_pool_account(stake_pool_spl, std::mem::size_of::<StakePool>()).into(),
        );
    }

    pub async fn initialize_stake_pool(&self) {
        // Call command_create_pool and execute transactions responded
        let mint = Keypair::new();

        let cli_config = spl_stake_pool_cli::CliConfig {
            manager: self.keypair.insecure_clone(),
            staker: self.keypair.insecure_clone(),
            funding_authority: None,
            token_owner: self.keypair.insecure_clone(),
            fee_payer: self.keypair.insecure_clone(),
            dry_run: false,
            no_update: false,
        };
        let epoch_fee = Fee {
            numerator: 1,
            denominator: 100,
        };
        let withdrawal_fee = Fee {
            numerator: 1,
            denominator: 100,
        };
        let deposit_fee = Fee {
            numerator: 1,
            denominator: 100,
        };
        let transactions_and_signers = spl_stake_pool_cli::command_create_pool(
            &cli_config,
            None,
            epoch_fee,
            withdrawal_fee,
            deposit_fee,
            0,
            MAX_VALIDATORS as u32,
            self.stake_pool_meta.stake_pool_keypair.insecure_clone(),
            self.stake_pool_meta.validator_list_keypair.insecure_clone(),
            mint,
            self.stake_pool_meta.reserve_keypair.insecure_clone(),
            true,
            spl_stake_pool::id(),
        )
        .expect("failed to create pool initialization instructions");

        for (instructions, signers) in transactions_and_signers {
            let signers = signers.iter().map(|s| s as &dyn Signer).collect::<Vec<_>>();
            let transaction = Transaction::new_signed_with_payer(
                &instructions,
                Some(&self.keypair.pubkey()),
                &signers,
                self.ctx.borrow().last_blockhash,
            );
            self.submit_transaction_assert_success(transaction).await;
        }
    }

    pub async fn initialize_steward(
        &self,
        parameters: Option<UpdateParametersArgs>,
        priority_fee_parameters: Option<UpdatePriorityFeeParametersArgs>,
    ) {
        // Initialize V1 first, realloc to full size, then migrate to V2
        self.initialize_steward_v1(parameters, priority_fee_parameters)
            .await;
        self.realloc_steward_state().await;

        // Set state to Idle before migration to avoid InvalidState error
        // We need to directly modify the account data to avoid excessive I/O
        const DISCRIMINATOR_LEN: usize = 8;
        const STATE_TAG_OFFSET: usize = DISCRIMINATOR_LEN;
        {
            let mut account = self.get_account(&self.steward_state).await;
            // Set state_tag to Idle (which is 2)
            account.data[STATE_TAG_OFFSET] = 2;

            self.ctx
                .borrow_mut()
                .set_account(&self.steward_state, &account.into());
        }

        // Migrate
        self.migrate_steward_state_to_v2().await;

        // Set state back to ComputeScores after migration
        {
            let mut account = self.get_account(&self.steward_state).await;
            // Set state_tag to ComputeScores (which is 0)
            account.data[STATE_TAG_OFFSET] = 0;
            self.ctx
                .borrow_mut()
                .set_account(&self.steward_state, &account.into());
        }
    }

    pub async fn initialize_steward_v1(
        &self,
        parameters: Option<UpdateParametersArgs>,
        priority_fee_parameters: Option<UpdatePriorityFeeParametersArgs>,
    ) {
        // Default parameters from JIP
        let update_parameters_args = parameters.unwrap_or(UpdateParametersArgs {
            mev_commission_range: Some(0),
            epoch_credits_range: Some(0),
            commission_range: Some(0),
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
            num_epochs_between_scoring: Some(10),
            minimum_stake_lamports: Some(5_000_000_000),
            minimum_voting_epochs: Some(0),
            compute_score_epoch_progress: Some(0.50),
            undirected_stake_floor_lamports: Some(10_000_000_000 * 1_000_000_000),
            directed_stake_unstake_cap_bps: Some(10_000),
        });

        let update_priority_fee_parameters_args =
            priority_fee_parameters.unwrap_or(UpdatePriorityFeeParametersArgs {
                priority_fee_lookback_epochs: Some(10),
                priority_fee_lookback_offset: Some(2),
                priority_fee_max_commission_bps: Some(5_000),
                priority_fee_error_margin_bps: Some(10),
                priority_fee_scoring_start_epoch: Some(0),
            });

        let instruction = Instruction {
            program_id: jito_steward::id(),
            accounts: jito_steward::accounts::InitializeSteward {
                config: self.steward_config.pubkey(),
                stake_pool: self.stake_pool_meta.stake_pool,
                state_account: self.steward_state,
                stake_pool_program: spl_stake_pool::id(),
                system_program: anchor_lang::solana_program::system_program::id(),
                current_staker: self.keypair.pubkey(),
            }
            .to_account_metas(None),
            data: jito_steward::instruction::InitializeSteward {
                update_parameters_args,
                update_priority_fee_parameters_args,
            }
            .data(),
        };

        let transaction = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&self.keypair.pubkey()),
            &[&self.keypair, &self.steward_config],
            self.ctx.borrow().last_blockhash,
        );
        self.submit_transaction_assert_success(transaction).await;
    }

    pub async fn migrate_steward_state_to_v2(&self) {
        let instructions = vec![
            ComputeBudgetInstruction::set_compute_unit_limit(1_400_000),
            Instruction {
                program_id: jito_steward::id(),
                accounts: jito_steward::accounts::MigrateStateToV2 {
                    state_account: self.steward_state,
                    config: self.steward_config.pubkey(),
                }
                .to_account_metas(None),
                data: jito_steward::instruction::MigrateStateToV2 {}.data(),
            },
        ];

        let transaction = Transaction::new_signed_with_payer(
            &instructions,
            Some(&self.keypair.pubkey()),
            &[&self.keypair],
            self.ctx.borrow().last_blockhash,
        );
        self.submit_transaction_assert_success(transaction).await;
    }

    pub async fn try_migrate_steward_state_to_v2(&self) -> Result<(), BanksClientError> {
        let instructions = vec![
            ComputeBudgetInstruction::set_compute_unit_limit(1_400_000),
            Instruction {
                program_id: jito_steward::id(),
                accounts: jito_steward::accounts::MigrateStateToV2 {
                    state_account: self.steward_state,
                    config: self.steward_config.pubkey(),
                }
                .to_account_metas(None),
                data: jito_steward::instruction::MigrateStateToV2 {}.data(),
            },
        ];

        let transaction = Transaction::new_signed_with_payer(
            &instructions,
            Some(&self.keypair.pubkey()),
            &[&self.keypair],
            self.ctx.borrow().last_blockhash,
        );

        let ctx = self.ctx.borrow_mut();
        ctx.banks_client
            .process_transaction_with_preflight(transaction)
            .await
    }

    pub async fn set_new_authority(&self, authority_type: AuthorityType) -> Keypair {
        let new_authority = Keypair::new();
        self.ctx
            .borrow_mut()
            .set_account(&new_authority.pubkey(), &system_account(1_000_000).into());

        let ix = Instruction {
            program_id: jito_steward::id(),
            accounts: jito_steward::accounts::SetNewAuthority {
                config: self.steward_config.pubkey(),
                new_authority: new_authority.pubkey(),
                admin: self.keypair.pubkey(),
            }
            .to_account_metas(None),
            data: jito_steward::instruction::SetNewAuthority { authority_type }.data(),
        };
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&self.keypair.pubkey()),
            &[&self.keypair],
            self.ctx.borrow().last_blockhash,
        );

        self.submit_transaction_assert_success(tx).await;

        new_authority
    }

    pub async fn get_latest_blockhash(&self) -> Hash {
        let blockhash = {
            let mut banks_client = self.ctx.borrow_mut().banks_client.clone();
            banks_client
                .get_new_latest_blockhash(&Hash::default())
                .await
                .unwrap()
        };

        blockhash
    }

    pub async fn realloc_steward_state(&self) {
        // Realloc validator history account
        let num_reallocs = (StewardStateAccount::SIZE - MAX_ALLOC_BYTES) / MAX_ALLOC_BYTES + 1;

        // Do one realloc per transaction to avoid exceeding compute budget
        for _ in 0..num_reallocs {
            let blockhash = self
                .ctx
                .borrow_mut()
                .get_new_latest_blockhash()
                .await
                .unwrap();

            let ixs = vec![
                ComputeBudgetInstruction::set_compute_unit_limit(1_400_000),
                Instruction {
                    program_id: jito_steward::id(),
                    accounts: jito_steward::accounts::ReallocState {
                        state_account: self.steward_state,
                        config: self.steward_config.pubkey(),
                        validator_list: self.stake_pool_meta.validator_list,
                        system_program: anchor_lang::solana_program::system_program::id(),
                        signer: self.keypair.pubkey(),
                    }
                    .to_account_metas(None),
                    data: jito_steward::instruction::ReallocState {}.data(),
                },
            ];

            let transaction = Transaction::new_signed_with_payer(
                &ixs,
                Some(&self.keypair.pubkey()),
                &[&self.keypair],
                blockhash,
            );
            self.submit_transaction_assert_success(transaction).await;
        }
    }

    pub async fn realloc_directed_stake_meta(&self) {
        use jito_steward::constants::MAX_ALLOC_BYTES;
        use jito_steward::state::directed_stake::DirectedStakeMeta;

        let directed_stake_meta = Pubkey::find_program_address(
            &[
                DirectedStakeMeta::SEED,
                self.steward_config.pubkey().as_ref(),
            ],
            &jito_steward::id(),
        )
        .0;

        // Get the validator list address from the config
        let config: jito_steward::Config = self
            .load_and_deserialize(&self.steward_config.pubkey())
            .await;
        let validator_list = config.validator_list;

        // Calculate how many reallocations we need
        let num_reallocs = (DirectedStakeMeta::SIZE - MAX_ALLOC_BYTES) / MAX_ALLOC_BYTES + 1;
        const INSTRUCTIONS_PER_TX: usize = 10;

        // Submit reallocations in batches to avoid exceeding instruction trace length
        let mut remaining = num_reallocs;
        while remaining > 0 {
            let batch_size = remaining.min(INSTRUCTIONS_PER_TX);
            let mut ixs = vec![];

            for _ in 0..batch_size {
                ixs.push(Instruction {
                    program_id: jito_steward::id(),
                    accounts: vec![
                        anchor_lang::solana_program::instruction::AccountMeta::new(
                            directed_stake_meta,
                            false,
                        ),
                        anchor_lang::solana_program::instruction::AccountMeta::new_readonly(
                            self.steward_config.pubkey(),
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
                            self.keypair.pubkey(),
                            true,
                        ),
                    ],
                    data: jito_steward::instruction::ReallocDirectedStakeMeta {}.data(),
                });
            }

            let blockhash = self
                .ctx
                .borrow_mut()
                .get_new_latest_blockhash()
                .await
                .unwrap();

            let tx = Transaction::new_signed_with_payer(
                &ixs,
                Some(&self.keypair.pubkey()),
                &[&self.keypair],
                blockhash,
            );
            self.submit_transaction_assert_success(tx).await;

            remaining = remaining.saturating_sub(batch_size);
        }

        // Verify that is_initialized has been set after reallocation
        let meta: DirectedStakeMeta = self.load_and_deserialize(&directed_stake_meta).await;
        assert!(
            bool::from(meta.is_initialized),
            "DirectedStakeMeta should be initialized after reallocation"
        );
    }

    pub async fn realloc_directed_stake_whitelist(&self) {
        use jito_steward::constants::MAX_ALLOC_BYTES;
        use jito_steward::state::directed_stake::DirectedStakeWhitelist;

        let directed_stake_whitelist = Pubkey::find_program_address(
            &[
                DirectedStakeWhitelist::SEED,
                self.steward_config.pubkey().as_ref(),
            ],
            &jito_steward::id(),
        )
        .0;

        // Get the validator list address from the config
        let config: jito_steward::Config = self
            .load_and_deserialize(&self.steward_config.pubkey())
            .await;
        let validator_list = config.validator_list;

        // Calculate how many reallocations we need
        let mut num_reallocs =
            (DirectedStakeWhitelist::SIZE - MAX_ALLOC_BYTES) / MAX_ALLOC_BYTES + 1;
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
                            self.steward_config.pubkey(),
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
                            self.keypair.pubkey(),
                            true,
                        ),
                    ],
                    data: jito_steward::instruction::ReallocDirectedStakeWhitelist {}.data(),
                };
                num_reallocs.min(10)
            ]);
            num_reallocs = num_reallocs.saturating_sub(10);
        }

        // Submit all reallocation instructions
        let tx = Transaction::new_signed_with_payer(
            &ixs,
            Some(&self.keypair.pubkey()),
            &[&self.keypair],
            self.ctx.borrow().last_blockhash,
        );
        self.submit_transaction_assert_success(tx).await;

        // Verify that is_initialized has been set after reallocation
        let whitelist: DirectedStakeWhitelist =
            self.load_and_deserialize(&directed_stake_whitelist).await;
        assert!(
            bool::from(whitelist.is_initialized),
            "DirectedStakeWhitelist should be initialized after reallocation"
        );
    }

    pub async fn initialize_validator_history_config(&self) {
        let instruction = Instruction {
            program_id: validator_history::id(),
            accounts: validator_history::accounts::InitializeConfig {
                config: self.validator_history_config,
                system_program: anchor_lang::solana_program::system_program::id(),
                signer: self.keypair.pubkey(),
            }
            .to_account_metas(None),
            data: validator_history::instruction::InitializeConfig {
                authority: self.keypair.pubkey(),
            }
            .data(),
        };

        let transaction = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&self.keypair.pubkey()),
            &[&self.keypair],
            self.ctx.borrow().last_blockhash,
        );
        self.submit_transaction_assert_success(transaction).await;
    }

    pub async fn initialize_validator_list(&self, num_validators: usize) {
        let stake_program_minimum = self.fetch_minimum_delegation().await;
        let pool_minimum_delegation = minimum_delegation(stake_program_minimum);
        let stake_rent = self.fetch_stake_rent().await;
        let minimum_active_stake_with_rent = pool_minimum_delegation + stake_rent;

        let validator_list_account_info =
            self.get_account(&self.stake_pool_meta.validator_list).await;

        let validator_list: ValidatorList = self
            .load_and_deserialize(&self.stake_pool_meta.validator_list)
            .await;

        let mut spl_validator_list = validator_list.as_ref().clone();

        for _ in 0..num_validators {
            spl_validator_list.validators.push(ValidatorStakeInfo {
                active_stake_lamports: minimum_active_stake_with_rent.into(),
                vote_account_address: Pubkey::new_unique(),
                ..ValidatorStakeInfo::default()
            });
        }

        self.ctx.borrow_mut().set_account(
            &self.stake_pool_meta.validator_list,
            &serialized_validator_list_account(
                spl_validator_list.clone(),
                Some(validator_list_account_info.data.len()),
            )
            .into(),
        );
    }

    /// Get a validator from the validator list by index
    pub async fn get_validator_from_list(&self, index: usize) -> Option<Pubkey> {
        let validator_list: ValidatorList = self
            .load_and_deserialize(&self.stake_pool_meta.validator_list)
            .await;

        validator_list
            .validators
            .get(index)
            .map(|v| v.vote_account_address)
    }

    // Turn this into a fixture creator
    pub async fn initialize_cluster_history_account(&self) -> ClusterHistory {
        todo!()
    }

    pub fn initialize_validator_history_with_credits(
        &self,
        vote_account: Pubkey,
        index: usize,
    ) -> Pubkey {
        let mut validator_history = validator_history_default(vote_account, index as u32);
        let validator_history_address = Pubkey::find_program_address(
            &[ValidatorHistory::SEED, vote_account.as_ref()],
            &validator_history::id(),
        )
        .0;
        for i in 0..20 {
            validator_history.history.push(ValidatorHistoryEntry {
                epoch: i,
                epoch_credits: 400000,
                activated_stake_lamports: 100_000_000_000_000,
                ..ValidatorHistoryEntry::default()
            });
        }

        let epoch_credits = vec![(0, 1, 0), (1, 2, 1), (2, 3, 2), (3, 4, 3), (4, 5, 4)];
        self.ctx.borrow_mut().set_account(
            &vote_account,
            &new_vote_account(Pubkey::new_unique(), vote_account, 1, Some(epoch_credits)).into(),
        );

        self.ctx.borrow_mut().set_account(
            &validator_history_address,
            &serialized_validator_history_account(validator_history).into(),
        );
        validator_history_address
    }

    pub async fn stake_accounts_for_validator(
        &self,
        vote_account: Pubkey,
    ) -> (Pubkey, Pubkey, Pubkey) {
        let (withdraw_authority, _) = find_withdraw_authority_program_address(
            &spl_stake_pool::id(),
            &self.stake_pool_meta.stake_pool,
        );

        // stake account
        let stake_account_address = find_stake_program_address(
            &spl_stake_pool::id(),
            &vote_account,
            &self.stake_pool_meta.stake_pool,
            None,
        )
        .0;

        // transient stake account
        let (transient_stake_account_address, _transient_seed) =
            find_transient_stake_program_address(
                &spl_stake_pool::id(),
                &vote_account,
                &self.stake_pool_meta.stake_pool,
                0,
            );

        (
            stake_account_address,
            transient_stake_account_address,
            withdraw_authority,
        )
    }

    pub async fn fetch_minimum_delegation(&self) -> u64 {
        let ix = solana_program::stake::instruction::get_minimum_delegation();
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&self.keypair.pubkey()),
            &[&self.keypair],
            self.ctx.borrow_mut().last_blockhash,
        );

        let process_tx_result = {
            let banks_client = self.ctx.borrow_mut().banks_client.clone();
            banks_client.process_transaction_with_metadata(tx).await
        };

        let result = process_tx_result.unwrap();

        assert!(result.result.is_ok());
        let metadata = result.metadata.unwrap();
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&metadata.return_data.clone().unwrap().data[..8]);
        u64::from_le_bytes(bytes)
    }

    pub async fn fetch_stake_rent(&self) -> u64 {
        let rent: Rent = {
            let banks_client = self.ctx.borrow_mut().banks_client.clone();
            banks_client.get_sysvar().await.expect("Failed to get rent")
        };

        rent.minimum_balance(StakeStateV2::size_of())
    }

    pub async fn advance_num_epochs(&self, num_epochs: u64, additional_slots: u64) {
        let clock: Clock = {
            let banks_client = self.ctx.borrow_mut().banks_client.clone();
            banks_client
                .get_sysvar()
                .await
                .expect("Failed getting clock")
        };
        let epoch_schedule: EpochSchedule = {
            let banks_client = self.ctx.borrow_mut().banks_client.clone();
            banks_client
                .get_sysvar()
                .await
                .expect("Failed getting epoch schedule")
        };
        let target_epoch = clock.epoch + num_epochs;
        let target_slot = epoch_schedule.get_first_slot_in_epoch(target_epoch) + additional_slots;

        self.ctx
            .borrow_mut()
            .warp_to_slot(target_slot)
            .expect("Failed warping to future epoch");
    }

    pub async fn advance_num_slots(&self, additional_slots: u64) {
        let clock: Clock = {
            let banks_client = self.ctx.borrow_mut().banks_client.clone();
            banks_client
                .get_sysvar()
                .await
                .expect("Failed getting clock")
        };
        let target_slot = clock.slot + additional_slots;

        self.ctx
            .borrow_mut()
            .warp_to_slot(target_slot)
            .expect("Failed warping to future slot");
    }

    pub async fn submit_transaction_assert_success(&self, transaction: Transaction) {
        let process_transaction_result = {
            let banks_client = self.ctx.borrow_mut().banks_client.clone();
            banks_client
                .process_transaction_with_preflight(transaction)
                .await
        };

        if let Err(e) = process_transaction_result {
            panic!("Error: {}", e);
        }
    }

    pub async fn submit_transaction_assert_error(
        &self,
        transaction: Transaction,
        error_message: &str,
    ) {
        let process_transaction_result = {
            let banks_client = self.ctx.borrow_mut().banks_client.clone();
            banks_client
                .process_transaction_with_preflight(transaction)
                .await
        };

        if let Err(e) = process_transaction_result {
            if !e.to_string().contains(error_message) {
                panic!("Error: {}\n\nDoes not match {}", e, error_message);
            }

            assert!(e.to_string().contains(error_message));
        } else {
            panic!("Error: Transaction succeeded. Expected {}", error_message);
        }
    }
}

#[derive(Clone)]
pub struct ExtraValidatorAccounts {
    pub vote_account: Pubkey,
    pub validator_history_address: Pubkey,
    pub stake_account_address: Pubkey,
    pub transient_stake_account_address: Pubkey,
    pub withdraw_authority: Pubkey,
}

/// Helper function to initialize directed stake meta
pub async fn initialize_directed_stake_meta(fixture: &TestFixture) -> Pubkey {
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
                system_program::id(),
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

pub async fn crank_stake_pool(fixture: &TestFixture) {
    let stake_pool: StakePool = fixture
        .load_and_deserialize(&fixture.stake_pool_meta.stake_pool)
        .await;
    let validator_list: ValidatorList = fixture
        .load_and_deserialize(&fixture.stake_pool_meta.validator_list)
        .await;
    let (initial_ixs, final_ixs) = spl_stake_pool::instruction::update_stake_pool(
        &spl_stake_pool::id(),
        stake_pool.as_ref(),
        validator_list.as_ref(),
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

/// Version of crank_rebalance_directed that works with real validator list data
/// This function finds validator indices automatically by searching the validator list
/// It also derives stake accounts automatically if they're not provided in extra_validator_accounts
pub async fn crank_rebalance_directed_with_real_validator_list(
    fixture: &TestFixture,
    _unit_test_fixtures: &StateMachineFixtures,
    extra_validator_accounts: &[ExtraValidatorAccounts],
) {
    let ctx = &fixture.ctx;

    // Load validator list once
    let validator_list: ValidatorList = fixture
        .load_and_deserialize(&fixture.stake_pool_meta.validator_list)
        .await;

    for extra_accounts in extra_validator_accounts.iter() {
        // Find the validator index in the validator list by vote account
        let _validator_list_index = validator_list
            .validators
            .iter()
            .position(|v| v.vote_account_address == extra_accounts.vote_account)
            .unwrap_or_else(|| {
                panic!(
                    "Vote account {} not found in validator list",
                    extra_accounts.vote_account
                )
            });

        let vote_account_at_index = extra_accounts.vote_account;

        // Find the directed_stake_meta_index by looking up the vote_pubkey in the directed_stake_meta
        let directed_stake_meta_pubkey = Pubkey::find_program_address(
            &[
                DirectedStakeMeta::SEED,
                fixture.steward_config.pubkey().as_ref(),
            ],
            &jito_steward::id(),
        )
        .0;
        let directed_stake_meta: DirectedStakeMeta = fixture
            .load_and_deserialize(&directed_stake_meta_pubkey)
            .await;

        // Skip if the vote account is not in directed_stake_meta (e.g., was removed by copy_directed_stake_targets with 0 lamports)
        // This ensures we only process validators that exist in both the validator_list and the directed_stake_meta
        let Some(directed_stake_meta_index) =
            directed_stake_meta.get_target_index(&vote_account_at_index)
        else {
            continue;
        };

        // Derive stake accounts if they're not provided (i.e., if they're default/pubkey::default())
        let (stake_account, transient_stake_account, withdraw_authority) =
            if extra_accounts.stake_account_address == Pubkey::default() {
                fixture
                    .stake_accounts_for_validator(vote_account_at_index)
                    .await
            } else {
                (
                    extra_accounts.stake_account_address,
                    extra_accounts.transient_stake_account_address,
                    extra_accounts.withdraw_authority,
                )
            };

        let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(1_400_000);
        let ix = Instruction {
            program_id: jito_steward::id(),
            accounts: jito_steward::accounts::RebalanceDirected {
                config: fixture.steward_config.pubkey(),
                state_account: fixture.steward_state,
                directed_stake_meta: directed_stake_meta_pubkey,
                stake_pool: fixture.stake_pool_meta.stake_pool,
                stake_pool_program: spl_stake_pool::id(),
                withdraw_authority,
                validator_list: fixture.stake_pool_meta.validator_list,
                reserve_stake: fixture.stake_pool_meta.reserve,
                stake_account,
                transient_stake_account,
                vote_account: vote_account_at_index,
                clock: solana_sdk::sysvar::clock::id(),
                rent: solana_sdk::sysvar::rent::id(),
                stake_history: solana_sdk::sysvar::stake_history::id(),
                stake_config: solana_sdk::stake::config::ID,
                system_program: system_program::id(),
                stake_program: solana_sdk::stake::program::id(),
            }
            .to_account_metas(None),
            data: jito_steward::instruction::RebalanceDirected {
                directed_stake_meta_index: directed_stake_meta_index as u64,
            }
            .data(),
        };
        let blockhash = ctx.borrow_mut().get_new_latest_blockhash().await.unwrap();
        let tx = Transaction::new_signed_with_payer(
            &[compute_budget_ix, ix],
            Some(&fixture.keypair.pubkey()),
            &[&fixture.keypair],
            blockhash,
        );
        fixture.submit_transaction_assert_success(tx).await;
    }
}

pub async fn crank_rebalance_directed(
    fixture: &TestFixture,
    _unit_test_fixtures: &StateMachineFixtures,
    extra_validator_accounts: &[ExtraValidatorAccounts],
    indices: &[usize],
) {
    let ctx = &fixture.ctx;

    for &i in indices {
        let extra_accounts = &extra_validator_accounts[i];
        let validator_list_index = i;

        // Get the vote account at the validator list index
        let validator_list: ValidatorList = fixture
            .load_and_deserialize(&fixture.stake_pool_meta.validator_list)
            .await;
        let vote_account_at_index =
            validator_list.validators[validator_list_index].vote_account_address;

        // Find the directed_stake_meta_index by looking up the vote_pubkey in the directed_stake_meta
        let directed_stake_meta_pubkey = Pubkey::find_program_address(
            &[
                DirectedStakeMeta::SEED,
                fixture.steward_config.pubkey().as_ref(),
            ],
            &jito_steward::id(),
        )
        .0;
        let directed_stake_meta: DirectedStakeMeta = fixture
            .load_and_deserialize(&directed_stake_meta_pubkey)
            .await;

        // Skip if the vote account is not in directed_stake_meta (e.g., was removed by copy_directed_stake_targets with 0 lamports)
        // This ensures we only process validators that exist in both the validator_list and the directed_stake_meta
        let Some(directed_stake_meta_index) =
            directed_stake_meta.get_target_index(&vote_account_at_index)
        else {
            continue;
        };

        let compute_budget_ix = ComputeBudgetInstruction::set_compute_unit_limit(1_400_000);
        let ix = Instruction {
            program_id: jito_steward::id(),
            accounts: jito_steward::accounts::RebalanceDirected {
                config: fixture.steward_config.pubkey(),
                state_account: fixture.steward_state,
                directed_stake_meta: directed_stake_meta_pubkey,
                stake_pool: fixture.stake_pool_meta.stake_pool,
                stake_pool_program: spl_stake_pool::id(),
                withdraw_authority: extra_accounts.withdraw_authority,
                validator_list: fixture.stake_pool_meta.validator_list,
                reserve_stake: fixture.stake_pool_meta.reserve,
                stake_account: extra_accounts.stake_account_address,
                transient_stake_account: find_transient_stake_program_address(
                    &spl_stake_pool::id(),
                    &vote_account_at_index,
                    &fixture.stake_pool_meta.stake_pool,
                    0u64,
                )
                .0,
                vote_account: vote_account_at_index,
                clock: solana_sdk::sysvar::clock::id(),
                rent: solana_sdk::sysvar::rent::id(),
                stake_history: solana_sdk::sysvar::stake_history::id(),
                stake_config: solana_sdk::stake::config::ID,
                system_program: system_program::id(),
                stake_program: solana_sdk::stake::program::id(),
            }
            .to_account_metas(None),
            data: jito_steward::instruction::RebalanceDirected {
                directed_stake_meta_index: directed_stake_meta_index as u64,
            }
            .data(),
        };
        let blockhash = ctx.borrow_mut().get_new_latest_blockhash().await.unwrap();
        let tx = Transaction::new_signed_with_payer(
            &[compute_budget_ix, ix],
            Some(&fixture.keypair.pubkey()),
            &[&fixture.keypair],
            blockhash,
        );
        fixture.submit_transaction_assert_success(tx).await;
    }
}

pub async fn crank_epoch_maintenance(fixture: &TestFixture, remove_indices: Option<&[usize]>) {
    let ctx = &fixture.ctx;
    let directed_stake_meta = Pubkey::find_program_address(
        &[
            jito_steward::state::directed_stake::DirectedStakeMeta::SEED,
            fixture.steward_config.pubkey().as_ref(),
        ],
        &jito_steward::id(),
    )
    .0;
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
                    directed_stake_meta,
                }
                .to_account_metas(None),
                data: jito_steward::instruction::EpochMaintenance {
                    validator_index_to_remove: Some(*i as u64),
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
                directed_stake_meta,
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

pub async fn auto_add_validator(fixture: &TestFixture, extra_accounts: &ExtraValidatorAccounts) {
    let ctx = &fixture.ctx;

    let ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::AutoAddValidator {
            validator_history_account: extra_accounts.validator_history_address,
            steward_state: fixture.steward_state,
            config: fixture.steward_config.pubkey(),
            stake_pool_program: spl_stake_pool::id(),
            stake_pool: fixture.stake_pool_meta.stake_pool,
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

pub async fn auto_remove_validator(
    fixture: &TestFixture,
    extra_accounts: &ExtraValidatorAccounts,
    index: u64,
) {
    let ctx = &fixture.ctx;

    let ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::AutoRemoveValidator {
            config: fixture.steward_config.pubkey(),
            state_account: fixture.steward_state,
            validator_list: fixture.stake_pool_meta.validator_list,
            stake_pool: fixture.stake_pool_meta.stake_pool,
            stake_account: extra_accounts.stake_account_address,
            withdraw_authority: extra_accounts.withdraw_authority,
            validator_history_account: extra_accounts.validator_history_address,
            reserve_stake: fixture.stake_pool_meta.reserve,
            transient_stake_account: extra_accounts.transient_stake_account_address,
            vote_account: extra_accounts.vote_account,
            stake_history: solana_sdk::sysvar::stake_history::id(),
            stake_config: stake::config::ID,
            stake_program: stake::program::id(),
            stake_pool_program: spl_stake_pool::id(),
            system_program: system_program::id(),
            rent: solana_sdk::sysvar::rent::id(),
            clock: solana_sdk::sysvar::clock::id(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::AutoRemoveValidatorFromPool {
            validator_list_index: index,
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

pub async fn instant_remove_validator(fixture: &TestFixture, index: usize) {
    let directed_stake_meta = Pubkey::find_program_address(
        &[
            jito_steward::state::directed_stake::DirectedStakeMeta::SEED,
            fixture.steward_config.pubkey().as_ref(),
        ],
        &jito_steward::id(),
    )
    .0;
    let ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::InstantRemoveValidator {
            config: fixture.steward_config.pubkey(),
            state_account: fixture.steward_state,
            validator_list: fixture.stake_pool_meta.validator_list,
            stake_pool: fixture.stake_pool_meta.stake_pool,
            directed_stake_meta,
        }
        .to_account_metas(None),
        data: jito_steward::instruction::InstantRemoveValidator {
            validator_index_to_remove: index as u64,
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
        &[ix],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        blockhash,
    );
    fixture.submit_transaction_assert_success(tx).await;
}

pub async fn manual_remove_validator(
    fixture: &TestFixture,
    index: usize,
    mark_for_removal: bool,
    immediate: bool,
) {
    let ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::AdminMarkForRemoval {
            config: fixture.steward_config.pubkey(),
            state_account: fixture.steward_state,
            authority: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::AdminMarkForRemoval {
            validator_list_index: index as u64,
            mark_for_removal: if mark_for_removal { 1 } else { 0 },
            immediate: if immediate { 1 } else { 0 },
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
        &[ix],
        Some(&fixture.keypair.pubkey()),
        &[&fixture.keypair],
        blockhash,
    );
    fixture.submit_transaction_assert_success(tx).await;
}

pub async fn crank_compute_score(
    fixture: &TestFixture,
    _unit_test_fixtures: &StateMachineFixtures,
    extra_validator_accounts: &[ExtraValidatorAccounts],
    indices: &[usize],
) {
    let ctx = &fixture.ctx;

    for &i in indices {
        let compute_ix = ComputeBudgetInstruction::set_compute_unit_limit(1_400_000);
        let heap_ix = ComputeBudgetInstruction::request_heap_frame(256 * 1024);
        let ix = Instruction {
            program_id: jito_steward::id(),
            accounts: jito_steward::accounts::ComputeScore {
                config: fixture.steward_config.pubkey(),
                state_account: fixture.steward_state,
                validator_list: fixture.stake_pool_meta.validator_list,
                validator_history: extra_validator_accounts[i].validator_history_address,
                cluster_history: fixture.cluster_history_account,
            }
            .to_account_metas(None),
            data: jito_steward::instruction::ComputeScore {
                validator_list_index: i as u64,
            }
            .data(),
        };
        let blockhash = ctx.borrow_mut().get_new_latest_blockhash().await.unwrap();
        let tx = Transaction::new_signed_with_payer(
            &[compute_ix, heap_ix, ix],
            Some(&fixture.keypair.pubkey()),
            &[&fixture.keypair],
            blockhash,
        );
        fixture.submit_transaction_assert_success(tx).await;
    }
}

/// Version of crank_compute_score that works with real validator list data
/// This function looks up the validator history address from the validator list index
pub async fn crank_compute_score_with_real_validator_list(
    fixture: &TestFixture,
    _unit_test_fixtures: &StateMachineFixtures,
    _extra_validator_accounts: &[ExtraValidatorAccounts],
    indices: &[usize],
) {
    let ctx = &fixture.ctx;

    // Load validator list once
    let validator_list: ValidatorList = fixture
        .load_and_deserialize(&fixture.stake_pool_meta.validator_list)
        .await;

    for &i in indices {
        // Get the vote account at the validator list index
        let vote_account = validator_list.validators[i].vote_account_address;

        // Derive the validator history address from the vote account
        let validator_history_address = Pubkey::find_program_address(
            &[
                validator_history::ValidatorHistory::SEED,
                vote_account.as_ref(),
            ],
            &validator_history::id(),
        )
        .0;

        let compute_ix = ComputeBudgetInstruction::set_compute_unit_limit(1_400_000);
        let heap_ix = ComputeBudgetInstruction::request_heap_frame(256 * 1024);
        let ix = Instruction {
            program_id: jito_steward::id(),
            accounts: jito_steward::accounts::ComputeScore {
                config: fixture.steward_config.pubkey(),
                state_account: fixture.steward_state,
                validator_list: fixture.stake_pool_meta.validator_list,
                validator_history: validator_history_address,
                cluster_history: fixture.cluster_history_account,
            }
            .to_account_metas(None),
            data: jito_steward::instruction::ComputeScore {
                validator_list_index: i as u64,
            }
            .data(),
        };
        let blockhash = ctx.borrow_mut().get_new_latest_blockhash().await.unwrap();
        let tx = Transaction::new_signed_with_payer(
            &[compute_ix, heap_ix, ix],
            Some(&fixture.keypair.pubkey()),
            &[&fixture.keypair],
            blockhash,
        );
        fixture.submit_transaction_assert_success(tx).await;
    }
}

pub async fn crank_compute_delegations(fixture: &TestFixture) {
    let ctx = &fixture.ctx;
    let ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::ComputeDelegations {
            config: fixture.steward_config.pubkey(),
            state_account: fixture.steward_state,
            validator_list: fixture.stake_pool_meta.validator_list,
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

pub async fn crank_idle(fixture: &TestFixture) {
    let ctx = &fixture.ctx;

    let ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::Idle {
            config: fixture.steward_config.pubkey(),
            state_account: fixture.steward_state,
            validator_list: fixture.stake_pool_meta.validator_list,
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

pub async fn crank_compute_instant_unstake(
    fixture: &TestFixture,
    _unit_test_fixtures: &StateMachineFixtures,
    extra_validator_accounts: &[ExtraValidatorAccounts],
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
            }
            .to_account_metas(None),
            data: jito_steward::instruction::ComputeInstantUnstake {
                validator_list_index: i as u64,
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

pub async fn crank_rebalance(
    fixture: &TestFixture,
    _unit_test_fixtures: &StateMachineFixtures,
    extra_validator_accounts: &[ExtraValidatorAccounts],
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
                withdraw_authority: extra_accounts.withdraw_authority,
                validator_list: fixture.stake_pool_meta.validator_list,
                reserve_stake: fixture.stake_pool_meta.reserve,
                stake_account: extra_accounts.stake_account_address,
                transient_stake_account: extra_accounts.transient_stake_account_address,
                vote_account: extra_accounts.vote_account,
                directed_stake_meta: Pubkey::find_program_address(
                    &[
                        DirectedStakeMeta::SEED,
                        fixture.steward_config.pubkey().as_ref(),
                    ],
                    &jito_steward::id(),
                )
                .0,
                system_program: system_program::id(),
                stake_program: stake::program::id(),
                rent: solana_sdk::sysvar::rent::id(),
                clock: solana_sdk::sysvar::clock::id(),
                stake_history: solana_sdk::sysvar::stake_history::id(),
                stake_config: stake::config::ID,
            }
            .to_account_metas(None),
            data: jito_steward::instruction::Rebalance {
                validator_list_index: i as u64,
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

pub async fn copy_vote_account(
    fixture: &TestFixture,
    extra_validator_accounts: &[ExtraValidatorAccounts],
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

pub async fn update_stake_history(
    fixture: &TestFixture,
    extra_validator_accounts: &[ExtraValidatorAccounts],
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

pub async fn copy_cluster_info(fixture: &TestFixture) {
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

pub async fn crank_validator_history_accounts_no_credits(
    fixture: &TestFixture,
    extra_validator_accounts: &[ExtraValidatorAccounts],
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
            .increment_vote_account_credits(&extra_validator_accounts[i].vote_account, 0);
        copy_vote_account(fixture, extra_validator_accounts, i).await;
        // only field that's relevant to score is is_superminority
        update_stake_history(
            fixture,
            extra_validator_accounts,
            i,
            clock.epoch,
            1_000_000,
            1_000,
            false,
        )
        .await;
    }
    copy_cluster_info(fixture).await;
}

pub async fn crank_validator_history_accounts(
    fixture: &TestFixture,
    extra_validator_accounts: &[ExtraValidatorAccounts],
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
        copy_vote_account(fixture, extra_validator_accounts, i).await;
        // only field that's relevant to score is is_superminority
        update_stake_history(
            fixture,
            extra_validator_accounts,
            i,
            clock.epoch,
            1_000_000,
            1_000,
            false,
        )
        .await;
    }
    copy_cluster_info(fixture).await;
}

/// Version of crank_validator_history_accounts that works with real validator list data
/// This function looks up the validator history address from the validator list index
pub async fn crank_validator_history_accounts_with_real_validator_list(
    fixture: &TestFixture,
    indices: &[usize],
) {
    // Load validator list once
    let validator_list: ValidatorList = fixture
        .load_and_deserialize(&fixture.stake_pool_meta.validator_list)
        .await;

    let clock: Clock = fixture
        .ctx
        .borrow_mut()
        .banks_client
        .get_sysvar()
        .await
        .unwrap();

    for &i in indices {
        // Get the vote account at the validator list index
        let vote_account = validator_list.validators[i].vote_account_address;

        // Derive the validator history address from the vote account
        let validator_history_address = Pubkey::find_program_address(
            &[
                validator_history::ValidatorHistory::SEED,
                vote_account.as_ref(),
            ],
            &validator_history::id(),
        )
        .0;

        fixture
            .ctx
            .borrow_mut()
            .increment_vote_account_credits(&vote_account, 1000);

        // Copy vote account
        let ctx = &fixture.ctx;
        let ix = Instruction {
            program_id: validator_history::id(),
            accounts: validator_history::accounts::CopyVoteAccount {
                validator_history_account: validator_history_address,
                vote_account,
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

        // Update stake history
        let ix = Instruction {
            program_id: validator_history::id(),
            accounts: validator_history::accounts::UpdateStakeHistory {
                validator_history_account: validator_history_address,
                vote_account,
                config: fixture.validator_history_config,
                oracle_authority: fixture.keypair.pubkey(),
            }
            .to_account_metas(None),
            data: validator_history::instruction::UpdateStakeHistory {
                epoch: clock.epoch,
                is_superminority: false,
                lamports: 1_000_000,
                rank: 1_000,
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
    copy_cluster_info(fixture).await;
}

pub struct ValidatorEntry {
    pub vote_address: Pubkey,
    pub vote_account: VoteStateVersions,
    pub validator_history: ValidatorHistory,
}

impl Default for ValidatorEntry {
    fn default() -> Self {
        let vote_address = Pubkey::new_unique();
        let vote_account = new_vote_state_versions(vote_address, vote_address, 0, None);
        let validator_history = validator_history_default(vote_address, 0);

        Self {
            vote_address,
            vote_account,
            validator_history,
        }
    }
}

pub struct FixtureDefaultAccounts {
    pub stake_pool_meta: StakePoolMetadata,
    pub directed_stake_meta: Pubkey,
    pub stake_pool: spl_stake_pool::state::StakePool,
    pub validator_list: SPLValidatorList,
    pub steward_config_keypair: Keypair,
    pub steward_config: Config,
    pub steward_state_address: Pubkey,
    pub steward_state: Box<StewardStateAccountV2>,
    pub validator_history_config: validator_history::state::Config,
    pub cluster_history: ClusterHistory,
    pub validators: Vec<ValidatorEntry>,
    pub keypair: Keypair,
}

impl Default for FixtureDefaultAccounts {
    fn default() -> Self {
        let keypair = Keypair::new();

        // For each main thing to add to runtime, create a default account
        let stake_pool_meta = StakePoolMetadata::default();
        let stake_pool =
            FixtureDefaultAccounts::stake_pool_default(&stake_pool_meta, keypair.pubkey());

        let validator_list = SPLValidatorList::new(MAX_VALIDATORS as u32);

        let steward_config_keypair = Keypair::new();
        let steward_config = Config {
            stake_pool: stake_pool_meta.stake_pool,
            validator_list: stake_pool_meta.validator_list,
            blacklist_authority: keypair.pubkey(),
            parameters_authority: keypair.pubkey(),
            admin: keypair.pubkey(),
            validator_history_blacklist: LargeBitMask::default(),
            parameters: Parameters::default(),
            paused: false.into(),
            _padding_0: [0u8; 7],
            priority_fee_parameters_authority: Pubkey::new_unique(),
            directed_stake_meta_upload_authority: Pubkey::new_unique(),
            directed_stake_whitelist_authority: Pubkey::new_unique(),
            directed_stake_ticket_override_authority: Pubkey::new_unique(),
            _padding: [0; 888],
        };

        let directed_stake_meta = Pubkey::find_program_address(
            &[
                DirectedStakeMeta::SEED,
                steward_config_keypair.pubkey().as_ref(),
            ],
            &jito_steward::id(),
        )
        .0;

        let (steward_state_address, steward_state_bump) = Pubkey::find_program_address(
            &[
                StewardStateAccount::SEED,
                steward_config_keypair.pubkey().as_ref(),
            ],
            &jito_steward::id(),
        );

        let steward_state = StewardStateV2 {
            state_tag: StewardStateEnum::RebalanceDirected,
            validator_lamport_balances: [0; MAX_VALIDATORS],
            scores: [0; MAX_VALIDATORS],
            sorted_score_indices: [SORTED_INDEX_DEFAULT; MAX_VALIDATORS],
            raw_scores: [0; MAX_VALIDATORS],
            sorted_raw_score_indices: [SORTED_INDEX_DEFAULT; MAX_VALIDATORS],
            delegations: [Delegation::default(); MAX_VALIDATORS],
            instant_unstake: BitMask::default(),
            progress: BitMask::default(),
            validators_to_remove: BitMask::default(),
            validators_for_immediate_removal: BitMask::default(),
            start_computing_scores_slot: 0,
            current_epoch: 0,
            next_cycle_epoch: 10,
            num_pool_validators: 0,
            scoring_unstake_total: 0,
            instant_unstake_total: 0,
            stake_deposit_unstake_total: 0,
            validators_added: 0,
            status_flags: 0,
            _padding0: [0; 2],
        };
        let steward_state_account = Box::new(StewardStateAccountV2 {
            state: steward_state,
            bump: steward_state_bump,
            _padding0: [0; 7],
        });

        let validator_history_config_bump = Pubkey::find_program_address(
            &[validator_history::state::Config::SEED],
            &validator_history::id(),
        )
        .1;
        let validator_history_config = validator_history::state::Config {
            bump: validator_history_config_bump,
            counter: 1,
            admin: keypair.pubkey(),
            oracle_authority: keypair.pubkey(),
            tip_distribution_program: jito_tip_distribution::id(),
            padding0: [0u8; 3],
            priority_fee_distribution_program: jito_priority_fee_distribution::id(),
            priority_fee_oracle_authority: keypair.pubkey(),
            reserve: [0u8; 224],
        };
        let cluster_history = cluster_history_default();

        Self {
            stake_pool_meta,
            stake_pool,
            validator_list,
            steward_config_keypair,
            steward_config,
            steward_state_address,
            steward_state: steward_state_account,
            validator_history_config,
            cluster_history,
            validators: vec![],
            keypair,
            directed_stake_meta,
        }
    }
}

impl FixtureDefaultAccounts {
    fn to_accounts_vec(&self) -> Vec<(Pubkey, Account)> {
        let validator_entry_accounts = self
            .validators
            .iter()
            .map(|ve| {
                let validator_history_address = Pubkey::find_program_address(
                    &[ValidatorHistory::SEED, ve.vote_address.as_ref()],
                    &validator_history::id(),
                )
                .0;
                (
                    validator_history_address,
                    serialized_validator_history_account(ve.validator_history),
                )
            })
            .collect::<Vec<_>>();
        let vote_accounts_and_addresses = self
            .validators
            .iter()
            .map(|ve| {
                let vote_address = ve.vote_address;
                let mut data = vec![0; VoteState::size_of()];
                VoteState::serialize(&ve.vote_account, &mut data).unwrap();

                let vote_account = Account {
                    lamports: 1000000,
                    data,
                    owner: anchor_lang::solana_program::vote::program::ID,
                    ..Account::default()
                };
                (vote_address, vote_account)
            })
            .collect::<Vec<_>>();

        let cluster_history_address =
            Pubkey::find_program_address(&[ClusterHistory::SEED], &validator_history::id()).0;
        let steward_state_address = Pubkey::find_program_address(
            &[
                StewardStateAccount::SEED,
                self.steward_config_keypair.pubkey().as_ref(),
            ],
            &jito_steward::id(),
        )
        .0;

        let validator_history_config_address = Pubkey::find_program_address(
            &[validator_history::state::Config::SEED],
            &validator_history::id(),
        )
        .0;

        // For each account, serialize and return as a tuple
        let mut accounts = vec![
            (
                self.steward_config_keypair.pubkey(),
                serialized_config(self.steward_config),
            ),
            (
                steward_state_address,
                serialized_steward_state_account(*self.steward_state),
            ),
            (
                validator_history_config_address,
                serialized_validator_history_config(self.validator_history_config.clone()),
            ),
            // (
            //     self.stake_pool_meta.stake_pool,
            //     serialized_stake_pool_account(
            //         self.stake_pool.clone(),
            //         std::mem::size_of::<StakePool>(),
            //     ),
            // ),
            // (
            //     self.stake_pool_meta.validator_list,
            //     serialized_validator_list_account(
            //         self.validator_list.clone(),
            //         Some(std::mem::size_of_val(&self.validator_list)),
            //     ),
            // ),
            (
                cluster_history_address,
                serialized_cluster_history_account(self.cluster_history),
            ),
            (self.keypair.pubkey(), system_account(100_000_000_000)),
        ];
        accounts.extend(validator_entry_accounts);
        accounts.extend(vote_accounts_and_addresses);
        accounts
    }

    fn stake_pool_default(
        stake_pool_meta: &StakePoolMetadata,
        admin: Pubkey,
    ) -> spl_stake_pool::state::StakePool {
        let stake_pool_address = stake_pool_meta.stake_pool;
        let validator_list = stake_pool_meta.validator_list;
        let reserve_stake = stake_pool_meta.reserve;
        let stake_deposit_authority = Pubkey::find_program_address(
            &[stake_pool_address.as_ref(), b"deposit"],
            &spl_stake_pool::id(),
        )
        .0;
        let stake_withdraw_bump_seed = Pubkey::find_program_address(
            &[stake_pool_address.as_ref(), STAKE_POOL_WITHDRAW_SEED],
            &spl_stake_pool::id(),
        )
        .1;
        let epoch_fee = Fee {
            numerator: 1,
            denominator: 100,
        };
        let withdrawal_fee = Fee {
            numerator: 1,
            denominator: 100,
        };
        let deposit_fee = Fee {
            numerator: 1,
            denominator: 100,
        };
        // Use default values from stake pool initialization
        spl_stake_pool::state::StakePool {
            account_type: AccountType::StakePool,
            manager: admin,
            staker: admin,
            stake_deposit_authority,
            stake_withdraw_bump_seed,
            validator_list,
            reserve_stake,
            pool_mint: Pubkey::new_unique(),
            manager_fee_account: Pubkey::new_unique(),
            token_program_id: spl_token::id(),
            total_lamports: 0,
            pool_token_supply: 0,
            last_update_epoch: 0,
            lockup: Lockup::default(),
            epoch_fee,
            next_epoch_fee: FutureEpoch::None,
            preferred_deposit_validator_vote_address: None,
            preferred_withdraw_validator_vote_address: None,
            stake_deposit_fee: deposit_fee,
            stake_withdrawal_fee: withdrawal_fee,
            next_stake_withdrawal_fee: FutureEpoch::None,
            stake_referral_fee: 0,
            sol_deposit_authority: None,
            sol_deposit_fee: deposit_fee,
            sol_withdraw_authority: None,
            sol_referral_fee: 0,
            sol_withdrawal_fee: withdrawal_fee,
            next_sol_withdrawal_fee: FutureEpoch::None,
            last_epoch_pool_token_supply: 0,
            last_epoch_total_lamports: 0,
        }
    }
}

pub fn new_vote_state_versions(
    node_pubkey: Pubkey,
    vote_pubkey: Pubkey,
    commission: u8,
    maybe_epoch_credits: Option<Vec<(u64, u64, u64)>>,
) -> VoteStateVersions {
    let vote_init = VoteInit {
        node_pubkey,
        authorized_voter: vote_pubkey,
        authorized_withdrawer: vote_pubkey,
        commission,
    };
    let clock = Clock {
        epoch: 0,
        slot: 0,
        unix_timestamp: 0,
        leader_schedule_epoch: 0,
        epoch_start_timestamp: 0,
    };
    let mut vote_state = VoteState::new(&vote_init, &clock);
    if let Some(epoch_credits) = maybe_epoch_credits {
        vote_state.epoch_credits = epoch_credits;
    }
    VoteStateVersions::new_current(vote_state)
}

pub fn validator_history_config_account(bump: u8, num_validators: u32) -> Account {
    let config = validator_history::state::Config {
        bump,
        counter: num_validators,
        ..Default::default()
    };

    let mut data = vec![];
    config.try_serialize(&mut data).unwrap();
    Account {
        lamports: 1_000_000_000,
        data,
        owner: validator_history::id(),
        ..Account::default()
    }
}

pub fn system_account(lamports: u64) -> Account {
    Account {
        lamports,
        owner: anchor_lang::system_program::ID,
        executable: false,
        rent_epoch: 0,
        data: vec![],
    }
}

pub fn new_vote_account(
    node_pubkey: Pubkey,
    vote_pubkey: Pubkey,
    commission: u8,
    maybe_epoch_credits: Option<Vec<(u64, u64, u64)>>,
) -> Account {
    let vote_init = VoteInit {
        node_pubkey,
        authorized_voter: vote_pubkey,
        authorized_withdrawer: vote_pubkey,
        commission,
    };
    let clock = Clock {
        epoch: 0,
        slot: 0,
        unix_timestamp: 0,
        leader_schedule_epoch: 0,
        epoch_start_timestamp: 0,
    };
    let mut vote_state = VoteState::new(&vote_init, &clock);
    if let Some(epoch_credits) = maybe_epoch_credits {
        vote_state.epoch_credits = epoch_credits;
    }
    let vote_state_versions = VoteStateVersions::new_current(vote_state);
    let mut data = vec![0; VoteState::size_of()];
    VoteState::serialize(&vote_state_versions, &mut data).unwrap();

    Account {
        lamports: 1000000,
        data,
        owner: anchor_lang::solana_program::vote::program::ID,
        ..Account::default()
    }
}

pub fn closed_vote_account() -> Account {
    Account {
        lamports: 0,
        data: vec![0; VoteState::size_of()],
        owner: anchor_lang::system_program::ID, // Close the account
        ..Account::default()
    }
}

pub fn serialized_stake_account(stake_account: StakeStateV2, lamports: u64) -> Account {
    let mut data = vec![];
    borsh1::BorshSerialize::serialize(&stake_account, &mut data).unwrap();
    Account {
        lamports,
        data,
        owner: anchor_lang::solana_program::stake::program::id(),
        ..Account::default()
    }
}

pub fn serialized_validator_history_account(validator_history: ValidatorHistory) -> Account {
    let mut data = vec![];
    validator_history.serialize(&mut data).unwrap();
    for byte in ValidatorHistory::DISCRIMINATOR.iter().rev() {
        data.insert(0, *byte);
    }
    Account {
        lamports: 1_000_000_000,
        data,
        owner: validator_history::id(),
        ..Account::default()
    }
}

pub fn serialized_steward_state_account_v1(state: StewardStateAccount) -> Account {
    let mut data = Vec::with_capacity(StewardStateAccount::SIZE);
    // Add discriminator
    data.extend_from_slice(StewardStateAccount::DISCRIMINATOR);
    // Add account data using bytemuck
    data.extend_from_slice(bytemuck::bytes_of(&state));
    Account {
        lamports: 100_000_000_000,
        data,
        owner: jito_steward::id(),
        ..Account::default()
    }
}

pub fn serialized_steward_state_account(state: StewardStateAccountV2) -> Account {
    let mut data = Vec::with_capacity(StewardStateAccountV2::SIZE);
    // Add discriminator
    data.extend_from_slice(StewardStateAccountV2::DISCRIMINATOR);
    // Add account data using bytemuck
    data.extend_from_slice(bytemuck::bytes_of(&state));
    Account {
        lamports: 100_000_000_000,
        data,
        owner: jito_steward::id(),
        ..Account::default()
    }
}

pub fn serialized_config(config: Config) -> Account {
    let mut data = vec![];
    config.serialize(&mut data).unwrap();
    for byte in Config::DISCRIMINATOR.iter().rev() {
        data.insert(0, *byte);
    }
    Account {
        lamports: 1_000_000_000,
        data,
        owner: jito_steward::id(),
        ..Account::default()
    }
}

pub fn validator_history_default(vote_account: Pubkey, index: u32) -> ValidatorHistory {
    let bump = Pubkey::find_program_address(
        &[ValidatorHistory::SEED, vote_account.as_ref()],
        &validator_history::id(),
    )
    .1;

    // Need to find a decent way to modify these entries
    let history = CircBuf {
        arr: [ValidatorHistoryEntry::default(); ValidatorHistory::MAX_ITEMS],
        idx: ValidatorHistory::MAX_ITEMS as u64 - 1,
        is_empty: 1,
        padding: [0; 7],
    };

    ValidatorHistory {
        struct_version: 0,
        vote_account,
        index,
        bump,
        _padding0: [0; 7],
        last_ip_timestamp: 0,
        last_version_timestamp: 0,
        validator_age: 0,
        validator_age_last_updated_epoch: 0,
        _padding1: [0; 226],
        history,
    }
}

pub fn serialized_validator_history_config(config: validator_history::state::Config) -> Account {
    let mut data = vec![];
    config.try_serialize(&mut data).unwrap();
    Account {
        lamports: 1_000_000_000,
        data,
        owner: validator_history::id(),
        ..Account::default()
    }
}

pub fn cluster_history_default() -> ClusterHistory {
    let bump = Pubkey::find_program_address(&[ClusterHistory::SEED], &validator_history::id()).1;
    ClusterHistory {
        struct_version: 0,
        bump,
        _padding0: [0; 7],
        cluster_history_last_update_slot: 0,
        _padding1: [0; 232],
        history: CircBufCluster {
            arr: [ClusterHistoryEntry::default(); ClusterHistory::MAX_ITEMS],
            idx: ClusterHistory::MAX_ITEMS as u64 - 1,
            is_empty: 1,
            padding: [0; 7],
        },
    }
}

pub fn serialized_cluster_history_account(cluster_history: ClusterHistory) -> Account {
    let mut data = vec![];
    cluster_history.serialize(&mut data).unwrap();
    for byte in ClusterHistory::DISCRIMINATOR.iter().rev() {
        data.insert(0, *byte);
    }
    Account {
        lamports: 10_000_000_000,
        data,
        owner: validator_history::id(),
        ..Account::default()
    }
}

/*
StewardState is large enough that you may need to heap-allocate this struct or request a larger stack size.
*/
pub struct StateMachineFixtures {
    pub current_epoch: u64,
    pub clock: Clock,
    pub epoch_schedule: EpochSchedule,
    pub validators: Vec<ValidatorHistory>,
    pub vote_accounts: Vec<VoteStateVersions>,
    pub cluster_history: ClusterHistory,
    pub config: Config,
    pub validator_list: Vec<ValidatorStakeInfo>,
    pub state: StewardStateV2,
}

impl Default for StateMachineFixtures {
    fn default() -> Self {
        let current_epoch = 20;

        // Setup parameters
        let parameters = Parameters {
            mev_commission_range: 10,
            epoch_credits_range: 20,
            commission_range: 20,
            mev_commission_bps_threshold: 1000,
            scoring_delinquency_threshold_ratio: 0.875,
            instant_unstake_delinquency_threshold_ratio: 0.1,
            commission_threshold: 10,
            historical_commission_threshold: 10,
            num_delegation_validators: 3,
            scoring_unstake_cap_bps: 1000,
            instant_unstake_cap_bps: 1000,
            stake_deposit_unstake_cap_bps: 1000,
            compute_score_slot_range: 500,
            instant_unstake_epoch_progress: 0.95,
            instant_unstake_inputs_epoch_progress: 0.5,
            num_epochs_between_scoring: 10,
            minimum_stake_lamports: 1,
            minimum_voting_epochs: 1,
            priority_fee_lookback_epochs: 10,
            priority_fee_lookback_offset: 2,
            priority_fee_max_commission_bps: 5_000,
            priority_fee_error_margin_bps: 10,
            priority_fee_scoring_start_epoch: 0,
            directed_stake_unstake_cap_bps: 750,
            compute_score_epoch_progress: 0.5,
            undirected_stake_floor_lamports: (10_000_000 * LAMPORTS_PER_SOL).to_le_bytes(),
            _padding_0: [0; 6],
            _padding_1: [0; 28],
            _padding_2: [0; 6],
        };

        // Setup Config
        let config = Config {
            stake_pool: Pubkey::new_unique(),
            parameters,
            paused: false.into(),
            validator_list: Pubkey::new_unique(),
            admin: Pubkey::new_unique(),
            parameters_authority: Pubkey::new_unique(),
            blacklist_authority: Pubkey::new_unique(),
            validator_history_blacklist: LargeBitMask::default(),
            _padding_0: [0u8; 7],
            priority_fee_parameters_authority: Pubkey::new_unique(),
            directed_stake_meta_upload_authority: Pubkey::new_unique(),
            directed_stake_whitelist_authority: Pubkey::new_unique(),
            directed_stake_ticket_override_authority: Pubkey::new_unique(),
            _padding: [0; 888],
        };

        // Setup Sysvars: Clock, EpochSchedule
        let epoch_schedule = EpochSchedule::default();
        let clock = Clock {
            epoch: current_epoch,
            slot: epoch_schedule.get_last_slot_in_epoch(current_epoch),
            ..Clock::default()
        };

        // Setup vote account addresses
        let vote_account_1 = Pubkey::new_unique();
        let vote_account_2 = Pubkey::new_unique();
        let vote_account_3 = Pubkey::new_unique();

        // First one: Good validator
        let mut validator_history_1 = validator_history_default(vote_account_1, 0);
        let mut epoch_credits: Vec<(u64, u64, u64)> = vec![];

        for i in 0..=20 {
            epoch_credits.push((i, (i + 1) * 1000, i * 1000));
            validator_history_1.history.push(ValidatorHistoryEntry {
                epoch: i as u16,
                epoch_credits: 1000 * TVC_MULTIPLIER,
                commission: 0,
                mev_commission: 0,
                is_superminority: 0,
                activated_stake_lamports: 10 * LAMPORTS_PER_SOL,
                vote_account_last_update_slot: epoch_schedule.get_last_slot_in_epoch(i),
                merkle_root_upload_authority: MerkleRootUploadAuthority::TipRouter,
                priority_fee_merkle_root_upload_authority: MerkleRootUploadAuthority::TipRouter,
                ..ValidatorHistoryEntry::default()
            });
        }
        let vote_account_1_state =
            new_vote_state_versions(vote_account_1, vote_account_1, 0, Some(epoch_credits));

        // Second one: Bad validator
        let mut validator_history_2 = validator_history_default(vote_account_2, 1);
        let mut epoch_credits: Vec<(u64, u64, u64)> = vec![];
        for i in 0..=20 {
            epoch_credits.push((i, (i + 1) * 200, i * 200));

            validator_history_2.history.push(ValidatorHistoryEntry {
                epoch: i as u16,
                epoch_credits: 200 * TVC_MULTIPLIER,
                commission: 99,
                mev_commission: 10000,
                is_superminority: 1,
                activated_stake_lamports: 10 * LAMPORTS_PER_SOL,
                vote_account_last_update_slot: epoch_schedule.get_last_slot_in_epoch(i),
                merkle_root_upload_authority: MerkleRootUploadAuthority::TipRouter,
                priority_fee_merkle_root_upload_authority: MerkleRootUploadAuthority::TipRouter,
                ..ValidatorHistoryEntry::default()
            });
        }
        let vote_account_2_state =
            new_vote_state_versions(vote_account_2, vote_account_2, 99, Some(epoch_credits));

        // Third one: Good validator
        let mut validator_history_3 = validator_history_default(vote_account_3, 2);
        let mut epoch_credits: Vec<(u64, u64, u64)> = vec![];
        for i in 0..=20 {
            epoch_credits.push((i, (i + 1) * 1000, i * 1000));

            validator_history_3.history.push(ValidatorHistoryEntry {
                epoch: i as u16,
                epoch_credits: 1000 * TVC_MULTIPLIER,
                commission: 5,
                mev_commission: 500,
                is_superminority: 0,
                activated_stake_lamports: 10 * LAMPORTS_PER_SOL,
                vote_account_last_update_slot: epoch_schedule.get_last_slot_in_epoch(i),
                merkle_root_upload_authority: MerkleRootUploadAuthority::TipRouter,
                priority_fee_merkle_root_upload_authority: MerkleRootUploadAuthority::TipRouter,
                ..ValidatorHistoryEntry::default()
            });
        }
        let vote_account_3_state =
            new_vote_state_versions(vote_account_3, vote_account_3, 5, Some(epoch_credits));

        // Setup ClusterHistory
        let mut cluster_history = cluster_history_default();
        cluster_history.cluster_history_last_update_slot =
            epoch_schedule.get_last_slot_in_epoch(current_epoch);
        for i in 0..=20 {
            cluster_history.history.push(ClusterHistoryEntry {
                epoch: i,
                total_blocks: 1000,
                ..ClusterHistoryEntry::default()
            });
        }

        // Setup ValidatorList
        let mut validator_list = vec![];
        for validator in [
            validator_history_1,
            validator_history_2,
            validator_history_3,
        ] {
            validator_list.push(ValidatorStakeInfo {
                active_stake_lamports: (LAMPORTS_PER_SOL * 1000).into(),
                transient_stake_lamports: 0.into(),
                status: StakeStatus::Active.into(),
                vote_account_address: validator.vote_account,
                ..ValidatorStakeInfo::default()
            });
        }

        let mut validator_lamport_balances = [0; MAX_VALIDATORS];
        validator_lamport_balances[0] = LAMPORTS_PER_SOL * 1000;
        validator_lamport_balances[1] = LAMPORTS_PER_SOL * 1000;
        validator_lamport_balances[2] = LAMPORTS_PER_SOL * 1000;

        // Setup StewardState
        let state = StewardStateV2 {
            state_tag: StewardStateEnum::RebalanceDirected, // Initial state
            validator_lamport_balances,
            scores: [0; MAX_VALIDATORS],
            sorted_score_indices: [SORTED_INDEX_DEFAULT; MAX_VALIDATORS],
            raw_scores: [0; MAX_VALIDATORS],
            sorted_raw_score_indices: [SORTED_INDEX_DEFAULT; MAX_VALIDATORS],
            start_computing_scores_slot: 20, // "Current" slot
            progress: BitMask::default(),
            current_epoch,
            next_cycle_epoch: current_epoch + parameters.num_epochs_between_scoring,
            num_pool_validators: 3,
            scoring_unstake_total: 0,
            instant_unstake_total: 0,
            stake_deposit_unstake_total: 0,
            delegations: [Delegation::default(); MAX_VALIDATORS],
            instant_unstake: BitMask::default(),
            status_flags: 0,
            validators_added: 0,
            validators_to_remove: BitMask::default(),
            validators_for_immediate_removal: BitMask::default(),
            _padding0: [0; 2],
        };

        StateMachineFixtures {
            current_epoch,
            clock,
            epoch_schedule,
            validators: vec![
                validator_history_1,
                validator_history_2,
                validator_history_3,
            ],
            vote_accounts: vec![
                vote_account_1_state,
                vote_account_2_state,
                vote_account_3_state,
            ],
            cluster_history,
            config,
            validator_list,
            state,
        }
    }
}

pub async fn crank_copy_directed_stake_targets(
    fixture: &TestFixture,
    vote_pubkey: Pubkey,
    target_lamports: u64,
) {
    let directed_stake_meta = Pubkey::find_program_address(
        &[
            jito_steward::state::directed_stake::DirectedStakeMeta::SEED,
            fixture.steward_config.pubkey().as_ref(),
        ],
        &jito_steward::id(),
    )
    .0;

    // Find the validator_list_index for this vote_pubkey
    let validator_list: ValidatorList = fixture
        .load_and_deserialize(&fixture.stake_pool_meta.validator_list)
        .await;
    let validator_list_index = validator_list
        .validators
        .iter()
        .position(|v| v.vote_account_address == vote_pubkey)
        .expect("Vote account not found in validator list");

    let ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::CopyDirectedStakeTargets {
            config: fixture.steward_config.pubkey(),
            directed_stake_meta,
            authority: fixture.keypair.pubkey(),
            clock: solana_sdk::sysvar::clock::id(),
            validator_list: fixture.stake_pool_meta.validator_list,
        }
        .to_account_metas(None),
        data: jito_steward::instruction::CopyDirectedStakeTargets {
            vote_pubkey,
            total_target_lamports: target_lamports,
            validator_list_index: validator_list_index as u32,
        }
        .data(),
    };

    let tx = Transaction::new_signed_with_payer(
        &[ix],
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

/// Helper function to deserialize validator list from raw account data
/// This is useful in tests where we don't have AccountInfo
pub fn deserialize_validator_list_from_account_data(
    account_data: &[u8],
) -> Result<ValidatorList, anchor_lang::error::Error> {
    use anchor_lang::AccountDeserialize;
    let mut data = account_data;
    ValidatorList::try_deserialize_unchecked(&mut data)
}

/// Helper function to find validator index by vote account in validator list
pub async fn find_validator_index_in_list(
    fixture: &TestFixture,
    vote_account: &Pubkey,
) -> Option<usize> {
    let validator_list: ValidatorList = fixture
        .load_and_deserialize(&fixture.stake_pool_meta.validator_list)
        .await;
    validator_list
        .validators
        .iter()
        .position(|v| v.vote_account_address == *vote_account)
}

/// Helper function to find validator indices for multiple vote accounts
pub async fn find_validator_indices_in_list(
    fixture: &TestFixture,
    vote_accounts: &[Pubkey],
) -> Vec<usize> {
    let validator_list: ValidatorList = fixture
        .load_and_deserialize(&fixture.stake_pool_meta.validator_list)
        .await;
    vote_accounts
        .iter()
        .filter_map(|vote_account| {
            validator_list
                .validators
                .iter()
                .position(|v| v.vote_account_address == *vote_account)
        })
        .collect()
}

/// Version of crank_directed_stake_permissions that works with real validator list data
/// This function verifies validators exist in the validator list before adding them
pub async fn crank_directed_stake_permissions_with_real_validator_list(
    fixture: &TestFixture,
    extra_validator_accounts: &[ExtraValidatorAccounts],
) {
    use jito_steward::instructions::AuthorityType;
    use jito_steward::state::directed_stake::DirectedStakeRecordType;

    // First, set the whitelist authority and ticket override authority to the signer
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

    // Submit the authority changes first
    let tx = Transaction::new_signed_with_payer(
        &[set_whitelist_auth_ix, set_ticket_override_auth_ix],
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

    // Initialize the directed stake whitelist first (this creates the account)
    let directed_stake_whitelist = Pubkey::find_program_address(
        &[
            jito_steward::state::directed_stake::DirectedStakeWhitelist::SEED,
            fixture.steward_config.pubkey().as_ref(),
        ],
        &jito_steward::id(),
    )
    .0;

    let init_whitelist_ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::InitializeDirectedStakeWhitelist {
            config: fixture.steward_config.pubkey(),
            directed_stake_whitelist,
            system_program: solana_sdk::system_program::id(),
            authority: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::InitializeDirectedStakeWhitelist {}.data(),
    };

    // Submit the whitelist initialization
    let tx = Transaction::new_signed_with_payer(
        &[init_whitelist_ix],
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

    // Now reallocate the directed stake whitelist to proper size
    fixture.realloc_directed_stake_whitelist().await;

    // Deserialize validator list to verify validators exist
    println!("load_and_deserialize validator list");
    let validator_list: ValidatorList = fixture
        .load_and_deserialize(&fixture.stake_pool_meta.validator_list)
        .await;

    // Add all validators to the whitelist
    for extra_accounts in extra_validator_accounts.iter() {
        // Verify the validator exists in the validator list
        let exists = validator_list
            .validators
            .iter()
            .any(|v| v.vote_account_address == extra_accounts.vote_account);

        if !exists {
            panic!(
                "Validator {} not found in validator list",
                extra_accounts.vote_account
            );
        }

        let add_validator_ix = Instruction {
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
                record_type: DirectedStakeRecordType::Validator,
                record: extra_accounts.vote_account,
            }
            .data(),
        };

        let tx = Transaction::new_signed_with_payer(
            &[add_validator_ix],
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

    // Add the caller as a user staker
    let add_staker_ix = Instruction {
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
            record_type: DirectedStakeRecordType::User,
            record: fixture.keypair.pubkey(),
        }
        .data(),
    };

    let tx = Transaction::new_signed_with_payer(
        &[add_staker_ix],
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

pub async fn crank_directed_stake_permissions(
    fixture: &TestFixture,
    extra_validator_accounts: &[ExtraValidatorAccounts],
) {
    use jito_steward::instructions::AuthorityType;
    use jito_steward::state::directed_stake::DirectedStakeRecordType;

    // First, set the whitelist authority and ticket override authority to the signer
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

    // Submit the authority changes first
    let tx = Transaction::new_signed_with_payer(
        &[set_whitelist_auth_ix, set_ticket_override_auth_ix],
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

    // Initialize the directed stake whitelist first (this creates the account)
    let directed_stake_whitelist = Pubkey::find_program_address(
        &[
            jito_steward::state::directed_stake::DirectedStakeWhitelist::SEED,
            fixture.steward_config.pubkey().as_ref(),
        ],
        &jito_steward::id(),
    )
    .0;

    let init_whitelist_ix = Instruction {
        program_id: jito_steward::id(),
        accounts: jito_steward::accounts::InitializeDirectedStakeWhitelist {
            config: fixture.steward_config.pubkey(),
            directed_stake_whitelist,
            system_program: solana_sdk::system_program::id(),
            authority: fixture.keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::InitializeDirectedStakeWhitelist {}.data(),
    };

    // Submit the whitelist initialization
    let tx = Transaction::new_signed_with_payer(
        &[init_whitelist_ix],
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

    // Now reallocate the directed stake whitelist to proper size
    fixture.realloc_directed_stake_whitelist().await;

    // Add all validators to the whitelist
    for extra_accounts in extra_validator_accounts.iter() {
        let add_validator_ix = Instruction {
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
                record_type: DirectedStakeRecordType::Validator,
                record: extra_accounts.vote_account,
            }
            .data(),
        };

        let tx = Transaction::new_signed_with_payer(
            &[add_validator_ix],
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

    // Add the caller as a user staker
    let add_staker_ix = Instruction {
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
            record_type: DirectedStakeRecordType::User,
            record: fixture.keypair.pubkey(),
        }
        .data(),
    };

    let tx = Transaction::new_signed_with_payer(
        &[add_staker_ix],
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
