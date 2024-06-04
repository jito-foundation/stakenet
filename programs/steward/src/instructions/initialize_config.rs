use anchor_lang::{prelude::*, solana_program::program::invoke};

use crate::{utils::StakePool, Config, Staker, UpdateParametersArgs};

#[derive(Accounts)]
pub struct InitializeConfig<'info> {
    #[account(
        init,
        payer = signer,
        space = Config::SIZE,
    )]
    pub config: AccountLoader<'info, Config>,

    // Creates an account that will be used to sign instructions for the stake pool.
    // The pool's "staker" keypair needs to be assigned to this address, and it has authority over
    // adding validators, removing validators, and delegating stake to validators in the pool.
    #[account(
        init,
        seeds = [Staker::SEED, config.key().as_ref()],
        payer = signer,
        space = Staker::SIZE,
        bump
    )]
    pub staker: Account<'info, Staker>,

    #[account(mut)]
    pub stake_pool: Account<'info, StakePool>,

    /// CHECK: CPI program
    #[account(address = spl_stake_pool::ID)]
    pub stake_pool_program: AccountInfo<'info>,

    pub system_program: Program<'info, System>,

    #[account(
        mut,
        address = stake_pool.staker
    )]
    pub signer: Signer<'info>,
}

pub fn handler(
    ctx: Context<InitializeConfig>,
    authority: Pubkey,
    update_parameters_args: &UpdateParametersArgs,
) -> Result<()> {
    let mut config = ctx.accounts.config.load_init()?;
    config.stake_pool = ctx.accounts.stake_pool.key();
    config.authority = authority;

    // Set Initial Parameters
    let max_slots_in_epoch = EpochSchedule::get()?.slots_per_epoch;
    let current_epoch = Clock::get()?.epoch;

    let initial_parameters = config.parameters.get_valid_updated_parameters(
        update_parameters_args,
        current_epoch,
        max_slots_in_epoch,
    )?;

    config.parameters = initial_parameters;

    // Set the staker account
    ctx.accounts.staker.bump = ctx.bumps.staker;
    invoke(
        &spl_stake_pool::instruction::set_staker(
            &ctx.accounts.stake_pool_program.key(),
            &ctx.accounts.stake_pool.key(),
            &ctx.accounts.signer.key(),
            &ctx.accounts.staker.key(),
        ),
        &[
            ctx.accounts.stake_pool.to_account_info(),
            ctx.accounts.signer.to_account_info(),
            ctx.accounts.staker.to_account_info(),
        ],
    )?;
    Ok(())
}
