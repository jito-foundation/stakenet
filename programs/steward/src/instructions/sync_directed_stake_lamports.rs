use anchor_lang::prelude::*;

use crate::state::directed_stake::{DirectedStakeMeta};
use crate::utils::get_validator_list;
use crate::utils::vote_pubkey_at_validator_list_index;
use crate::{errors::StewardError, Config};
use spl_stake_pool::state::ValidatorListHeader;
use std::mem::size_of;
use spl_stake_pool::big_vec::BigVec;

#[derive(Accounts)]
pub struct SyncDirectedStakeLamports<'info> {
    #[account()]
    pub config: AccountLoader<'info, Config>,

    #[account(
        mut,
        seeds = [DirectedStakeMeta::SEED, config.key().as_ref()],
        bump
    )]
    pub directed_stake_meta: AccountLoader<'info, DirectedStakeMeta>,

    pub clock: Sysvar<'info, Clock>,

    /// CHECK: Used to get validator_list_index of target
    #[account(
        mut,
        address = get_validator_list(&config)?,
    )]
    pub validator_list: AccountInfo<'info>,

    #[account(
        mut,
        address = config.load()?.directed_stake_meta_upload_authority @ StewardError::Unauthorized
    )]
    pub authority: Signer<'info>,
}

impl SyncDirectedStakeLamports<'_> {
    pub const SIZE: usize = 8 + size_of::<Self>();
}

pub fn sync_directed_stake_lamports(stake_meta: &mut DirectedStakeMeta, validator_list: &BigVec<'_>) -> Result<()> {
    for validator_list_index in 0..validator_list.len() as usize {
        let validator_list_vote_pubkey =
            vote_pubkey_at_validator_list_index(&validator_list, validator_list_index as usize)?;
        
        if stake_meta.directed_stake_meta_indices[validator_list_index] == u64::MAX {
            continue;
        }
        let directed_stake_meta_index = stake_meta.directed_stake_meta_indices[validator_list_index] as usize;
        let directed_stake_meta_vote_pubkey = stake_meta.targets[directed_stake_meta_index].vote_pubkey;
        
        if directed_stake_meta_vote_pubkey != validator_list_vote_pubkey {
            msg!("Warning: Vote pubkey does not match for validator list index: {}, validator list vote pubkey: {}, directed stake meta vote pubkey: {}", validator_list_index, validator_list_vote_pubkey, directed_stake_meta_vote_pubkey);
            continue;
        }

        let target_total_staked_lamports = stake_meta.targets[directed_stake_meta_index].total_staked_lamports;
        stake_meta.directed_stake_lamports[validator_list_index] = target_total_staked_lamports;
        num_targets_synced += 1;
    }
    Ok(())
}

pub fn handler(ctx: Context<SyncDirectedStakeLamports>) -> Result<()> {
    let mut stake_meta = ctx.accounts.directed_stake_meta.load_mut()?;

    let mut validator_list_data = ctx.accounts.validator_list.try_borrow_mut_data()?;
    let (header, validator_list) = ValidatorListHeader::deserialize_vec(&mut validator_list_data)?;
    require!(
        header.account_type == spl_stake_pool::state::AccountType::ValidatorList,
        StewardError::ValidatorListTypeMismatch
    );
    sync_directed_stake_lamports(&mut stake_meta, &validator_list)?;
    Ok(())
}