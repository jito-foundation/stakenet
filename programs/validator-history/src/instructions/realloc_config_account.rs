use crate::Config;
use anchor_lang::{prelude::*, system_program};

#[derive(Accounts)]
pub struct ReallocConfigAccount<'info> {
    #[account(
        mut,
        seeds = [Config::SEED],
        owner = crate::ID,
        bump
    )]
    pub config_account: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
    #[account(mut)]
    pub payer: Signer<'info>,
}

pub fn handle_realloc_config_account(ctx: Context<ReallocConfigAccount>) -> Result<()> {
    let new_size = Config::SIZE;
    let current_lamports = ctx.accounts.config_account.lamports();
    let rent = Rent::get()?;
    let new_lamports = rent.minimum_balance(new_size);

    // Transfer lamports if needed (from payer to account)
    if new_lamports > current_lamports {
        let lamports_diff = new_lamports - current_lamports;
        let cpi_accounts = system_program::Transfer {
            from: ctx.accounts.payer.to_account_info(),
            to: ctx.accounts.config_account.to_account_info(),
        };
        let cpi_program = ctx.accounts.system_program.to_account_info();
        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);
        system_program::transfer(cpi_ctx, lamports_diff)?;
    }

    // Call realloc
    ctx.accounts.config_account.realloc(new_size, true)?;

    Ok(())
}
