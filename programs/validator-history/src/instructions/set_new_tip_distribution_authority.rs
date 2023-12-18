use anchor_lang::prelude::*;

use crate::state::Config;

#[derive(Accounts)]
pub struct SetNewTipDistributionAuthority<'info> {
    #[account(
        mut,
        seeds = [Config::SEED],
        bump = config.bump,
        has_one = tip_distribution_authority,
    )]
    pub config: Account<'info, Config>,
    /// CHECK: fine since we are not deserializing account
    pub new_authority: AccountInfo<'info>,
    pub tip_distribution_authority: Signer<'info>,
}

pub fn handler(ctx: Context<SetNewTipDistributionAuthority>) -> Result<()> {
    ctx.accounts.config.tip_distribution_authority = ctx.accounts.new_authority.key();
    Ok(())
}
