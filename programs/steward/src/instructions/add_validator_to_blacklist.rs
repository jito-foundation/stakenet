use crate::{utils::get_config_blacklist_authority, Config};
use anchor_lang::prelude::*;

#[derive(Accounts)]
pub struct AddValidatorToBlacklist<'info> {
    #[account(mut)]
    pub config: AccountLoader<'info, Config>,

    #[account(mut, address = get_config_blacklist_authority(&config)?)]
    pub authority: Signer<'info>,
}

// Removes ability for validator to receive delegation. Score will be set to 0 and instant unstaking will occur.
// Index is the index of the validator from ValidatorHistory.
pub fn handler(ctx: Context<AddValidatorToBlacklist>, validator_history_index: u32) -> Result<()> {
    let mut config = ctx.accounts.config.load_mut()?;
    config
        .validator_history_blacklist
        .set(validator_history_index as usize, true)?;
    Ok(())
}
