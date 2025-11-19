use std::sync::Arc;

use anyhow::Result;
use jito_steward::DirectedStakeTicket;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use stakenet_sdk::utils::accounts::get_directed_stake_tickets;

use crate::commands::command_args::ViewDirectedStakeTickets;

pub async fn command_view_directed_stake_tickets(
    args: ViewDirectedStakeTickets,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    let ticket_map = get_directed_stake_tickets(client.clone(), &program_id).await?;
    let tickets: Vec<DirectedStakeTicket> = ticket_map.values().map(|t| *t).collect();
    let tickets_count = tickets.len();

    if args.print_json {
        let mut json_output = serde_json::Map::new();
        let mut tickets_array = Vec::new();

        for ticket in &tickets {
            let mut ticket_info = serde_json::Map::new();

            ticket_info.insert(
                "ticket_update_authority".to_string(),
                serde_json::Value::String(ticket.ticket_update_authority.to_string()),
            );
            ticket_info.insert(
                "ticket_holder_is_protocol".to_string(),
                serde_json::Value::Bool(ticket.ticket_holder_is_protocol.into()),
            );
            ticket_info.insert(
                "num_preferences".to_string(),
                serde_json::Value::Number(serde_json::Number::from(ticket.num_preferences)),
            );

            // Add preferences
            let mut preferences_array = Vec::new();
            let mut total_bps = 0u32;

            for i in 0..ticket.num_preferences as usize {
                let pref = &ticket.staker_preferences[i];
                total_bps += pref.stake_share_bps as u32;

                let mut pref_obj = serde_json::Map::new();
                pref_obj.insert(
                    "vote_pubkey".to_string(),
                    serde_json::Value::String(pref.vote_pubkey.to_string()),
                );
                pref_obj.insert(
                    "stake_share_bps".to_string(),
                    serde_json::Value::Number(serde_json::Number::from(pref.stake_share_bps)),
                );
                pref_obj.insert(
                    "stake_share_percent".to_string(),
                    serde_json::Value::Number(
                        serde_json::Number::from_f64(pref.stake_share_bps as f64 / 100.0)
                            .unwrap_or(serde_json::Number::from(0)),
                    ),
                );
                preferences_array.push(serde_json::Value::Object(pref_obj));
            }

            ticket_info.insert(
                "preferences".to_string(),
                serde_json::Value::Array(preferences_array),
            );
            ticket_info.insert(
                "total_allocation_bps".to_string(),
                serde_json::Value::Number(serde_json::Number::from(total_bps)),
            );
            ticket_info.insert(
                "total_allocation_percent".to_string(),
                serde_json::Value::Number(
                    serde_json::Number::from_f64(total_bps as f64 / 100.0)
                        .unwrap_or(serde_json::Number::from(0)),
                ),
            );

            tickets_array.push(serde_json::Value::Object(ticket_info));
        }

        json_output.insert(
            "tickets".to_string(),
            serde_json::Value::Array(tickets_array),
        );
        json_output.insert(
            "count".to_string(),
            serde_json::Value::Number(serde_json::Number::from(tickets_count)),
        );

        println!("{}", serde_json::to_string_pretty(&json_output)?);
    } else {
        println!("Found {} DirectedStakeTicket accounts:\n", tickets_count);

        for (pda, ticket) in ticket_map {
            println!("Ticket: {pda}");
            println!("  Update Authority: {}", ticket.ticket_update_authority);
            println!(
                "  Is Protocol: {}",
                bool::from(ticket.ticket_holder_is_protocol)
            );
            println!("  Number of Preferences: {}", ticket.num_preferences);

            if ticket.num_preferences > 0 {
                println!("  Stake Preferences:");
                let mut total_bps = 0u32;
                for i in 0..ticket.num_preferences as usize {
                    let pref = &ticket.staker_preferences[i];
                    total_bps += pref.stake_share_bps as u32;
                    println!(
                        "    [{}] Validator: {} - {:.2}% ({} bps)",
                        i + 1,
                        pref.vote_pubkey,
                        pref.stake_share_bps as f64 / 100.0,
                        pref.stake_share_bps
                    );
                }
                println!(
                    "  Total Allocation: {:.2}% ({} bps)",
                    total_bps as f64 / 100.0,
                    total_bps
                );
            }
            println!();
        }
    }

    Ok(())
}
