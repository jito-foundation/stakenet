use anchor_lang::prelude::*;

use crate::state::Config;

#[derive(Accounts)]
pub struct SetNewPriorityFeeDistributionProgram<'info> {
    #[account(
        mut,
        seeds = [Config::SEED],
        bump = config.bump,
        has_one = admin,
    )]
    pub config: Account<'info, Config>,
    /// CHECK: fine since we are not deserializing account
    pub new_priority_fee_distribution_program: AccountInfo<'info>,
    pub admin: Signer<'info>,
}

pub fn handle_set_new_priority_fee_distribution_program(
    ctx: Context<SetNewPriorityFeeDistributionProgram>,
) -> Result<()> {
    ctx.accounts.config.priority_fee_distribution_program =
        ctx.accounts.new_priority_fee_distribution_program.key();
    Ok(())
}
