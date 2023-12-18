use anchor_lang::prelude::*;

use crate::state::Config;

#[derive(Accounts)]
pub struct SetNewTipDistributionProgram<'info> {
    #[account(
        mut,
        seeds = [Config::SEED],
        bump = config.bump,
        has_one = tip_distribution_authority,
    )]
    pub config: Account<'info, Config>,
    /// CHECK: fine since we are not deserializing account
    #[account(executable)]
    pub new_tip_distribution_program: AccountInfo<'info>,
    pub tip_distribution_authority: Signer<'info>,
}

pub fn handler(ctx: Context<SetNewTipDistributionProgram>) -> Result<()> {
    ctx.accounts.config.tip_distribution_program = ctx.accounts.new_tip_distribution_program.key();
    Ok(())
}
