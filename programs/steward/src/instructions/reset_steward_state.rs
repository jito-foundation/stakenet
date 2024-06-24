use crate::{
    constants::{MAX_VALIDATORS, SORTED_INDEX_DEFAULT},
    errors::StewardError,
    state::{Config, StewardStateAccount},
    utils::{deserialize_stake_pool, get_config_authority, get_stake_pool_address},
    BitMask, Delegation, StewardStateEnum, STATE_PADDING_0_SIZE,
};
use anchor_lang::prelude::*;
use spl_stake_pool::state::ValidatorListHeader;

#[derive(Accounts)]
pub struct ResetStewardState<'info> {
    #[account(
        mut,
        seeds = [StewardStateAccount::SEED, config.key().as_ref()],
        bump
    )]
    pub state_account: AccountLoader<'info, StewardStateAccount>,

    pub config: AccountLoader<'info, Config>,

    /// CHECK: Correct account guaranteed if address is correct
    #[account(address = get_stake_pool_address(&config)?)]
    pub stake_pool: AccountInfo<'info>,

    /// CHECK: Correct account guaranteed if address is correct
    #[account(address = deserialize_stake_pool(&stake_pool)?.validator_list)]
    pub validator_list: AccountInfo<'info>,

    #[account(mut, address = get_config_authority(&config)?)]
    pub authority: Signer<'info>,
}

/*
    Resets steward state account to its initial state.
*/
pub fn handler(ctx: Context<ResetStewardState>) -> Result<()> {
    let mut state_account = ctx.accounts.state_account.load_mut()?;

    let clock = Clock::get()?;
    state_account.is_initialized = true.into();
    state_account.bump = ctx.bumps.state_account;

    let config = ctx.accounts.config.load()?;
    let validator_list_data = &mut ctx.accounts.validator_list.try_borrow_mut_data()?;
    let (_, validator_list) = ValidatorListHeader::deserialize_vec(validator_list_data)?;

    state_account.state.state_tag = StewardStateEnum::ComputeScores;
    state_account.state.num_pool_validators = validator_list.len() as u64;
    state_account.state.scores = [0; MAX_VALIDATORS];
    state_account.state.sorted_score_indices = [SORTED_INDEX_DEFAULT; MAX_VALIDATORS];
    state_account.state.yield_scores = [0; MAX_VALIDATORS];
    state_account.state.sorted_yield_score_indices = [SORTED_INDEX_DEFAULT; MAX_VALIDATORS];
    state_account.state.progress = BitMask::default();
    state_account.state.current_epoch = clock.epoch;
    state_account.state.next_cycle_epoch = clock
        .epoch
        .checked_add(config.parameters.num_epochs_between_scoring)
        .ok_or(StewardError::ArithmeticError)?;
    state_account.state.delegations = [Delegation::default(); MAX_VALIDATORS];
    state_account.state.rebalance_completed = false.into();
    state_account.state.instant_unstake = BitMask::default();
    state_account.state.start_computing_scores_slot = clock.slot;
    state_account.state.validators_to_remove = BitMask::default();
    state_account.state.validators_added = 0;
    state_account.state.checked_validators_removed_from_list = false.into();
    state_account.state._padding0 = [0; STATE_PADDING_0_SIZE];
    Ok(())
}
