use crate::DirectedStakeMeta;
use crate::{
    constants::MAX_ALLOC_BYTES, errors::StewardError, state::Config, utils::get_validator_list,
};
use anchor_lang::prelude::*;

fn is_initialized(account_info: &AccountInfo) -> bool {
    // Checks position of is_initialized byte in account data
    account_info.data_len() >= DirectedStakeMeta::SIZE
        && account_info.data.borrow()[DirectedStakeMeta::IS_INITIALIZED_BYTE_POSITION] != 0
}

fn get_realloc_size(account_info: &AccountInfo) -> Result<usize> {
    let account_size = account_info.data_len();

    if account_size < DirectedStakeMeta::SIZE {
        Ok(DirectedStakeMeta::SIZE.min(
            account_size
                .checked_add(MAX_ALLOC_BYTES)
                .ok_or(StewardError::ArithmeticError)?,
        ))
    } else {
        Ok(account_size)
    }
}

#[derive(Accounts)]
pub struct ReallocDirectedStakeMeta<'info> {
    #[account(
        mut,
        realloc = get_realloc_size(directed_stake_meta.as_ref())?,
        realloc::payer = signer,
        realloc::zero = false,
        seeds = [DirectedStakeMeta::SEED, config.key().as_ref()],
        bump
    )]
    pub directed_stake_meta: AccountLoader<'info, DirectedStakeMeta>,

    pub config: AccountLoader<'info, Config>,

    /// CHECK: We check against the Config
    #[account(address = get_validator_list(&config)?)]
    pub validator_list: AccountInfo<'info>,

    pub system_program: Program<'info, System>,

    #[account(mut)]
    pub signer: Signer<'info>,
}

pub fn handler(ctx: Context<ReallocDirectedStakeMeta>) -> Result<()> {
    let account_size = ctx.accounts.directed_stake_meta.as_ref().data_len();
    if account_size >= DirectedStakeMeta::SIZE && !is_initialized(ctx.accounts.directed_stake_meta.as_ref()) {
        let mut stake_meta = ctx.accounts.directed_stake_meta.load_mut()?;
        // Initialize the targets array when the account reaches full size
        let default_target = crate::state::directed_stake::DirectedStakeTarget {
            vote_pubkey: Pubkey::default(),
            total_target_lamports: 0,
            total_staked_lamports: 0,
            target_last_updated_epoch: 0,
            staked_last_updated_epoch: 0,
            _padding0: [0; 32],
        };
        stake_meta.targets = [default_target; crate::MAX_PERMISSIONED_DIRECTED_VALIDATORS];
        stake_meta.is_initialized = true.into();
    }
    Ok(())
}
