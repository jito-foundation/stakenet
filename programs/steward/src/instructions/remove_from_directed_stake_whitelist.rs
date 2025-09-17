use anchor_lang::prelude::*;

use crate::{
    errors::StewardError, state::directed_stake::DirectedStakeRecordType, Config,
    DirectedStakeWhitelist,
};
use std::mem::size_of;

#[derive(Accounts)]
pub struct RemoveFromDirectedStakeWhitelist<'info> {
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

impl RemoveFromDirectedStakeWhitelist<'_> {
    pub const SIZE: usize = 8 + size_of::<Self>();
}

pub fn handler(
    ctx: Context<RemoveFromDirectedStakeWhitelist>,
    record_type: DirectedStakeRecordType,
    record: Pubkey,
) -> Result<()> {
    let mut whitelist = ctx.accounts.directed_stake_whitelist.load_init()?;

    match record_type {
        DirectedStakeRecordType::Validator => {
            whitelist.remove_validator(&record)?;
        }
        DirectedStakeRecordType::User => {
            whitelist.remove_user_staker(&record)?;
        }
        DirectedStakeRecordType::Protocol => {
            whitelist.remove_protocol_staker(&record)?;
        }
    }

    Ok(())
}
