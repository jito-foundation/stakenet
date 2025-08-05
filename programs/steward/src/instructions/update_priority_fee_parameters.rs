use crate::{
    utils::get_config_priority_fee_parameter_authority, Config, UpdatePriorityFeeParametersArgs,
};
use anchor_lang::prelude::*;

#[derive(Accounts)]
pub struct UpdatePriorityFeeParameters<'info> {
    #[account(mut)]
    pub config: AccountLoader<'info, Config>,

    #[account(address = get_config_priority_fee_parameter_authority(&config)?)]
    pub authority: Signer<'info>,
}

pub fn handler(
    ctx: Context<UpdatePriorityFeeParameters>,
    update_priority_fee_parameters_args: &UpdatePriorityFeeParametersArgs,
) -> Result<()> {
    let mut config = ctx.accounts.config.load_mut()?;
    let max_slots_in_epoch = EpochSchedule::get()?.slots_per_epoch;
    let current_epoch = Clock::get()?.epoch;

    let new_parameters = config.parameters.priority_fee_parameters(
        update_priority_fee_parameters_args,
        current_epoch,
        max_slots_in_epoch,
    )?;

    config.parameters = new_parameters;

    Ok(())
}
