use anchor_lang::prelude::*;
use borsh::{BorshSerialize, BorshDeserialize};
use crate::state::directed_stake::DirectedStakeMeta;
use crate::{constants::MAX_ALLOC_BYTES, errors::StewardError, Config};
use std::mem::size_of;

#[derive(Accounts)]
pub struct InitializeDirectedStakeMeta<'info> {
    #[account()]
    pub config: AccountLoader<'info, Config>,

    #[account(
        init,
        payer = authority,
        space = MAX_ALLOC_BYTES,
        seeds = [DirectedStakeMeta::SEED, config.key().as_ref()],
        bump
    )]
    pub directed_stake_meta: AccountLoader<'info, DirectedStakeMeta>,

    pub clock: Sysvar<'info, Clock>,

    pub system_program: Program<'info, System>,

    #[account(
        mut,
        address = config.load()?.directed_stake_whitelist_authority @ StewardError::Unauthorized
    )]
    pub authority: Signer<'info>,
}

impl InitializeDirectedStakeMeta<'_> {
    pub const SIZE: usize = 8 + size_of::<Self>();
}

#[derive(BorshSerialize, BorshDeserialize)]
pub struct DirectedStakeMetaHeader {
    discriminator: [u8; 8],
    epoch: u64,
    total_stake_targets: u16,
    uploaded_stake_targets: u16,
}

pub fn handler(ctx: Context<InitializeDirectedStakeMeta>, total_stake_targets: u16) -> Result<()> {
    let epoch = ctx.accounts.clock.epoch;
    let total_stake_targets = total_stake_targets as u16;
    let mut stake_meta_data = ctx.accounts.directed_stake_meta.as_ref().try_borrow_mut_data()?;
    let discriminator_bytes: [u8; 8] = DirectedStakeMeta::DISCRIMINATOR.try_into().map_err(|_| error!(StewardError::InvalidParameterValue))?;
    // At the time of initialization we can serialize the header, but not the full account 
    // because the targets array is not fully allocated.
    let header = DirectedStakeMetaHeader {
        discriminator: discriminator_bytes,
        epoch,
        total_stake_targets,
        uploaded_stake_targets: 0,
    };
    header.serialize(&mut *stake_meta_data)?;
    Ok(())
}
