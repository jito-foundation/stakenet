use anchor_lang::prelude::*;

use crate::{utils::get_config_authority, Config};

#[derive(Accounts)]
pub struct ResumeSteward<'info> {
    #[account(mut)]
    pub config: AccountLoader<'info, Config>,

    #[account(mut, address = get_config_authority(&config)?)]
    pub authority: Signer<'info>,
}

/*
Resumes ability to make progress in state machine
*/
pub fn handler(ctx: Context<ResumeSteward>) -> Result<()> {
    let mut config = ctx.accounts.config.load_mut()?;
    config.set_paused(false);
    Ok(())
}
