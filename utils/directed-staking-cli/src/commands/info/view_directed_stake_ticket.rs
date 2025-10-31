use std::sync::Arc;

use anyhow::Result;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use stakenet_sdk::utils::accounts::get_directed_stake_ticket;

use crate::commands::command_args::ViewDirectedStakeTicket;

pub async fn command_view_directed_stake_ticket(
    args: ViewDirectedStakeTicket,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    let ticket =
        get_directed_stake_ticket(client.clone(), &args.ticket_signer, &program_id).await?;

    println!("num_preferences: {:?}", ticket.num_preferences);

    println!("staker_preferences:");
    for preference in ticket.staker_preferences {
        println!("  Vote pubkey: {:?}", preference.vote_pubkey);
        println!("  Stake share bps: {:?}", preference.stake_share_bps);
    }

    println!(
        "ticket_update_authority: {:?}",
        ticket.ticket_update_authority
    );
    println!(
        "ticket_holder_is_protocol: {:?}",
        ticket.ticket_holder_is_protocol.value
    );

    Ok(())
}
