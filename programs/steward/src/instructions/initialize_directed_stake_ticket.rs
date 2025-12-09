use anchor_lang::prelude::*;

use crate::{
    errors::StewardError, state::directed_stake::DirectedStakePreference, utils::U8Bool, Config,
    DirectedStakeTicket, DirectedStakeWhitelist,
};
use std::mem::size_of;

#[derive(Accounts)]
#[instruction(ticket_update_authority: Pubkey)]
pub struct InitializeDirectedStakeTicket<'info> {
    #[account()]
    pub config: AccountLoader<'info, Config>,

    #[account(
        seeds = [DirectedStakeWhitelist::SEED, config.key().as_ref()],
        bump
    )]
    pub whitelist_account: AccountLoader<'info, DirectedStakeWhitelist>,

    #[account(
        init,
        payer = signer,
        space = DirectedStakeTicket::SIZE,
        seeds = [DirectedStakeTicket::SEED, config.key().as_ref(), ticket_update_authority.as_ref()],
        bump
    )]
    pub ticket_account: AccountLoader<'info, DirectedStakeTicket>,

    pub system_program: Program<'info, System>,

    #[account(mut)]
    pub signer: Signer<'info>,
}

impl InitializeDirectedStakeTicket<'_> {
    pub const SIZE: usize = 8 + size_of::<Self>();

    pub fn auth(
        whitelist: &DirectedStakeWhitelist,
        signer_pubkey: &Pubkey,
        ticket_update_authority: &Pubkey,
        ticket_override_authority: &Pubkey,
    ) -> Result<()> {
        if !whitelist.is_staker_permissioned(signer_pubkey)
            && signer_pubkey != ticket_override_authority
        {
            return Err(error!(StewardError::Unauthorized));
        }

        if signer_pubkey != ticket_update_authority && signer_pubkey != ticket_override_authority {
            msg!("Error: Only a valid ticket authority can initialize tickets.");
            return Err(error!(StewardError::Unauthorized));
        }

        Ok(())
    }
}

pub fn handler(
    ctx: Context<InitializeDirectedStakeTicket>,
    ticket_update_authority: Pubkey,
    ticket_holder_is_protocol: bool,
) -> Result<()> {
    let config = ctx.accounts.config.load()?;
    let whitelist = ctx.accounts.whitelist_account.load()?;
    InitializeDirectedStakeTicket::auth(
        &whitelist,
        ctx.accounts.signer.key,
        &ticket_update_authority,
        &config.directed_stake_ticket_override_authority,
    )?;

    // PDA is verified by Anchor seeds constraint

    let mut ticket = ctx.accounts.ticket_account.load_init()?;
    ticket.num_preferences = 0;
    ticket.staker_preferences =
        [DirectedStakePreference::empty(); crate::MAX_PREFERENCES_PER_TICKET];
    ticket.ticket_update_authority = ticket_update_authority;
    ticket.ticket_holder_is_protocol = U8Bool::from(ticket_holder_is_protocol);
    ticket._padding0 = [0; 125];

    Ok(())
}
