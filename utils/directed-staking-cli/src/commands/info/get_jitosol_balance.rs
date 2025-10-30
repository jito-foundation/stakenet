// use crate::commands::command_args::GetJitosolBalance;
// use anyhow::Result;
// use solana_client::nonblocking::rpc_client::RpcClient;
// use solana_sdk::pubkey::Pubkey;
// use std::sync::Arc;
//
// pub async fn command_get_jitosol_balance(
//     args: GetJitosolBalance,
//     client: &Arc<RpcClient>,
//     _program_id: Pubkey,
// ) -> Result<()> {
//     // Get the token account balance
//     let token_balance = client
//         .get_token_account_balance(&args.token_account)
//         .await?;
//
//     if args.print_json {
//         let mut json_output = serde_json::Map::new();
//
//         json_output.insert(
//             "token_account".to_string(),
//             serde_json::Value::String(args.token_account.to_string()),
//         );
//         json_output.insert(
//             "amount".to_string(),
//             serde_json::Value::String(token_balance.amount.clone()),
//         );
//         json_output.insert(
//             "decimals".to_string(),
//             serde_json::Value::Number(serde_json::Number::from(token_balance.decimals)),
//         );
//         json_output.insert(
//             "ui_amount".to_string(),
//             serde_json::Value::Number(
//                 serde_json::Number::from_f64(token_balance.ui_amount.unwrap_or(0.0)).unwrap(),
//             ),
//         );
//
//         println!("{}", serde_json::to_string_pretty(&json_output)?);
//     } else {
//         let ui_amount = token_balance.ui_amount.unwrap_or(0.0);
//         println!("JitoSOL Balance for {}:", args.token_account);
//         println!("  Amount: {} JitoSOL", ui_amount);
//         println!("  Raw Amount: {}", token_balance.amount);
//         println!("  Decimals: {}", token_balance.decimals);
//     }
//
//     Ok(())
// }
//
