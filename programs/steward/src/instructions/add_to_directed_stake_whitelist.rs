use anchor_lang::prelude::*;

use crate::{
    errors::StewardError, state::directed_stake::DirectedStakeRecordType, Config,
    DirectedStakeWhitelist,
};
use std::mem::size_of;

#[derive(Accounts)]
pub struct AddToDirectedStakeWhitelist<'info> {
    #[account()]
    pub config: AccountLoader<'info, Config>,

    #[account(
        seeds = [DirectedStakeWhitelist::SEED, config.key().as_ref()],
        bump
    )]
    pub directed_stake_whitelist: AccountLoader<'info, DirectedStakeWhitelist>,

    #[account(
        mut,
        address = config.load()?.directed_stake_whitelist_authority @ StewardError::Unauthorized
    )]
    pub authority: Signer<'info>,
}

impl AddToDirectedStakeWhitelist<'_> {
    pub const SIZE: usize = 8 + size_of::<Self>();
}

pub fn handler(
    ctx: Context<AddToDirectedStakeWhitelist>,
    record_type: DirectedStakeRecordType,
    record: Pubkey,
) -> Result<()> {
    let mut whitelist = ctx.accounts.directed_stake_whitelist.load_mut()?;
    let config = ctx.accounts.config.load()?;
    
    if ctx.accounts.authority.key() != config.directed_stake_whitelist_authority {
        return Err(error!(StewardError::Unauthorized));
    }

    match record_type {
        DirectedStakeRecordType::Validator => {
            whitelist.add_validator(record)?;
        }
        DirectedStakeRecordType::User => {
            whitelist.add_user_staker(record)?;
        }
        DirectedStakeRecordType::Protocol => {
            whitelist.add_protocol_staker(record)?;
        }
    }
    Ok(())
}
