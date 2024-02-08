use anchor_lang::prelude::*;

use crate::state::Config;

#[derive(Accounts)]
pub struct SetNewAdmin<'info> {
    #[account(
        mut,
        seeds = [Config::SEED],
        bump = config.bump,
        has_one = admin,
    )]
    pub config: Account<'info, Config>,
    /// CHECK: fine since we are not deserializing account
    pub new_admin: AccountInfo<'info>,
    pub admin: Signer<'info>,
}

pub fn handle_set_new_admin(ctx: Context<SetNewAdmin>) -> Result<()> {
    ctx.accounts.config.admin = ctx.accounts.new_admin.key();
    Ok(())
}
