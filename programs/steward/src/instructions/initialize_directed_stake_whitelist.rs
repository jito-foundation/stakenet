use anchor_lang::prelude::*;

use crate::{constants::MAX_ALLOC_BYTES, errors::StewardError, Config, DirectedStakeWhitelist};

#[derive(Accounts)]
pub struct InitializeDirectedStakeWhitelist<'info> {
    #[account()]
    pub config: AccountLoader<'info, Config>,

    #[account(
        init,
        payer = authority,
        space = MAX_ALLOC_BYTES,
        seeds = [DirectedStakeWhitelist::SEED, config.key().as_ref()],
        bump
    )]
    pub directed_stake_whitelist: AccountLoader<'info, DirectedStakeWhitelist>,

    pub system_program: Program<'info, System>,

    #[account(mut)]
    pub authority: Signer<'info>,
}

impl InitializeDirectedStakeWhitelist<'_> {
    pub const SIZE: usize = 8 + size_of::<Self>();

    pub fn auth(config: &Config, payer_pubkey: &Pubkey) -> Result<()> {
        if config.directed_stake_whitelist_authority == Pubkey::default() {
            msg!("Error: Whitelist authority not initialized in Steward Config");
            return Err(error!(StewardError::WhitelistAuthorityUnset));
        }
        if payer_pubkey != &config.admin {
            msg!("Error: Admin must initialize whitelist");
            return Err(error!(StewardError::Unauthorized));
        }
        Ok(())
    }
}

pub fn handler(ctx: Context<InitializeDirectedStakeWhitelist>) -> Result<()> {
    let config = ctx.accounts.config.load_init()?;
    InitializeDirectedStakeWhitelist::auth(&config, ctx.accounts.authority.key)?;
    let _ = ctx.accounts.directed_stake_whitelist.load_init()?;
    Ok(())
}
