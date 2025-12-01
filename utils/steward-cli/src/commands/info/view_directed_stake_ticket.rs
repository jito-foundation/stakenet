use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use stakenet_sdk::utils::accounts::get_directed_stake_ticket;

#[derive(Parser)]
#[command(about = "View DirectedStakeTicket account")]
pub struct ViewDirectedStakeTicket {
    /// Steward config account
    #[arg(long, env)]
    pub steward_config: Pubkey,

    /// Directed stake ticket address
    #[arg(long)]
    pub ticket_signer: Pubkey,
}

pub async fn command_view_directed_stake_ticket(
    args: ViewDirectedStakeTicket,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    let ticket = get_directed_stake_ticket(
        client.clone(),
        &args.steward_config,
        &args.ticket_signer,
        &program_id,
    )
    .await?;

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
