use anchor_lang::prelude::*;

use crate::{
    errors::StewardError, Config, DirectedStakePreference, DirectedStakeTicket,
    DirectedStakeWhitelist,
};
use std::mem::size_of;

#[derive(Accounts)]
pub struct CloseDirectedStakeTicket<'info> {
    #[account()]
    pub config: AccountLoader<'info, Config>,

    #[account(
        mut,
        close = authority
    )]
    pub ticket_account: AccountLoader<'info, DirectedStakeTicket>,

    #[account(
        seeds = [DirectedStakeWhitelist::SEED, config.key().as_ref()],
        bump
    )]
    pub whitelist_account: AccountLoader<'info, DirectedStakeWhitelist>,

    #[account(mut)]
    pub authority: Signer<'info>,
}

impl CloseDirectedStakeTicket<'_> {
    pub const SIZE: usize = 8 + size_of::<Self>();

    pub fn auth(
        ticket: &DirectedStakeTicket,
        whitelist: &DirectedStakeWhitelist,
        signer_pubkey: &Pubkey,
        preferences: &[DirectedStakePreference],
        ticket_override_authority: &Pubkey,
    ) -> Result<()> {
        if !whitelist.is_staker_permissioned(signer_pubkey)
            && signer_pubkey != ticket_override_authority
        {
            return Err(error!(StewardError::Unauthorized));
        }

        if signer_pubkey != &ticket.ticket_update_authority
            && signer_pubkey != ticket_override_authority
        {
            msg!("Error: Only a valid ticket authority can close tickets.");
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

pub fn handler(ctx: Context<CloseDirectedStakeTicket>) -> Result<()> {
    let ticket = ctx.accounts.ticket_account.load()?;
    let config = ctx.accounts.config.load()?;
    let whitelist = ctx.accounts.whitelist_account.load()?;

    // Verify the PDA: seeds should be [SEED, config.key(), ticket_update_authority]
    let (expected_ticket_address, _bump) = Pubkey::find_program_address(
        &[
            DirectedStakeTicket::SEED,
            ctx.accounts.config.key().as_ref(),
            ticket.ticket_update_authority.as_ref(),
        ],
        ctx.program_id,
    );
    require_keys_eq!(
        ctx.accounts.ticket_account.key(),
        expected_ticket_address,
        StewardError::Unauthorized
    );

    CloseDirectedStakeTicket::auth(
        &ticket,
        &whitelist,
        ctx.accounts.authority.key,
        &[],
        &config.directed_stake_ticket_override_authority,
    )?;
    Ok(())
}
