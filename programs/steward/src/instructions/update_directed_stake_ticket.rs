use anchor_lang::prelude::*;

use crate::{
    errors::StewardError, state::directed_stake::DirectedStakePreference, Config,
    DirectedStakeTicket, DirectedStakeWhitelist,
};
use std::mem::size_of;

#[derive(Accounts)]
pub struct UpdateDirectedStakeTicket<'info> {
    #[account()]
    pub config: AccountLoader<'info, Config>,

    #[account(
        seeds = [DirectedStakeWhitelist::SEED, config.key().as_ref()],
        bump
    )]
    pub whitelist_account: AccountLoader<'info, DirectedStakeWhitelist>,

    #[account(
        mut,
        seeds = [DirectedStakeTicket::SEED, config.key().as_ref(), signer.key().as_ref()],
        bump
    )]
    pub ticket_account: AccountLoader<'info, DirectedStakeTicket>,

    #[account(mut)]
    pub signer: Signer<'info>,
}

impl UpdateDirectedStakeTicket<'_> {
    pub const SIZE: usize = 8 + size_of::<Self>();

    pub fn auth(
        ticket: &DirectedStakeTicket,
        whitelist: &DirectedStakeWhitelist,
        authority_pubkey: &Pubkey,
        preferences: &[DirectedStakePreference],
    ) -> Result<()> {
        if !whitelist.is_staker_permissioned(authority_pubkey) {
            return Err(error!(StewardError::Unauthorized));
        }

        if authority_pubkey != &ticket.ticket_update_authority {
            msg!("Error: Only the ticket update authority can update ticket preferences");
            return Err(error!(StewardError::Unauthorized));
        }

        for preference in preferences {
            if !whitelist.is_validator_permissioned(&preference.vote_pubkey) {
                msg!(
                    "Error: Validator {} is not on the directed stake whitelist",
                    preference.vote_pubkey
                );
                return Err(error!(StewardError::Unauthorized));
            }
        }

        Ok(())
    }
}

pub fn handler(
    ctx: Context<UpdateDirectedStakeTicket>,
    preferences: Vec<DirectedStakePreference>,
) -> Result<()> {
    let whitelist = ctx.accounts.whitelist_account.load()?;
    let mut ticket = ctx.accounts.ticket_account.load_mut()?;

    UpdateDirectedStakeTicket::auth(&ticket, &whitelist, ctx.accounts.signer.key, &preferences)?;

    if preferences.len() > crate::MAX_PREFERENCES_PER_TICKET {
        msg!("Error: Too many preferences provided");
        return Err(error!(StewardError::InvalidParameterValue));
    }

    let total_bps: u32 = preferences
        .iter()
        .map(|pref| pref.stake_share_bps as u32)
        .sum();

    if total_bps > 10_000 {
        msg!("Error: Total stake share basis points cannot exceed 10_000");
        return Err(error!(StewardError::InvalidParameterValue));
    }

    ticket.num_preferences = preferences.len() as u16;
    ticket.staker_preferences =
        [DirectedStakePreference::empty(); crate::MAX_PREFERENCES_PER_TICKET];

    for (i, preference) in preferences.iter().enumerate() {
        if i < crate::MAX_PREFERENCES_PER_TICKET {
            ticket.staker_preferences[i] = *preference;
        }
    }

    Ok(())
}
