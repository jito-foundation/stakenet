use anchor_lang::prelude::*;

use crate::{errors::StewardError, state::Config};

#[derive(Accounts)]
pub struct SetNewAuthority<'info> {
    #[account(mut)]
    pub config: AccountLoader<'info, Config>,

    /// CHECK: fine since we are not deserializing account
    pub new_authority: AccountInfo<'info>,

    #[account(mut)]
    pub authority: Signer<'info>,
}

pub fn handler(ctx: Context<SetNewAuthority>) -> Result<()> {
    let mut config = ctx.accounts.config.load_mut()?;
    if config.authority != *ctx.accounts.authority.key {
        return Err(StewardError::Unauthorized.into());
    }

    config.authority = ctx.accounts.new_authority.key();
    Ok(())
}
