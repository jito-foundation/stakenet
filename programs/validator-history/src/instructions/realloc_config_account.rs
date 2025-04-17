use crate::{errors::ValidatorHistoryError, Config};
use anchor_lang::{prelude::*, system_program, Discriminator};

#[derive(Accounts)]
pub struct ReallocConfigAccount<'info> {
    #[account(
        mut,
        seeds = [Config::SEED],
        owner = crate::ID,
        bump
    )]
    /// CHECK: Handled with owner and seeds
    pub config_account: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
    #[account(mut)]
    pub payer: Signer<'info>,
}

pub fn handle_realloc_config_account(ctx: Context<ReallocConfigAccount>) -> Result<()> {
    let new_size = Config::SIZE;
    // Block instruction if no size change
    require!(
        new_size != ctx.accounts.config_account.data_len(),
        ValidatorHistoryError::NoReallocNeeded
    );

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

    // Set the priority_fee_oracle_authority if not already set
    let mut config = {
        let data = ctx.accounts.config_account.try_borrow_data()?;
        Config::try_deserialize(&mut &data[..])?
    };
    if config.priority_fee_oracle_authority.eq(&Pubkey::default()) {
        config.priority_fee_oracle_authority = config.oracle_authority;
        let mut data = ctx.accounts.config_account.try_borrow_mut_data()?;
        data[Config::DISCRIMINATOR.len()..].copy_from_slice(&config.try_to_vec()?);
    }

    Ok(())
}
