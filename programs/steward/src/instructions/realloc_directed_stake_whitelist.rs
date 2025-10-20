use crate::DirectedStakeWhitelist;
use crate::{
    constants::MAX_ALLOC_BYTES,
    errors::StewardError,
    state::{Config, StewardStateAccount},
    utils::get_validator_list,
};
use anchor_lang::prelude::*;

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

    #[account(
        mut,
        address = config.load()?.directed_stake_whitelist_authority @ StewardError::Unauthorized
    )]
    pub signer: Signer<'info>,
}

pub fn handler(ctx: Context<ReallocDirectedStakeWhitelist>) -> Result<()> {
    let account_size = ctx.accounts.directed_stake_whitelist.as_ref().data_len();
    if account_size >= DirectedStakeWhitelist::SIZE {
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
    }
    Ok(())
}