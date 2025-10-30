// use std::sync::Arc;
//
// use anchor_lang::{InstructionData, ToAccountMetas};
// use anyhow::Result;
//
// use solana_client::nonblocking::rpc_client::RpcClient;
// use solana_program::instruction::Instruction;
// #[allow(deprecated)]
// use solana_sdk::{
//     pubkey::Pubkey, signature::read_keypair_file, signer::Signer, stake, system_program,
//     transaction::Transaction,
// };
// use spl_stake_pool::find_stake_program_address;
// use stakenet_sdk::utils::{
//     accounts::{get_all_steward_accounts, get_validator_history_address},
//     transactions::{configure_instruction, print_base58_tx},
// };
// use validator_history::id as validator_history_id;
//
// use crate::commands::command_args::AutoAddValidatorFromPool;
//
// pub async fn command_auto_add_validator_from_pool(
//     args: AutoAddValidatorFromPool,
//     client: &Arc<RpcClient>,
//     program_id: Pubkey,
// ) -> Result<()> {
//     // Creates config account
//     let payer = Arc::new(
//         read_keypair_file(args.permissionless_parameters.payer_keypair_path)
//             .expect("Failed reading keypair file ( Payer )"),
//     );
//
//     let validator_history_program_id = validator_history_id();
//     let steward_config = args.permissionless_parameters.steward_config;
//
//     let steward_accounts = get_all_steward_accounts(client, &program_id, &steward_config).await?;
//
//     let vote_account = args.vote_account;
//     let history_account =
//         get_validator_history_address(&vote_account, &validator_history_program_id);
//
//     let (stake_address, _) = find_stake_program_address(
//         &spl_stake_pool::id(),
//         &vote_account,
//         &steward_accounts.stake_pool_address,
//         None,
//     );
//
//     let ix = Instruction {
//         program_id,
//         accounts: jito_steward::accounts::AutoAddValidator {
//             validator_history_account: history_account,
//             steward_state: steward_accounts.state_address,
//             config: args.permissionless_parameters.steward_config,
//             stake_pool_program: spl_stake_pool::id(),
//             stake_pool: steward_accounts.stake_pool_address,
//             reserve_stake: steward_accounts.stake_pool_account.reserve_stake,
//             withdraw_authority: steward_accounts.stake_pool_withdraw_authority,
//             validator_list: steward_accounts.validator_list_address,
//             stake_account: stake_address,
//             vote_account,
//             rent: solana_sdk::sysvar::rent::id(),
//             clock: solana_sdk::sysvar::clock::id(),
//             stake_history: solana_sdk::sysvar::stake_history::id(),
//             stake_config: stake::config::ID,
//             system_program: system_program::id(),
//             stake_program: stake::program::id(),
//         }
//         .to_account_metas(None),
//         data: jito_steward::instruction::AutoAddValidatorToPool {}.data(),
//     };
//
//     let blockhash = client
//         .get_latest_blockhash()
//         .await
//         .expect("Failed to get recent blockhash");
//
//     let configured_ix = configure_instruction(
//         &[ix],
//         args.permissionless_parameters
//             .transaction_parameters
//             .priority_fee,
//         args.permissionless_parameters
//             .transaction_parameters
//             .compute_limit,
//         args.permissionless_parameters
//             .transaction_parameters
//             .heap_size,
//     );
//
//     let transaction = Transaction::new_signed_with_payer(
//         &configured_ix,
//         Some(&payer.pubkey()),
//         &[&payer],
//         blockhash,
//     );
//
//     if args
//         .permissionless_parameters
//         .transaction_parameters
//         .print_tx
//     {
//         print_base58_tx(&configured_ix)
//     } else {
//         let signature = client
//             .send_and_confirm_transaction_with_spinner(&transaction)
//             .await?;
//
//         println!("Signature: {}", signature);
//     }
//
//     Ok(())
// }
//
