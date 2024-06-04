use std::{cell::RefCell, rc::Rc, str::FromStr, vec};

use crate::spl_stake_pool_cli;
use anchor_lang::{
    prelude::SolanaSysvar,
    solana_program::{
        clock::Clock,
        pubkey::Pubkey,
        vote::state::{VoteInit, VoteState, VoteStateVersions},
    },
    AccountSerialize, AnchorSerialize, Discriminator, InstructionData, ToAccountMetas,
};
use jito_steward::{
    bitmask::BitMask,
    constants::{MAX_VALIDATORS, SORTED_INDEX_DEFAULT, STAKE_POOL_WITHDRAW_SEED},
    utils::StakePool,
    Config, Delegation, Parameters, Staker, StewardState, StewardStateAccount, StewardStateEnum,
    UpdateParametersArgs,
};
use solana_program_test::*;
use solana_sdk::{
    account::Account, epoch_schedule::EpochSchedule, hash::Hash, instruction::Instruction,
    native_token::LAMPORTS_PER_SOL, rent::Rent, signature::Keypair, signer::Signer,
    stake::state::StakeStateV2, transaction::Transaction,
};
use spl_stake_pool::{
    find_stake_program_address, find_transient_stake_program_address,
    state::{Fee, StakeStatus, ValidatorList, ValidatorStakeInfo},
};
use validator_history::{
    self, constants::MAX_ALLOC_BYTES, CircBuf, CircBufCluster, ClusterHistory, ClusterHistoryEntry,
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
    pub staker: Pubkey,
    pub steward_config: Keypair,
    pub steward_state: Pubkey,
    pub cluster_history_account: Pubkey,
    pub validator_history_config: Pubkey,
    pub keypair: Keypair,
}

