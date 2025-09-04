use anchor_lang::prelude::*;

use crate::{utils::get_config_admin, Config, StewardStateAccountV2};

#[derive(Accounts)]
pub struct AdminMarkForRemoval<'info> {
    #[account(mut)]
    pub config: AccountLoader<'info, Config>,

    #[account(
        mut,
        seeds = [StewardStateAccountV2::SEED, config.key().as_ref()],
        bump
    )]
    pub state_account: AccountLoader<'info, StewardStateAccountV2>,

    #[account(mut, address = get_config_admin(&config)?)]
    pub authority: Signer<'info>,
}

/*
Used by the admin to unstick the machine
*/
pub fn handler(
    ctx: Context<AdminMarkForRemoval>,
    validator_list_index: usize,
    mark_for_removal: bool,
    immediate: bool,
) -> Result<()> {
    let mut state = ctx.accounts.state_account.load_mut()?;

    if immediate {
        state
            .state
            .validators_for_immediate_removal
            .set(validator_list_index, mark_for_removal)?;
    } else {
        state
            .state
            .validators_to_remove
            .set(validator_list_index, mark_for_removal)?;
    }

    Ok(())
}
