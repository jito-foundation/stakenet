use anchor_lang::prelude::*;

use crate::{errors::StewardError, Config, DirectedStakeMeta};
use std::mem::size_of;

#[derive(Accounts)]
pub struct CloseDirectedStakeMeta<'info> {
    #[account()]
    pub config: AccountLoader<'info, Config>,

    #[account(
        mut,
        close = authority,
        seeds = [DirectedStakeMeta::SEED, config.key().as_ref()],
        bump
    )]
    pub directed_stake_meta: AccountLoader<'info, DirectedStakeMeta>,

    #[account(
        mut,
        address = config.load()?.directed_stake_meta_upload_authority @ StewardError::Unauthorized
    )]
    pub authority: Signer<'info>,
}

impl CloseDirectedStakeMeta<'_> {
    pub const SIZE: usize = 8 + size_of::<Self>();
}

pub fn handler(_ctx: Context<CloseDirectedStakeMeta>) -> Result<()> {
    Ok(())
}
