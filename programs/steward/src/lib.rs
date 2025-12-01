#![allow(unexpected_cfgs)]
#![allow(deprecated)]
#![allow(clippy::redundant_pub_crate)]
use anchor_lang::prelude::*;
#[cfg(feature = "idl-build")]
use anchor_lang::IdlBuild;
use instructions::*;

use crate::stake_pool_utils::PreferredValidatorType;

mod allocator;
pub mod constants;
pub mod delegation;
pub mod directed_delegation;
pub mod errors;
pub mod events;
pub mod instructions;
pub mod score;
pub mod stake_pool_utils;
pub mod state;
pub mod utils;

pub use state::*;

declare_id!("Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8");

/*
This program manages the selection of validators and delegation of stake for a SPL Stake Pool.

It relies on validator metrics collected by the Validator History Program.

To initialize a Steward-managed pool:
1) `initialize_config` - creates Config account, and transfers ownership of the pool's staker authority to the Staker PDA
2) `initialize_state` - creates State account
3) `realloc_state` - increases the size of the State account to StewardStateAccount::SIZE, and initializes values once at that size

Each cycle, the following steps are performed by a permissionless cranker:
x) epoch_maintenance ( once per epoch )
1) compute_score ( once per validator )
2) compute_delegations
3) idle
4) compute_instant_unstake ( once per validator )
5) rebalance ( once per validator )

For the remaining epochs in a cycle, the state will repeat idle->compute_instant_unstake->rebalance.
After `num_epochs_between_scoring` epochs, the state can transition back to ComputeScores.

To manage the validators in the pool, there are the following permissionless instructions:
- `auto_add_validator_to_pool`
- `auto_remove_validator_from_pool`
- `instant_remove_validator` - called when a validator can be removed within the same epoch it was marked for removal

There are three authorities within the program:
- `admin` - can update authority, pause, resume, and reset state
- `parameters_authority` - can update parameters
- `blacklist_authority` - can add and remove validators from the blacklist

If manual intervention is required, the following spl-stake-pool instructions are available, and can be executed by the config.authority:
- `add_validator_to_pool`
- `remove_validator_from_pool`
- `set_preferred_validator`
- `increase_validator_stake`
- `decrease_validator_stake`
- `increase_additional_validator_stake`
- `decrease_additional_validator_stake`
- `set_staker`
*/
#[program]
pub mod steward {

    use super::*;

    /* Initialization instructions */

    // Initializes Config and Staker accounts. Must be called before any other instruction
    // Requires Pool to be initialized
    pub fn initialize_steward(
        ctx: Context<InitializeSteward>,
        update_parameters_args: UpdateParametersArgs,
        update_priority_fee_parameters_args: UpdatePriorityFeeParametersArgs,
    ) -> Result<()> {
        instructions::initialize_steward::handler(
            ctx,
            &update_parameters_args,
            &update_priority_fee_parameters_args,
        )
    }

    /// Increases state account by 10KiB each ix until it reaches StewardStateAccount::SIZE
    pub fn realloc_state(ctx: Context<ReallocState>) -> Result<()> {
        instructions::realloc_state::handler(ctx)
    }

    /* Main cycle loop */

    /// Adds a validator to the pool if it has a validator history account, matches stake_minimum, and is not yet in the pool
    pub fn auto_add_validator_to_pool(ctx: Context<AutoAddValidator>) -> Result<()> {
        instructions::auto_add_validator_to_pool::handler(ctx)
    }

    /// Removes a validator from the pool if its stake account is inactive or the vote account has closed
    pub fn auto_remove_validator_from_pool(
        ctx: Context<AutoRemoveValidator>,
        validator_list_index: u64,
    ) -> Result<()> {
        instructions::auto_remove_validator_from_pool::handler(ctx, validator_list_index as usize)
    }

    /// When a validator is marked for immediate removal, it needs to be removed before normal functions can continue
    pub fn instant_remove_validator(
        ctx: Context<InstantRemoveValidator>,
        validator_index_to_remove: u64,
    ) -> Result<()> {
        instructions::instant_remove_validator::handler(ctx, validator_index_to_remove as usize)
    }

