use crate::{
    state::{Config, StewardStateAccount},
    utils::get_config_authority,
    Staker,
};
use anchor_lang::prelude::*;

#[derive(Accounts)]
pub struct CloseStewardAccounts<'info> {
    pub config: AccountLoader<'info, Config>,

    #[account(
        mut,
        close = authority,
        seeds = [Staker::SEED, config.key().as_ref()],
        bump,
    )]
    staker: Account<'info, Staker>,

    #[account(
        mut,
        close = authority,
        seeds = [StewardStateAccount::SEED, config.key().as_ref()],
        bump
    )]
    pub state_account: AccountLoader<'info, StewardStateAccount>,

    #[account(mut, address = get_config_authority(&config)?)]
    pub authority: Signer<'info>,
}

/*
    Closes Steward PDA accounts associated with a given Config (StewardStateAccount, and Staker).
    Config is not closed as it is a Keypair, so lamports can simply be withdrawn.
    Reclaims lamports to authority
*/
pub const fn handler(_ctx: Context<CloseStewardAccounts>) -> Result<()> {
    Ok(())
}
