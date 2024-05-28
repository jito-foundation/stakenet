use jito_steward::{
    constants::{MAX_VALIDATORS, SORTED_INDEX_DEFAULT},
    BitMask, Config, Delegation, Parameters, StewardState, StewardStateEnum,
};
use solana_sdk::{
    clock::Clock, epoch_schedule::EpochSchedule, native_token::LAMPORTS_PER_SOL, pubkey::Pubkey,
};
use spl_stake_pool::state::{StakeStatus, ValidatorStakeInfo};
use validator_history::{
    ClusterHistory, ClusterHistoryEntry, ValidatorHistory, ValidatorHistoryEntry,
};

use crate::steward_fixtures::{cluster_history_default, validator_history_default};

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
            padding0: [0; 7],
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
