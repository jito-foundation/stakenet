use crate::{utils::get_config_blacklist_authority, Config};
use anchor_lang::prelude::*;

#[derive(Accounts)]
pub struct RemoveValidatorsFromBlacklist<'info> {
    #[account(mut)]
    pub config: AccountLoader<'info, Config>,

    #[account(mut, address = get_config_blacklist_authority(&config)?)]
    pub authority: Signer<'info>,
}

// Removes validator from blacklist. Validator will be eligible to receive delegation again when scores are recomputed.
// Index is the index of the validator from ValidatorHistory.
pub fn handler(
    ctx: Context<RemoveValidatorsFromBlacklist>,
    validator_history_indices: &[u32],
) -> Result<()> {
    let mut config = ctx.accounts.config.load_mut()?;
    for index in validator_history_indices {
        config.validator_history_blacklist.set(*index as usize, false)?;
    }
    Ok(())
}
