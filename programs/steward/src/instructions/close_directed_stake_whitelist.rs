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

    #[account(
        mut,
        address = config.load()?.directed_stake_whitelist_authority @ StewardError::Unauthorized
    )]
    pub authority: Signer<'info>,
}

impl CloseDirectedStakeWhitelist<'_> {
    pub const SIZE: usize = 8 + size_of::<Self>();
}

pub fn handler(_ctx: Context<CloseDirectedStakeWhitelist>) -> Result<()> {
    Ok(())
}
