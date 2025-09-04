use crate::{
    state::{Config, StewardStateAccountV2},
    utils::get_config_admin,
};
use anchor_lang::prelude::*;

#[derive(Accounts)]
pub struct CloseStewardAccounts<'info> {
    pub config: AccountLoader<'info, Config>,

    #[account(
        mut,
        close = authority,
        seeds = [StewardStateAccountV2::SEED, config.key().as_ref()],
        bump
    )]
    pub state_account: AccountLoader<'info, StewardStateAccountV2>,

    #[account(mut, address = get_config_admin(&config)?)]
    pub authority: Signer<'info>,
}

/*
    Closes Steward PDA accounts associated with a given Config (StewardStateAccountV2, and Staker).
    Config is not closed as it is a Keypair, so lamports can simply be withdrawn.
    Reclaims lamports to authority
*/
pub const fn handler(_ctx: Context<CloseStewardAccounts>) -> Result<()> {
    Ok(())
}
