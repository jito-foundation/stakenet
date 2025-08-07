use anchor_lang::prelude::*;

use crate::state::Config;

#[derive(Accounts)]
pub struct SetNewPriorityFeeOracleAuthority<'info> {
    #[account(
        mut,
        seeds = [Config::SEED],
        bump = config.bump,
        has_one = admin,
    )]
    pub config: Account<'info, Config>,
    /// CHECK: fine since we trust the admin
    pub new_priority_fee_oracle_authority: AccountInfo<'info>,
    pub admin: Signer<'info>,
}

pub fn handle_set_new_priority_fee_oracle_authority(
    ctx: Context<SetNewPriorityFeeOracleAuthority>,
) -> Result<()> {
    ctx.accounts.config.priority_fee_oracle_authority =
        ctx.accounts.new_priority_fee_oracle_authority.key();
    Ok(())
}