impl TestFixture {
    pub async fn new() -> Self {
        /*
           Initializes test context with Steward and Stake Pool programs loaded, as well as
           a vote account and a system account for signing transactions.

           Returns a fixture with relevant account addresses and keypairs.
        */

        let mut program = match std::env::var("SBF_OUT_DIR") {
            Ok(_) | Err(_) => {
                let mut program = ProgramTest::new("jito_steward", jito_steward::ID, None);
                program.add_program("spl_stake_pool", spl_stake_pool::id(), None);
                program
            } // Err(_) => {
              //     let mut program = ProgramTest::new(
              //         "jito-steward",
              //         jito_steward::ID,
              //         processor!(jito_steward::entry),
              //     );
              //     program.add_program(
              //         "spl-stake-pool",
              //         spl_stake_pool::id(),
              //         processor!(spl_stake_pool::processor::Processor::process),
              //     );
              //     program
              // }
        };

        let stake_pool_meta = StakePoolMetadata::default();
        let steward_config = Keypair::new();
        let staker = Pubkey::find_program_address(
            &[Staker::SEED, steward_config.pubkey().as_ref()],
            &jito_steward::id(),
        )
        .0;
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
            staker,
            steward_state,
            steward_config,
            validator_history_config,
            cluster_history_account,
            keypair,
        }
    }

    pub async fn load_and_deserialize<T: anchor_lang::AccountDeserialize>(
        &self,
        address: &Pubkey,
    ) -> T {
        let ai = {
            let mut banks_client = self.ctx.borrow_mut().banks_client.clone();
            banks_client.get_account(*address).await.unwrap().unwrap()
        };

        T::try_deserialize(&mut ai.data.as_slice()).unwrap()
    }

    pub async fn get_sysvar<T: SolanaSysvar>(&self) -> T {
        let sysvar = {
            let mut banks_client = self.ctx.borrow_mut().banks_client.clone();
            banks_client.get_sysvar().await.unwrap()
        };

        sysvar
    }

    pub async fn get_account(&self, address: &Pubkey) -> Account {
        let account = {
            let mut banks_client = self.ctx.borrow_mut().banks_client.clone();
            banks_client.get_account(*address).await.unwrap().unwrap()
        };

        account
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

    pub async fn initialize_config(&self, parameters: Option<UpdateParametersArgs>) {
        // Default parameters from JIP
        let update_parameters_args = parameters.unwrap_or(UpdateParametersArgs {
            mev_commission_range: Some(0), // Set to pass validation, where epochs starts at 0
            epoch_credits_range: Some(0),  // Set to pass validation, where epochs starts at 0
            commission_range: Some(0),     // Set to pass validation, where epochs starts at 0
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
            minimum_voting_epochs: Some(0), // Set to pass validation, where epochs starts at 0
        });

        let instruction = Instruction {
            program_id: jito_steward::id(),
            accounts: jito_steward::accounts::InitializeConfig {
                config: self.steward_config.pubkey(),
                stake_pool: self.stake_pool_meta.stake_pool,
                staker: self.staker,
                stake_pool_program: spl_stake_pool::id(),
                system_program: anchor_lang::solana_program::system_program::id(),
                signer: self.keypair.pubkey(),
            }
            .to_account_metas(None),
            data: jito_steward::instruction::InitializeConfig {
                authority: self.keypair.pubkey(),
                update_parameters_args,
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

    pub async fn initialize_steward_state(&self) {
        let instruction = Instruction {
            program_id: jito_steward::id(),
            accounts: jito_steward::accounts::InitializeState {
                state_account: self.steward_state,
                config: self.steward_config.pubkey(),
                system_program: anchor_lang::solana_program::system_program::id(),
                signer: self.keypair.pubkey(),
            }
            .to_account_metas(None),
            data: jito_steward::instruction::InitializeState {}.data(),
        };

        let mut ixs = vec![instruction];

        // Realloc validator history account
        let mut num_reallocs = (StewardStateAccount::SIZE - MAX_ALLOC_BYTES) / MAX_ALLOC_BYTES + 1;

        while num_reallocs > 0 {
            ixs.extend(vec![
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
                };
                num_reallocs.min(10)
            ]);
            let blockhash = {
                let mut banks_client = self.ctx.borrow_mut().banks_client.clone();

                banks_client
                    .get_new_latest_blockhash(&Hash::default())
                    .await
                    .unwrap()
            };
            let transaction = Transaction::new_signed_with_payer(
                &ixs,
                Some(&self.keypair.pubkey()),
                &[&self.keypair],
                blockhash,
            );
            self.submit_transaction_assert_success(transaction).await;
            num_reallocs -= num_reallocs.min(10);
            ixs = vec![];
        }
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
        let stake_pool: StakePool = self
            .load_and_deserialize(&self.stake_pool_meta.stake_pool)
            .await;

        let withdraw_authority = Pubkey::create_program_address(
            &[
                self.stake_pool_meta.stake_pool.as_ref(),
                STAKE_POOL_WITHDRAW_SEED,
                &[stake_pool.as_ref().stake_withdraw_bump_seed],
            ],
            &spl_stake_pool::id(),
        )
        .unwrap();

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
            let mut banks_client = self.ctx.borrow_mut().banks_client.clone();
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
            let mut banks_client = self.ctx.borrow_mut().banks_client.clone();
            banks_client.get_sysvar().await.expect("Failed to get rent")
        };

        rent.minimum_balance(StakeStateV2::size_of())
    }

    pub async fn advance_num_epochs(&self, num_epochs: u64, additional_slots: u64) {
        let clock: Clock = {
            let mut banks_client = self.ctx.borrow_mut().banks_client.clone();
            banks_client
                .get_sysvar()
                .await
                .expect("Failed getting clock")
        };
        let epoch_schedule: EpochSchedule = {
            let mut banks_client = self.ctx.borrow_mut().banks_client.clone();
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

    pub async fn submit_transaction_assert_success(&self, transaction: Transaction) {
        let process_transaction_result = {
            let mut banks_client = self.ctx.borrow_mut().banks_client.clone();
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
            let mut banks_client = self.ctx.borrow_mut().banks_client.clone();
            banks_client
                .process_transaction_with_preflight(transaction)
                .await
        };

        if let Err(e) = process_transaction_result {
            assert!(e.to_string().contains(error_message));
        } else {
            panic!("Error: Transaction succeeded. Expected {}", error_message);
        }
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

// TODO write a function to serialize any account with T: AnchorSerialize
pub fn serialized_validator_list_account(
    validator_list: ValidatorList,
    account_size: Option<usize>,
) -> Account {
    // Passes in size because zeros at the end will be truncated during serialization
    let mut data = vec![];
    validator_list.serialize(&mut data).unwrap();
    let account_size = account_size.unwrap_or(5 + 4 + 73 * validator_list.validators.len());
    data.extend(vec![0; account_size - data.len()]);
    Account {
        lamports: 1_000_000_000,
        data,
        owner: spl_stake_pool::id(),
        ..Account::default()
    }
}

pub fn serialized_stake_pool_account(
    stake_pool: spl_stake_pool::state::StakePool,
    account_size: usize,
) -> Account {
    let mut data = vec![];
    stake_pool.serialize(&mut data).unwrap();
    data.extend(vec![0; account_size - data.len()]);
    Account {
        lamports: 10_000_000_000,
        data,
        owner: spl_stake_pool::id(),
        ..Account::default()
    }
}

pub fn serialized_stake_account(stake_account: StakeStateV2, lamports: u64) -> Account {
    let mut data = vec![];
    stake_account.serialize(&mut data).unwrap();
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
    for byte in ValidatorHistory::discriminator().into_iter().rev() {
        data.insert(0, byte);
    }
    Account {
        lamports: 1_000_000_000,
        data,
        owner: validator_history::id(),
        ..Account::default()
    }
}

pub fn serialized_steward_state_account(state: StewardStateAccount) -> Account {
    let mut data = vec![];
    state.serialize(&mut data).unwrap();
    for byte in StewardStateAccount::discriminator().into_iter().rev() {
        data.insert(0, byte);
    }
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
    for byte in Config::discriminator().into_iter().rev() {
        data.insert(0, byte);
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
        _padding1: [0; 232],
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
    for byte in ClusterHistory::discriminator().into_iter().rev() {
        data.insert(0, byte);
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
    pub cluster_history: ClusterHistory,
    pub config: Config,
    pub validator_list: Vec<ValidatorStakeInfo>,
    pub state: StewardState,
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
            padding0: [0; 6],
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
        };

        // Setup Config
        let config = Config {
            stake_pool: Pubkey::new_unique(),
            authority: Pubkey::new_unique(),
            blacklist: BitMask::default(),
            parameters,
            _padding: [0; 1023],
            paused: false.into(),
        };

        // Setup Sysvars: Clock, EpochSchedule

        let epoch_schedule = EpochSchedule::custom(1000, 1000, false);

        let clock = Clock {
            epoch: current_epoch,
            slot: epoch_schedule.get_last_slot_in_epoch(current_epoch),
            ..Clock::default()
        };

        // Setup ValidatorHistory accounts
        let vote_account_1 = Pubkey::new_unique();
        let vote_account_2 = Pubkey::new_unique();
        let vote_account_3 = Pubkey::new_unique();

        // First one: Good validator
        let mut validator_history_1 = validator_history_default(vote_account_1, 0);
        for i in 0..=20 {
            validator_history_1.history.push(ValidatorHistoryEntry {
                epoch: i,
                epoch_credits: 1000,
                commission: 0,
                mev_commission: 0,
                is_superminority: 0,
                vote_account_last_update_slot: epoch_schedule.get_last_slot_in_epoch(i.into()),
                ..ValidatorHistoryEntry::default()
            });
        }

        // Second one: Bad validator
        let mut validator_history_2 = validator_history_default(vote_account_2, 1);
        for i in 0..=20 {
            validator_history_2.history.push(ValidatorHistoryEntry {
                epoch: i,
                epoch_credits: 200,
                commission: 99,
                mev_commission: 10000,
                is_superminority: 1,
                vote_account_last_update_slot: epoch_schedule.get_last_slot_in_epoch(i.into()),
                ..ValidatorHistoryEntry::default()
            });
        }

        // Third one: Good validator
        let mut validator_history_3 = validator_history_default(vote_account_3, 2);
        for i in 0..=20 {
            validator_history_3.history.push(ValidatorHistoryEntry {
                epoch: i,
                epoch_credits: 1000,
                commission: 5,
                mev_commission: 500,
                is_superminority: 0,
                vote_account_last_update_slot: epoch_schedule.get_last_slot_in_epoch(i.into()),
                ..ValidatorHistoryEntry::default()
            });
        }

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
        let state = StewardState {
            state_tag: StewardStateEnum::ComputeScores, // Initial state
            validator_lamport_balances,
            scores: [0; MAX_VALIDATORS],
            sorted_score_indices: [SORTED_INDEX_DEFAULT; MAX_VALIDATORS],
            yield_scores: [0; MAX_VALIDATORS],
            sorted_yield_score_indices: [SORTED_INDEX_DEFAULT; MAX_VALIDATORS],
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
            compute_delegations_completed: false.into(),
            rebalance_completed: false.into(),
            _padding0: [0; 6 + 8 * MAX_VALIDATORS],
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
            cluster_history,
            config,
            validator_list,
            state,
        }
    }
}
