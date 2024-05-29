use crate::{
    constants::MAX_ALLOC_BYTES,
    state::{Config, StewardStateAccount},
};
use anchor_lang::prelude::*;

#[derive(Accounts)]
pub struct InitializeState<'info> {
    #[account(
        init,
        payer = signer,
        space = MAX_ALLOC_BYTES,
        seeds = [StewardStateAccount::SEED, config.key().as_ref()],
        bump
    )]
    pub state_account: AccountLoader<'info, StewardStateAccount>,

    pub config: AccountLoader<'info, Config>,

    pub system_program: Program<'info, System>,

    #[account(mut)]
    pub signer: Signer<'info>,
}

/*
Initializes steward state account, without assigning any values until it has been reallocated to desired size.
Split into multiple instructions due to 10240 byte allocation limit for PDAs.
*/
pub const fn handler(_ctx: Context<InitializeState>) -> Result<()> {
    Ok(())
}
