use anchor_lang::prelude::*;

use crate::{errors::StewardError, Config, DirectedStakeWhitelist};
use std::mem::size_of;

#[derive(Accounts)]
pub struct CloseDirectedStakeWhitelist<'info> {
    #[account()]
    pub config: AccountLoader<'info, Config>,

    #[account(
        mut,
        close = authority,
        seeds = [DirectedStakeWhitelist::SEED, config.key().as_ref()],
        bump
    )]
    pub whitelist_account: AccountLoader<'info, DirectedStakeWhitelist>,

    #[account(mut)]
    pub authority: Signer<'info>,
}

impl CloseDirectedStakeWhitelist<'_> {
    pub const SIZE: usize = 8 + size_of::<Self>();

    pub fn auth(config: &Config, authority_pubkey: &Pubkey) -> Result<()> {
        if config.directed_stake_whitelist_authority == Pubkey::default() {
            msg!("Error: Whitelist authority not initialized in Steward Config");
            return Err(error!(StewardError::WhitelistAuthorityUnset));
        }
        if authority_pubkey != &config.directed_stake_whitelist_authority {
            msg!("Error: Only the directed_stake_whitelist_authority can close the whitelist");
            return Err(error!(StewardError::Unauthorized));
        }
        Ok(())
    }
}

pub fn handler(ctx: Context<CloseDirectedStakeWhitelist>) -> Result<()> {
    let config = ctx.accounts.config.load_init()?;
    CloseDirectedStakeWhitelist::auth(&config, ctx.accounts.authority.key)?;
    Ok(())
}
