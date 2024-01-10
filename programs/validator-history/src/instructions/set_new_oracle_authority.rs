use anchor_lang::prelude::*;

use crate::state::Config;

#[derive(Accounts)]
pub struct SetNewOracleAuthority<'info> {
    #[account(
        mut,
        seeds = [Config::SEED],
        bump = config.bump,
        has_one = admin,
    )]
    pub config: Account<'info, Config>,
    /// CHECK: fine since we are not deserializing account
    pub new_oracle_authority: AccountInfo<'info>,
    pub admin: Signer<'info>,
}

pub fn handler(ctx: Context<SetNewOracleAuthority>) -> Result<()> {
    ctx.accounts.config.oracle_authority = ctx.accounts.new_oracle_authority.key();
    Ok(())
}
