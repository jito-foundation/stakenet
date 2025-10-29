use crate::commands::command_args::ViewDirectedStakeTickets;
use anchor_lang::Discriminator;
use anyhow::Result;
use jito_steward::DirectedStakeTicket;
use solana_account_decoder::UiAccountEncoding;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_filter::{Memcmp, MemcmpEncodedBytes, RpcFilterType};
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;

pub async fn command_view_directed_stake_tickets(
    args: ViewDirectedStakeTickets,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    let discriminator = <DirectedStakeTicket as Discriminator>::DISCRIMINATOR;
    let memcmp_filter = RpcFilterType::Memcmp(Memcmp::new(
        0,
        MemcmpEncodedBytes::Base58(solana_sdk::bs58::encode(discriminator).into_string()),
    ));

    let accounts = client
        .get_program_accounts_with_config(
            &program_id,
            solana_client::rpc_config::RpcProgramAccountsConfig {
                filters: Some(vec![memcmp_filter]),
                account_config: solana_client::rpc_config::RpcAccountInfoConfig {
                    encoding: Some(UiAccountEncoding::Base64),
                    commitment: Some(CommitmentConfig::confirmed()),
                    data_slice: None,
                    min_context_slot: None,
                },
                with_context: Some(true),
                sort_results: None,
            },
        )
        .await?;

    let accounts_count = accounts.len();

    if args.print_json {
        let mut json_output = serde_json::Map::new();
        let mut accounts_array = serde_json::Map::new();

        for (pubkey, account) in &accounts {
            let mut account_info = serde_json::Map::new();
            account_info.insert(
                "pubkey".to_string(),
                serde_json::Value::String(pubkey.to_string()),
            );
            account_info.insert(
                "owner".to_string(),
                serde_json::Value::String(account.owner.to_string()),
            );
            account_info.insert(
                "lamports".to_string(),
                serde_json::Value::Number(serde_json::Number::from(account.lamports)),
            );
            account_info.insert(
                "data".to_string(),
                serde_json::Value::String(base64::Engine::encode(
                    &base64::engine::general_purpose::STANDARD,
                    &account.data,
                )),
            );
            account_info.insert(
                "executable".to_string(),
                serde_json::Value::Bool(account.executable),
            );
            account_info.insert(
                "rent_epoch".to_string(),
                serde_json::Value::Number(serde_json::Number::from(account.rent_epoch)),
            );

            accounts_array.insert(pubkey.to_string(), serde_json::Value::Object(account_info));
        }

        json_output.insert(
            "accounts".to_string(),
            serde_json::Value::Object(accounts_array),
        );
        json_output.insert(
            "count".to_string(),
            serde_json::Value::Number(serde_json::Number::from(accounts_count)),
        );

        println!("{}", serde_json::to_string_pretty(&json_output)?);
    } else {
        println!("Found {} DirectedStakeTicket accounts:", accounts_count);
        for (pubkey, _account) in &accounts {
            println!("  {}", pubkey);
        }
    }

    Ok(())
}
