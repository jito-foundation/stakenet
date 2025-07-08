use anchor_lang::{prelude::*, solana_program::program::invoke};

use crate::{
    constants::MAX_ALLOC_BYTES, utils::deserialize_stake_pool, Config, StewardStateAccount,
    UpdateParametersArgs, UpdatePriorityFeeParametersArgs,
};

#[derive(Accounts)]
pub struct InitializeSteward<'info> {
    #[account(
        init,
        payer = current_staker,
        space = Config::SIZE,
    )]
    pub config: AccountLoader<'info, Config>,

    #[account(
        init,
        payer = current_staker,
        space = MAX_ALLOC_BYTES,
        seeds = [StewardStateAccount::SEED, config.key().as_ref()],
        bump
    )]
    pub state_account: AccountLoader<'info, StewardStateAccount>,

    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(mut)]
    pub stake_pool: AccountInfo<'info>,

    /// CHECK: CPI program
    #[account(address = spl_stake_pool::ID)]
    pub stake_pool_program: AccountInfo<'info>,

    pub system_program: Program<'info, System>,

    #[account(
        mut,
        address = deserialize_stake_pool(&stake_pool)?.staker
    )]
    pub current_staker: Signer<'info>,
}

pub fn handler(
    ctx: Context<InitializeSteward>,
    update_parameters_args: &UpdateParametersArgs,
    update_priority_fee_parameters_args: &UpdatePriorityFeeParametersArgs,
) -> Result<()> {
    // Confirm that the stake pool is valid
    let stake_pool_account = deserialize_stake_pool(&ctx.accounts.stake_pool)?;
    let mut config = ctx.accounts.config.load_init()?;

    // Set the stake pool information
    config.stake_pool = ctx.accounts.stake_pool.key();
    config.validator_list = stake_pool_account.validator_list;

    // Set all authorities to the current_staker
    let admin = ctx.accounts.current_staker.key();
    config.admin = admin;
    config.blacklist_authority = admin;
    config.parameters_authority = admin;

    // Set Initial Parameters
    let max_slots_in_epoch = EpochSchedule::get()?.slots_per_epoch;
    let current_epoch = Clock::get()?.epoch;

    let initial_parameters = config.parameters.get_valid_updated_parameters(
        update_parameters_args,
        current_epoch,
        max_slots_in_epoch,
    )?;

    let initial_parameters = initial_parameters.priority_fee_parameters(
        update_priority_fee_parameters_args,
        current_epoch,
        max_slots_in_epoch,
    )?;

    config.parameters = initial_parameters;

    // The staker is the state account
    invoke(
        &spl_stake_pool::instruction::set_staker(
            &ctx.accounts.stake_pool_program.key(),
            &ctx.accounts.stake_pool.key(),
            &ctx.accounts.current_staker.key(),
            &ctx.accounts.state_account.key(),
        ),
        &[
            ctx.accounts.stake_pool.to_account_info(),
            ctx.accounts.current_staker.to_account_info(),
            ctx.accounts.state_account.to_account_info(),
        ],
    )?;
    Ok(())
}
