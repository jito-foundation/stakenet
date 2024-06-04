use crate::{utils::get_config_authority, Config, UpdateParametersArgs};
use anchor_lang::prelude::*;

#[derive(Accounts)]
pub struct UpdateParameters<'info> {
    #[account(mut)]
    pub config: AccountLoader<'info, Config>,

    #[account(mut, address = get_config_authority(&config)?)]
    pub authority: Signer<'info>,
}

pub fn handler(
    ctx: Context<UpdateParameters>,
    update_parameters_args: &UpdateParametersArgs,
) -> Result<()> {
    let mut config = ctx.accounts.config.load_mut()?;
    let max_slots_in_epoch = EpochSchedule::get()?.slots_per_epoch;
    let current_epoch = Clock::get()?.epoch;

    let new_parameters = config.parameters.get_valid_updated_parameters(
        update_parameters_args,
        current_epoch,
        max_slots_in_epoch,
    )?;

    config.parameters = new_parameters;

    Ok(())
}
