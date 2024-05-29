#![allow(clippy::redundant_pub_crate)]
use anchor_lang::prelude::*;
use instructions::*;

use spl_stake_pool::instruction::PreferredValidatorType;

mod allocator;
pub mod constants;
pub mod delegation;
pub mod errors;
pub mod instructions;
pub mod score;
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
1) compute_score (once per validator)
2) compute_delegations
3) idle
4) compute_instant_unstake (once per validator)
5) rebalance (once per validator)

For the remaining epochs in a cycle, the state will repeat idle->compute_instant_unstake->rebalance.
After `num_epochs_between_scoring` epochs, the state can transition back to ComputeScores.


If manual intervention is required, the following spl-stake-pool instructions are available, and can be executed by the config.authority:
- `add_validator_to_pool`
- `remove_validator_from_pool`
- `set_preferred_validator`
- `increase_validator_stake`
- `decrease_validator_stake`
- `increase_additional_validator_stake`
- `decrease_additional_validator_stake`
- `redelegate`
- `set_staker`
*/
#[program]
pub mod steward {
    use super::*;

    /* Initialization instructions */

    // Initializes Config and Staker accounts. Must be called before any other instruction
    // Requires Pool to be initialized
    pub fn initialize_config(ctx: Context<InitializeConfig>, authority: Pubkey) -> Result<()> {
        instructions::initialize_config::handler(ctx, authority)
    }

    /// Creates state account
    pub const fn initialize_state(ctx: Context<InitializeState>) -> Result<()> {
        instructions::initialize_state::handler(ctx)
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
        validator_list_index: usize,
    ) -> Result<()> {
        instructions::auto_remove_validator_from_pool::handler(ctx, validator_list_index)
    }

    /// Computes score for a the validator at `validator_list_index` for the current cycle.
    pub fn compute_score(ctx: Context<ComputeScore>, validator_list_index: usize) -> Result<()> {
        instructions::compute_score::handler(ctx, validator_list_index)
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
        validator_list_index: usize,
    ) -> Result<()> {
        instructions::compute_instant_unstake::handler(ctx, validator_list_index)
    }

    /// Increases or decreases stake for a validator at `validator_list_index` to match the target stake,
    /// given constraints on increase/decrease priority, reserve balance, and unstaking caps
    pub fn rebalance(ctx: Context<Rebalance>, validator_list_index: usize) -> Result<()> {
        instructions::rebalance::handler(ctx, validator_list_index)
    }

    /* Admin instructions */

    // If `new_authority` is not a pubkey you own, you cannot regain the authority, but you can
    // use the stake pool manager to set a new staker
    pub fn set_new_authority(ctx: Context<SetNewAuthority>) -> Result<()> {
        instructions::set_new_authority::handler(ctx)
    }

    pub fn pause_steward(ctx: Context<PauseSteward>) -> Result<()> {
        instructions::pause_steward::handler(ctx)
    }

    pub fn resume_steward(ctx: Context<ResumeSteward>) -> Result<()> {
        instructions::resume_steward::handler(ctx)
    }

    /// Adds the validator at `index` to the blacklist. It will be instant unstaked and never receive delegations
    pub fn add_validator_to_blacklist(
        ctx: Context<AddValidatorToBlacklist>,
        index: u32,
    ) -> Result<()> {
        instructions::add_validator_to_blacklist::handler(ctx, index)
    }

    /// Removes the validator at `index` from the blacklist
    pub fn remove_validator_from_blacklist(
        ctx: Context<RemoveValidatorFromBlacklist>,
        index: u32,
    ) -> Result<()> {
        instructions::remove_validator_from_blacklist::handler(ctx, index)
    }

    /// For parameters that are present in args, the instruction checks that they are within sensible bounds and saves them to config struct
    pub fn update_parameters(
        ctx: Context<UpdateParameters>,
        update_parameters_args: UpdateParametersArgs,
    ) -> Result<()> {
        instructions::update_parameters::handler(ctx, &update_parameters_args)
    }

    /* Passthrough instructions to spl-stake-pool, where the signer is Staker. Must be invoked by `config.authority` */

    pub fn set_staker(ctx: Context<SetStaker>) -> Result<()> {
        instructions::spl_passthrough::set_staker_handler(ctx)
    }

    pub fn add_validator_to_pool(
        ctx: Context<AddValidatorToPool>,
        validator_seed: Option<u32>,
    ) -> Result<()> {
        instructions::spl_passthrough::add_validator_to_pool_handler(ctx, validator_seed)
    }

    pub fn remove_validator_from_pool(
        ctx: Context<RemoveValidatorFromPool>,
        validator_list_index: usize,
    ) -> Result<()> {
        instructions::spl_passthrough::remove_validator_from_pool_handler(ctx, validator_list_index)
    }

    pub fn set_preferred_validator(
        ctx: Context<SetPreferredValidator>,
        validator_type: PreferredValidatorType,
        validator: Option<Pubkey>,
    ) -> Result<()> {
        instructions::spl_passthrough::set_preferred_validator_handler(
            ctx,
            validator_type,
            validator,
        )
    }

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
}
