use anchor_lang::prelude::*;

use crate::{
    errors::StewardError, state::directed_stake::DirectedStakeRecordType, Config,
    DirectedStakeWhitelist,
};

#[derive(Accounts)]
pub struct RemoveFromDirectedStakeWhitelist<'info> {
    #[account()]
    pub config: AccountLoader<'info, Config>,

    #[account(
        seeds = [DirectedStakeWhitelist::SEED, config.key().as_ref()],
        bump
    )]
    pub directed_stake_whitelist: AccountLoader<'info, DirectedStakeWhitelist>,

    #[account(mut)]
    pub authority: Signer<'info>,
}

impl RemoveFromDirectedStakeWhitelist<'_> {
    pub const SIZE: usize = 8 + size_of::<Self>();

    pub fn auth(config: &Config, authority_pubkey: &Pubkey) -> Result<()> {
        if config.directed_stake_whitelist_authority == Pubkey::default() {
            msg!("Error: Whitelist authority not initialized in Steward Config");
            return Err(error!(StewardError::WhitelistAuthorityUnset));
        }
        if authority_pubkey != &config.directed_stake_whitelist_authority {
            msg!("Error: directed_stake_whitelist_authority is the only permissioned key for this instruction.");
            return Err(error!(StewardError::Unauthorized));
        }
        Ok(())
    }
}

pub fn handler(
    ctx: Context<RemoveFromDirectedStakeWhitelist>,
    record_type: DirectedStakeRecordType,
    record: Pubkey,
) -> Result<()> {
    let config = ctx.accounts.config.load_init()?;
    RemoveFromDirectedStakeWhitelist::auth(&config, ctx.accounts.authority.key)?;
    let mut whitelist = ctx.accounts.directed_stake_whitelist.load_init()?;

    match record_type {
        DirectedStakeRecordType::Validator => {
            whitelist.remove_validator(&record)?;
        }
        DirectedStakeRecordType::Staker => {
            whitelist.remove_staker(&record)?;
        }
    }

    Ok(())
}
