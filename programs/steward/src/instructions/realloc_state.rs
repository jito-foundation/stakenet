use crate::{
    bitmask::BitMask,
    constants::{MAX_ALLOC_BYTES, MAX_VALIDATORS, SORTED_INDEX_DEFAULT},
    errors::StewardError,
    state::{Config, StewardStateAccount},
    utils::get_validator_list,
    Delegation, StewardStateEnum, STATE_PADDING_0_SIZE,
};
use anchor_lang::prelude::*;
use spl_stake_pool::state::ValidatorListHeader;

fn get_realloc_size(account_info: &AccountInfo) -> Result<usize> {
    let account_size = account_info.data_len();

    // If account is already over-allocated, don't try to shrink
    if account_size < StewardStateAccount::SIZE {
        Ok(StewardStateAccount::SIZE.min(
            account_size
                .checked_add(MAX_ALLOC_BYTES)
                .ok_or(StewardError::ArithmeticError)?,
        ))
    } else {
        Ok(account_size)
    }
}

fn is_initialized(account_info: &AccountInfo) -> bool {
    // Checks position of is_initialized byte in account data
    account_info.data_len() >= StewardStateAccount::SIZE
        && account_info.data.borrow()[StewardStateAccount::IS_INITIALIZED_BYTE_POSITION] != 0
}

#[derive(Accounts)]
pub struct ReallocState<'info> {
    #[account(
        mut,
        realloc = get_realloc_size(state_account.as_ref())?,
        realloc::payer = signer,
        realloc::zero = false,
        seeds = [StewardStateAccount::SEED, config.key().as_ref()],
        bump
    )]
    pub state_account: AccountLoader<'info, StewardStateAccount>,

    pub config: AccountLoader<'info, Config>,

    /// CHECK: We check against the Config
    #[account(address = get_validator_list(&config)?)]
    pub validator_list: AccountInfo<'info>,

    pub system_program: Program<'info, System>,

    #[account(mut)]
    pub signer: Signer<'info>,
}

/*
Increases size of delegation account, assigning default values once reached desired size.
*/
pub fn handler(ctx: Context<ReallocState>) -> Result<()> {
    let account_size = ctx.accounts.state_account.as_ref().data_len();
    if account_size >= StewardStateAccount::SIZE
        && !is_initialized(ctx.accounts.state_account.as_ref())
    {
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
        state_account.state.current_epoch = 0; // will be set by epoch_maintenance
        state_account.state.next_cycle_epoch = clock
            .epoch
            .checked_add(config.parameters.num_epochs_between_scoring)
            .ok_or(StewardError::ArithmeticError)?;
        state_account.state.delegations = [Delegation::default(); MAX_VALIDATORS];
        state_account.state.instant_unstake = BitMask::default();
        state_account.state.start_computing_scores_slot = clock.slot;
        state_account.state.validators_to_remove = BitMask::default();
        state_account.state.validators_for_immediate_removal = BitMask::default();
        state_account.state.validators_added = 0;
        state_account.state.clear_flags();
        state_account.state._padding0 = [0; STATE_PADDING_0_SIZE];
    }

    Ok(())
}
