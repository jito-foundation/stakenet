use anchor_lang::prelude::*;

use crate::{utils::get_config_authority, Config};

#[derive(Accounts)]
pub struct PauseSteward<'info> {
    #[account(mut)]
    pub config: AccountLoader<'info, Config>,

    #[account(mut, address = get_config_authority(&config)?)]
    pub authority: Signer<'info>,
}

/*
Prevents state machine from making progress. All state machine instructions must check for pause flag before running.
*/
pub fn handler(ctx: Context<PauseSteward>) -> Result<()> {
    let mut config = ctx.accounts.config.load_mut()?;
    config.set_paused(true);
    Ok(())
}