    /// Housekeeping, run at the start of any new epoch before any other instructions
    pub fn epoch_maintenance(
        ctx: Context<EpochMaintenance>,
        validator_index_to_remove: Option<u64>,
    ) -> Result<()> {
        instructions::epoch_maintenance::handler(ctx, validator_index_to_remove.map(|x| x as usize))
    }

    /// Computes score for a the validator at `validator_list_index` for the current cycle.
    pub fn compute_score(ctx: Context<ComputeScore>, validator_list_index: u64) -> Result<()> {
        instructions::compute_score::handler(ctx, validator_list_index as usize)
    }

    /// Computes delegation for a validator for the current cycle.
    /// All validators must have delegations computed before stake can be delegated
    pub fn compute_delegations(ctx: Context<ComputeDelegations>) -> Result<()> {
        instructions::compute_delegations::handler(ctx)
    }

    /// Idle state, waiting for epoch progress before transitioning to next state
    pub fn idle(ctx: Context<Idle>) -> Result<()> {
        instructions::idle::handler(ctx)
    }

    /// Checks if a validator at `validator_list_index` should be instant unstaked, and marks it if so
    pub fn compute_instant_unstake(
        ctx: Context<ComputeInstantUnstake>,
        validator_list_index: u64,
    ) -> Result<()> {
        instructions::compute_instant_unstake::handler(ctx, validator_list_index as usize)
    }

    /// Increases or decreases stake for a validator at `validator_list_index` to match the target stake,
    /// given constraints on increase/decrease priority, reserve balance, and unstaking caps
    pub fn rebalance(ctx: Context<Rebalance>, validator_list_index: u64) -> Result<()> {
        instructions::rebalance::handler(ctx, validator_list_index as usize)
    }

    /// Increases or decreases stake for a validator at `validator_list_index` using directed stake targets
    pub fn rebalance_directed(
        ctx: Context<RebalanceDirected>,
        directed_stake_meta_index: u64,
    ) -> Result<()> {
        instructions::rebalance_directed::handler(ctx, directed_stake_meta_index as usize)
    }

    /* Admin instructions */

    // If `new_authority` is not a pubkey you own, you cannot regain the authority, but you can
    // use the stake pool manager to set a new staker
    pub fn set_new_authority(
        ctx: Context<SetNewAuthority>,
        authority_type: AuthorityType,
    ) -> Result<()> {
        instructions::set_new_authority::handler(ctx, authority_type)
    }

    /// Pauses the steward, preventing any further state transitions
    pub fn pause_steward(ctx: Context<PauseSteward>) -> Result<()> {
        instructions::pause_steward::handler(ctx)
    }

    /// Resumes the steward, allowing state transitions to continue
    pub fn resume_steward(ctx: Context<ResumeSteward>) -> Result<()> {
        instructions::resume_steward::handler(ctx)
    }

    /// Adds the validators to the blacklist. They will be instant unstaked and never receive delegations. Each u32 is a ValidatorHistory index.
    pub fn add_validators_to_blacklist(
        ctx: Context<AddValidatorsToBlacklist>,
        validator_history_blacklist: Vec<u32>,
    ) -> Result<()> {
        instructions::add_validators_to_blacklist::handler(ctx, &validator_history_blacklist)
    }

    /// Removes the validators from the blacklist. Each u32 is a ValidatorHistory index.
    pub fn remove_validators_from_blacklist(
        ctx: Context<RemoveValidatorsFromBlacklist>,
        validator_history_blacklist: Vec<u32>,
    ) -> Result<()> {
        instructions::remove_validators_from_blacklist::handler(ctx, &validator_history_blacklist)
    }

    /// For parameters that are present in args, the instruction checks that they are within sensible bounds and saves them to config struct
    pub fn update_parameters(
        ctx: Context<UpdateParameters>,
        update_parameters_args: UpdateParametersArgs,
    ) -> Result<()> {
        instructions::update_parameters::handler(ctx, &update_parameters_args)
    }

    /// Resets steward state account to its initial state.
    pub fn reset_steward_state(ctx: Context<ResetStewardState>) -> Result<()> {
        instructions::reset_steward_state::handler(ctx)
    }

