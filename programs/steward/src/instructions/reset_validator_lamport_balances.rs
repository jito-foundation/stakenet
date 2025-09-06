use crate::{
    constants::{LAMPORT_BALANCE_DEFAULT, MAX_VALIDATORS},
    state::*,
    utils::get_config_admin,
};
use anchor_lang::prelude::*;

#[derive(Accounts)]
pub struct ResetValidatorLamportBalances<'info> {
    #[account(
        mut,
        seeds = [StewardStateAccount::SEED, config.key().as_ref()],
        bump
    )]
    pub steward_state: AccountLoader<'info, StewardStateAccountV2>,

    pub config: AccountLoader<'info, Config>,

    #[account(address = get_config_admin(&config)?)]
    pub authority: Signer<'info>,
}

pub fn handler(ctx: Context<ResetValidatorLamportBalances>) -> Result<()> {
    let state_account = &mut ctx.accounts.steward_state.load_mut()?;

    state_account.state.validator_lamport_balances = [LAMPORT_BALANCE_DEFAULT; MAX_VALIDATORS];

    Ok(())
}
