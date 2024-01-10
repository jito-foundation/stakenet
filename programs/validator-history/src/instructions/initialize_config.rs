use anchor_lang::prelude::*;

use crate::Config;

#[derive(Accounts)]
pub struct InitializeConfig<'info> {
    #[account(
        init,
        payer = signer,
        space = Config::SIZE,
        seeds = [Config::SEED],
        bump,
    )]
    pub config: Account<'info, Config>,
    pub system_program: Program<'info, System>,
    #[account(mut)]
    pub signer: Signer<'info>,
}

pub fn handler(ctx: Context<InitializeConfig>, authority: Pubkey) -> Result<()> {
    ctx.accounts.config.oracle_authority = authority;
    ctx.accounts.config.admin = authority;
    ctx.accounts.config.bump = *ctx.bumps.get("config").unwrap();
    ctx.accounts.config.counter = 0;
    Ok(())
}
