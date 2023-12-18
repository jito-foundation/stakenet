use anchor_lang::prelude::*;

use crate::state::Config;

#[derive(Accounts)]
pub struct SetNewStakeAuthority<'info> {
    #[account(
        mut,
        seeds = [Config::SEED],
        bump = config.bump,
        has_one = stake_authority,
    )]
    pub config: Account<'info, Config>,
    /// CHECK: fine since we are not deserializing account
    pub new_authority: AccountInfo<'info>,
    pub stake_authority: Signer<'info>,
}

pub fn handler(ctx: Context<SetNewStakeAuthority>) -> Result<()> {
    ctx.accounts.config.stake_authority = ctx.accounts.new_authority.key();
    Ok(())
}
