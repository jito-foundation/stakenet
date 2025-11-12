use crate::DirectedStakeWhitelist;
use crate::{
    constants::MAX_ALLOC_BYTES, errors::StewardError, state::Config, utils::get_validator_list,
};
use anchor_lang::prelude::*;

fn is_initialized(account_info: &AccountInfo) -> bool {
    // Checks position of is_initialized byte in account data
    account_info.data_len() >= DirectedStakeWhitelist::SIZE
        && account_info.data.borrow()[DirectedStakeWhitelist::IS_INITIALIZED_BYTE_POSITION] != 0
}

fn get_realloc_size(account_info: &AccountInfo) -> Result<usize> {
    let account_size = account_info.data_len();

    if account_size < DirectedStakeWhitelist::SIZE {
        Ok(DirectedStakeWhitelist::SIZE.min(
            account_size
                .checked_add(MAX_ALLOC_BYTES)
                .ok_or(StewardError::ArithmeticError)?,
        ))
    } else {
        Ok(account_size)
    }
}

#[derive(Accounts)]
pub struct ReallocDirectedStakeWhitelist<'info> {
    #[account(
        mut,
        realloc = get_realloc_size(directed_stake_whitelist.as_ref())?,
        realloc::payer = signer,
        realloc::zero = false,
        seeds = [DirectedStakeWhitelist::SEED, config.key().as_ref()],
        bump
    )]
    pub directed_stake_whitelist: AccountLoader<'info, DirectedStakeWhitelist>,

    pub config: AccountLoader<'info, Config>,

    /// CHECK: We check against the Config
    #[account(address = get_validator_list(&config)?)]
    pub validator_list: AccountInfo<'info>,

    pub system_program: Program<'info, System>,

    #[account(mut)]
    pub signer: Signer<'info>,
}

pub fn handler(ctx: Context<ReallocDirectedStakeWhitelist>) -> Result<()> {
    let account_size = ctx.accounts.directed_stake_whitelist.as_ref().data_len();
    if account_size >= DirectedStakeWhitelist::SIZE
        && !is_initialized(ctx.accounts.directed_stake_whitelist.as_ref())
    {
        let mut whitelist = ctx.accounts.directed_stake_whitelist.load_mut()?;
        whitelist.permissioned_user_stakers =
            [Pubkey::default(); crate::MAX_PERMISSIONED_DIRECTED_STAKERS];
        whitelist.permissioned_protocol_stakers =
            [Pubkey::default(); crate::MAX_PERMISSIONED_DIRECTED_STAKERS];
        whitelist.permissioned_validators =
            [Pubkey::default(); crate::MAX_PERMISSIONED_DIRECTED_VALIDATORS];
        whitelist.total_permissioned_user_stakers = 0;
        whitelist.total_permissioned_protocol_stakers = 0;
        whitelist.total_permissioned_validators = 0;
        whitelist.is_initialized = true.into();
    }
    Ok(())
}
