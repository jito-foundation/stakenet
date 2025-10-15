use crate::{
    errors::ValidatorHistoryError,
    state::{Config, ValidatorHistory},
};
use anchor_lang::prelude::*;

#[derive(Accounts)]
pub struct UploadValidatorAge<'info> {
    #[account(
        mut,
        seeds = [ValidatorHistory::SEED, vote_account.key().as_ref()],
        bump
    )]
    pub validator_history_account: AccountLoader<'info, ValidatorHistory>,

    /// CHECK: This account may be closed
    pub vote_account: AccountInfo<'info>,

    #[account(
        seeds = [Config::SEED],
        bump = config.bump,
        has_one = oracle_authority
    )]
    pub config: Account<'info, Config>,

    #[account(mut)]
    pub oracle_authority: Signer<'info>,
}

/// Allows the oracle authority to manually set the validator_age field.
pub fn handle_upload_validator_age(
    ctx: Context<UploadValidatorAge>,
    validator_age: u32,
    validator_age_last_updated_epoch: u16,
) -> Result<()> {
    let mut validator_history_account = ctx.accounts.validator_history_account.load_mut()?;

    let clock = Clock::get()?;
    let current_epoch = clock.epoch as u16;

    // Verify the update epoch is not in the future
    if validator_age_last_updated_epoch > current_epoch {
        return Err(ValidatorHistoryError::EpochOutOfRange.into());
    }

    // Update the validator age fields
    validator_history_account.validator_age = validator_age;
    validator_history_account.validator_age_last_updated_epoch = validator_age_last_updated_epoch;

    msg!(
        "Updated validator age to {} at epoch {} for vote account {}",
        validator_age,
        validator_age_last_updated_epoch,
        ctx.accounts.vote_account.key()
    );

    Ok(())
}
