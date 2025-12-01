use anchor_lang::prelude::*;

use crate::{
    errors::StewardError,
    stake_pool_utils::deserialize_stake_pool,
    state::directed_stake::DirectedStakeRecordType,
    utils::{get_stake_pool_address, validator_exists_in_list},
    Config, DirectedStakeWhitelist,
};
use std::mem::size_of;

#[derive(Accounts)]
pub struct AddToDirectedStakeWhitelist<'info> {
    #[account()]
    pub config: AccountLoader<'info, Config>,

    #[account(
        mut,
        seeds = [DirectedStakeWhitelist::SEED, config.key().as_ref()],
        bump
    )]
    pub directed_stake_whitelist: AccountLoader<'info, DirectedStakeWhitelist>,

    #[account(
        mut,
        address = config.load()?.directed_stake_whitelist_authority @ StewardError::Unauthorized
    )]
    pub authority: Signer<'info>,

    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(address = get_stake_pool_address(&config)?)]
    pub stake_pool: AccountInfo<'info>,

    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(
        mut,
        address = deserialize_stake_pool(&stake_pool)?.validator_list
    )]
    pub validator_list: AccountInfo<'info>,
}

impl AddToDirectedStakeWhitelist<'_> {
    pub const SIZE: usize = 8 + size_of::<Self>();
}

pub fn handler(
    ctx: Context<AddToDirectedStakeWhitelist>,
    record_type: DirectedStakeRecordType,
    record: Pubkey,
) -> Result<()> {
    let mut whitelist = ctx.accounts.directed_stake_whitelist.load_mut()?;

    match record_type {
        DirectedStakeRecordType::Validator => {
            // Ensure the validator exists in the validator list
            let exists = validator_exists_in_list(&ctx.accounts.validator_list, &record)?;
            require!(exists, StewardError::ValidatorNotInList);
            whitelist.add_validator(record)?;
        }
        DirectedStakeRecordType::User => {
            whitelist.add_user_staker(record)?;
        }
        DirectedStakeRecordType::Protocol => {
            whitelist.add_protocol_staker(record)?;
        }
    }
    Ok(())
}
