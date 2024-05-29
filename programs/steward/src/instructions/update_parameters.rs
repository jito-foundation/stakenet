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
    let mut parameters = ctx.accounts.config.load_mut()?.parameters;
    let max_slots_in_epoch = EpochSchedule::get()?.slots_per_epoch;
    let current_epoch = Clock::get()?.epoch;
    parameters.update(update_parameters_args, current_epoch, max_slots_in_epoch)?;
    Ok(())
}
