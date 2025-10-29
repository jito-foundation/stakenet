use anchor_lang::prelude::*;

use crate::{errors::StewardError, Config, DirectedStakeTicket};
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

    #[account(mut)]
    pub authority: Signer<'info>,
}

impl CloseDirectedStakeTicket<'_> {
    pub const SIZE: usize = 8 + size_of::<Self>();

    pub fn auth(
        ticket: &DirectedStakeTicket,
        authority_pubkey: &Pubkey,
        directed_stake_whitelist_authority: &Pubkey,
    ) -> Result<()> {
        if authority_pubkey != directed_stake_whitelist_authority
            && authority_pubkey != &ticket.ticket_update_authority
        {
            msg!("Error: Only the ticket update authority or directed stake whitelist authority can close the ticket.");
            return Err(error!(StewardError::Unauthorized));
        }
        Ok(())
    }
}

pub fn handler(ctx: Context<CloseDirectedStakeTicket>) -> Result<()> {
    let ticket = ctx.accounts.ticket_account.load()?;
    let config = ctx.accounts.config.load()?;
    CloseDirectedStakeTicket::auth(
        &ticket,
        ctx.accounts.authority.key,
        &config.directed_stake_whitelist_authority,
    )?;
    Ok(())
}
