use anchor_lang::{prelude::*, solana_program::vote};

use crate::{errors::ValidatorHistoryError, state::ValidatorHistory, utils::cast_epoch, Config};

/// Records whether a validator is connected to BAM for a given epoch.
/// Only callable by the oracle authority.
#[derive(Accounts)]
pub struct CopyIsBamConnected<'info> {
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

/// Sets the BAM connection status for a validator at the specified epoch.
/// Accepts only `0` (not connected) or `1` (connected); any other value is rejected.
pub fn handle_copy_is_bam_connected(
    ctx: Context<CopyIsBamConnected>,
    epoch: u64,
    is_bam_connected: u8,
) -> Result<()> {
    require!(
        is_bam_connected == 0 || is_bam_connected == 1,
        ValidatorHistoryError::InvalidBamClientValue
    );

    let mut validator_history_account = ctx.accounts.validator_history_account.load_mut()?;
    let epoch = cast_epoch(epoch)?;

    validator_history_account.set_is_bam_connected(epoch, is_bam_connected)?;

    Ok(())
}
