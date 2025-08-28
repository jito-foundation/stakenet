use crate::commands::command_args::ViewDirectedStakeWhitelist;
use anyhow::Result;
use jito_steward::state::directed_stake::DirectedStakeWhitelist;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;

pub async fn command_view_directed_stake_whitelist(
    args: ViewDirectedStakeWhitelist,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    let (directed_stake_whitelist_pda, _bump) = Pubkey::find_program_address(
        &[DirectedStakeWhitelist::SEED, args.steward_config.as_ref()],
        &program_id,
    );

    let whitelist_account = client
        .get_account_with_commitment(&directed_stake_whitelist_pda, CommitmentConfig::confirmed())
        .await?;

    if whitelist_account.value.is_none() {
        anyhow::bail!(
            "DirectedStakeWhitelist account not found: {}",
            directed_stake_whitelist_pda
        );
    }

    let whitelist_data = whitelist_account.value.unwrap().data;
    let whitelist = bytemuck::from_bytes::<DirectedStakeWhitelist>(&whitelist_data);

    if args.print_json {
        let mut json_output = serde_json::Map::new();

        let mut user_stakers = Vec::new();
        for i in 0..whitelist.total_permissioned_user_stakers as usize {
            if whitelist.permissioned_user_stakers[i] != Pubkey::default() {
                user_stakers.push(whitelist.permissioned_user_stakers[i].to_string());
            }
        }
        json_output.insert(
            "user_stakers".to_string(),
            serde_json::Value::Array(
                user_stakers
                    .into_iter()
                    .map(serde_json::Value::String)
                    .collect(),
            ),
        );

        let mut protocol_stakers = Vec::new();
        for i in 0..whitelist.total_permissioned_protocol_stakers as usize {
            if whitelist.permissioned_protocol_stakers[i] != Pubkey::default() {
                protocol_stakers.push(whitelist.permissioned_protocol_stakers[i].to_string());
            }
        }
        json_output.insert(
            "protocol_stakers".to_string(),
            serde_json::Value::Array(
                protocol_stakers
                    .into_iter()
                    .map(serde_json::Value::String)
                    .collect(),
            ),
        );

        let mut validators = Vec::new();
        for i in 0..whitelist.total_permissioned_validators as usize {
            if whitelist.permissioned_validators[i] != Pubkey::default() {
                validators.push(whitelist.permissioned_validators[i].to_string());
            }
        }
        json_output.insert(
            "validators".to_string(),
            serde_json::Value::Array(
                validators
                    .into_iter()
                    .map(serde_json::Value::String)
                    .collect(),
            ),
        );

        json_output.insert(
            "total_user_stakers".to_string(),
            serde_json::Value::Number(serde_json::Number::from(
                whitelist.total_permissioned_user_stakers,
            )),
        );
        json_output.insert(
            "total_protocol_stakers".to_string(),
            serde_json::Value::Number(serde_json::Number::from(
                whitelist.total_permissioned_protocol_stakers,
            )),
        );
        json_output.insert(
            "total_validators".to_string(),
            serde_json::Value::Number(serde_json::Number::from(
                whitelist.total_permissioned_validators,
            )),
        );

        println!("{}", serde_json::to_string_pretty(&json_output)?);
    } else {
        println!(
            "DirectedStakeWhitelist Account: {}",
            directed_stake_whitelist_pda
        );
        println!(
            "Total User Stakers: {}",
            whitelist.total_permissioned_user_stakers
        );
        println!(
            "Total Protocol Stakers: {}",
            whitelist.total_permissioned_protocol_stakers
        );
        println!(
            "Total Validators: {}",
            whitelist.total_permissioned_validators
        );
        println!();

        if whitelist.total_permissioned_user_stakers > 0 {
            println!("User Stakers:");
            for i in 0..whitelist.total_permissioned_user_stakers as usize {
                if whitelist.permissioned_user_stakers[i] != Pubkey::default() {
                    println!("  {}", whitelist.permissioned_user_stakers[i]);
                }
            }
            println!();
        }

        if whitelist.total_permissioned_protocol_stakers > 0 {
            println!("Protocol Stakers:");
            for i in 0..whitelist.total_permissioned_protocol_stakers as usize {
                if whitelist.permissioned_protocol_stakers[i] != Pubkey::default() {
                    println!("  {}", whitelist.permissioned_protocol_stakers[i]);
                }
            }
            println!();
        }

        if whitelist.total_permissioned_validators > 0 {
            println!("Validators:");
            for i in 0..whitelist.total_permissioned_validators as usize {
                if whitelist.permissioned_validators[i] != Pubkey::default() {
                    println!("  {}", whitelist.permissioned_validators[i]);
                }
            }
        }
    }

    Ok(())
}