    /// Admin to mark or unmark validator for removal and unstuck the machine
    pub fn admin_mark_for_removal(
        ctx: Context<AdminMarkForRemoval>,
        validator_list_index: u64,
        mark_for_removal: u8,
        immediate: u8,
    ) -> Result<()> {
        instructions::admin_mark_for_removal::handler(
            ctx,
            validator_list_index as usize,
            mark_for_removal != 0,
            immediate != 0,
        )
    }

    /// Reset validator_lamport_balances to default
    pub fn reset_validator_lamport_balances(
        ctx: Context<ResetValidatorLamportBalances>,
    ) -> Result<()> {
        instructions::reset_validator_lamport_balances::handler(ctx)
    }

    /// Closes Steward PDA accounts associated with a given Config (StewardStateAccount, and Staker).
    /// Config is not closed as it is a Keypair, so lamports can simply be withdrawn.
    /// Reclaims lamports to authority
    pub fn close_steward_accounts(ctx: Context<CloseStewardAccounts>) -> Result<()> {
        instructions::close_steward_accounts::handler(ctx)
    }

    /// Migrates the state account from V1 (u32 scores) to V2 (u64 scores with 4-tier encoding)
    pub fn migrate_state_to_v2(ctx: Context<MigrateStateToV2>) -> Result<()> {
        instructions::migrate_state_to_v2::handler(ctx)
    }

    /* Passthrough instructions */
    /* passthrough to spl-stake-pool, where the signer is Staker. Must be invoked by `config.authority` */

    /// Passthrough spl-stake-pool: Set the staker for the pool
    pub fn set_staker(ctx: Context<SetStaker>) -> Result<()> {
        instructions::spl_passthrough::set_staker_handler(ctx)
    }

    /// Passthrough spl-stake-pool: Add a validator to the pool
    pub fn add_validator_to_pool(
        ctx: Context<AddValidatorToPool>,
        validator_seed: Option<u32>,
    ) -> Result<()> {
        instructions::spl_passthrough::add_validator_to_pool_handler(ctx, validator_seed)
    }

    /// Passthrough spl-stake-pool: Remove a validator from the pool
    pub fn remove_validator_from_pool(
        ctx: Context<RemoveValidatorFromPool>,
        validator_list_index: u64,
    ) -> Result<()> {
        instructions::spl_passthrough::remove_validator_from_pool_handler(
            ctx,
            validator_list_index as usize,
        )
    }

    /// Passthrough spl-stake-pool: Set the preferred validator
    pub fn set_preferred_validator(
        ctx: Context<SetPreferredValidator>,
        validator_type: PreferredValidatorType,
        validator: Option<Pubkey>,
    ) -> Result<()> {
        instructions::spl_passthrough::set_preferred_validator_handler(
            ctx,
            validator_type.as_ref(),
            validator,
        )
    }

    /// Passthrough spl-stake-pool: Increase validator stake
    pub fn increase_validator_stake(
        ctx: Context<IncreaseValidatorStake>,
        lamports: u64,
        transient_seed: u64,
    ) -> Result<()> {
        instructions::spl_passthrough::increase_validator_stake_handler(
            ctx,
            lamports,
            transient_seed,
        )
    }

    /// Passthrough spl-stake-pool: Decrease validator stake
    pub fn decrease_validator_stake(
        ctx: Context<DecreaseValidatorStake>,
        lamports: u64,
        transient_seed: u64,
    ) -> Result<()> {
        instructions::spl_passthrough::decrease_validator_stake_handler(
            ctx,
            lamports,
            transient_seed,
        )
    }

    /// Passthrough spl-stake-pool: Increase additional validator stake
    pub fn increase_additional_validator_stake(
        ctx: Context<IncreaseAdditionalValidatorStake>,
        lamports: u64,
        transient_seed: u64,
        ephemeral_seed: u64,
    ) -> Result<()> {
        instructions::spl_passthrough::increase_additional_validator_stake_handler(
            ctx,
            lamports,
            transient_seed,
            ephemeral_seed,
        )
    }

    /// Passthrough spl-stake-pool: Decrease additional validator stake
    pub fn decrease_additional_validator_stake(
        ctx: Context<DecreaseAdditionalValidatorStake>,
        lamports: u64,
        transient_seed: u64,
        ephemeral_seed: u64,
    ) -> Result<()> {
        instructions::spl_passthrough::decrease_additional_validator_stake_handler(
            ctx,
            lamports,
            transient_seed,
            ephemeral_seed,
        )
    }

