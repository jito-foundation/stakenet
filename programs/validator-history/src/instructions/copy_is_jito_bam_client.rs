use anchor_lang::{
    prelude::*,
    solana_program::{clock::Clock, vote},
};

use crate::{errors::ValidatorHistoryError, state::ValidatorHistory, utils::cast_epoch, Config};

#[derive(Accounts)]
pub struct CopyIsJitoBamClient<'info> {
    #[account(
        seeds = [Config::SEED],
        bump = config.bump,
        has_one = oracle_authority
    )]
    pub config: Account<'info, Config>,

    #[account(
        mut,
        seeds = [ValidatorHistory::SEED, vote_account.key().as_ref()],
        bump,
        has_one = vote_account
    )]
    pub validator_history_account: AccountLoader<'info, ValidatorHistory>,

    /// CHECK: Safe because we check the vote program is the owner before reading bytes.
    #[account(owner = vote::program::ID.key())]
    pub vote_account: AccountInfo<'info>,

    pub oracle_authority: Signer<'info>,
}

pub fn handle_copy_is_jito_bam_client(
    ctx: Context<CopyIsJitoBamClient>,
    is_jito_bam_client: u8,
) -> Result<()> {
    require!(
        is_jito_bam_client == 0 || is_jito_bam_client == 1,
        ValidatorHistoryError::InvalidBamClientValue
    );

    let mut validator_history_account = ctx.accounts.validator_history_account.load_mut()?;
    let clock = Clock::get()?;
    let epoch = cast_epoch(clock.epoch)?;

    validator_history_account.set_is_jito_bam_client(epoch, is_jito_bam_client)?;

    Ok(())
}