    /// For priority fee parameters that are present in args, the instruction checks that they
    /// are within sensible bounds and saves them to config struct
    pub fn update_priority_fee_parameters(
        ctx: Context<UpdatePriorityFeeParameters>,
        update_priority_fee_parameters_args: UpdatePriorityFeeParametersArgs,
    ) -> Result<()> {
        instructions::update_priority_fee_parameters::handler(
            ctx,
            &update_priority_fee_parameters_args,
        )
    }

    /* Directed Stake Instructions */

    /// Initialize DirectedStakeMeta account
    pub fn initialize_directed_stake_meta(ctx: Context<InitializeDirectedStakeMeta>) -> Result<()> {
        instructions::initialize_directed_stake_meta::handler(ctx)
    }

    /// Copy directed stake targets to the meta account
    pub fn copy_directed_stake_targets(
        ctx: Context<CopyDirectedStakeTargets>,
        vote_pubkey: Pubkey,
        total_target_lamports: u64,
        validator_list_index: u32,
    ) -> Result<()> {
        instructions::copy_directed_stake_targets::handler(
            ctx,
            vote_pubkey,
            total_target_lamports,
            validator_list_index as usize,
        )
    }

    /// Initialize DirectedStakeWhitelist account
    pub fn initialize_directed_stake_whitelist(
        ctx: Context<InitializeDirectedStakeWhitelist>,
    ) -> Result<()> {
        instructions::initialize_directed_stake_whitelist::handler(ctx)
    }

    /// Initialize DirectedStakeTicket account
    pub fn initialize_directed_stake_ticket(
        ctx: Context<InitializeDirectedStakeTicket>,
        ticket_update_authority: Pubkey,
        ticket_holder_is_protocol: bool,
    ) -> Result<()> {
        instructions::initialize_directed_stake_ticket::handler(
            ctx,
            ticket_update_authority,
            ticket_holder_is_protocol,
        )
    }

    /// Reallocate DirectedStakeMeta account to proper size
    pub fn realloc_directed_stake_meta(ctx: Context<ReallocDirectedStakeMeta>) -> Result<()> {
        instructions::realloc_directed_stake_meta::handler(ctx)
    }

    /// Reallocate DirectedStakeWhitelist account to proper size
    pub fn realloc_directed_stake_whitelist(
        ctx: Context<ReallocDirectedStakeWhitelist>,
    ) -> Result<()> {
        instructions::realloc_directed_stake_whitelist::handler(ctx)
    }

    /// Add staker or validator to DirectedStakeWhitelist
    pub fn add_to_directed_stake_whitelist(
        ctx: Context<AddToDirectedStakeWhitelist>,
        record_type: DirectedStakeRecordType,
        record: Pubkey,
    ) -> Result<()> {
        instructions::add_to_directed_stake_whitelist::handler(ctx, record_type, record)
    }

    /// Remove staker or validator from DirectedStakeWhitelist
    pub fn remove_from_directed_stake_whitelist(
        ctx: Context<RemoveFromDirectedStakeWhitelist>,
        record_type: DirectedStakeRecordType,
        record: Pubkey,
    ) -> Result<()> {
        instructions::remove_from_directed_stake_whitelist::handler(ctx, record_type, record)
    }

    /// Update DirectedStakeTicket preferences
    pub fn update_directed_stake_ticket(
        ctx: Context<UpdateDirectedStakeTicket>,
        preferences: Vec<DirectedStakePreference>,
    ) -> Result<()> {
        instructions::update_directed_stake_ticket::handler(ctx, preferences)
    }

    /// Close DirectedStakeTicket account
    pub fn close_directed_stake_ticket(ctx: Context<CloseDirectedStakeTicket>) -> Result<()> {
        instructions::close_directed_stake_ticket::handler(ctx)
    }

    /// Close DirectedStakeWhitelist account
    pub fn close_directed_stake_whitelist(ctx: Context<CloseDirectedStakeWhitelist>) -> Result<()> {
        instructions::close_directed_stake_whitelist::handler(ctx)
    }
}
