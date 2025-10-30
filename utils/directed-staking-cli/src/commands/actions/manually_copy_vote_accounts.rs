// use std::sync::Arc;
//
// use anchor_lang::{InstructionData, ToAccountMetas};
// use anyhow::Result;
//
// use solana_client::nonblocking::rpc_client::RpcClient;
// use solana_program::instruction::Instruction;
//
// use crate::commands::command_args::ManuallyCopyVoteAccount;
// use solana_sdk::{pubkey::Pubkey, signature::read_keypair_file, signer::Signer};
// use stakenet_sdk::utils::{
//     accounts::{get_all_steward_accounts, get_validator_history_address},
//     transactions::{
//         configure_instruction, print_base58_tx, print_errors_if_any, submit_packaged_transactions,
//     },
// };
//
// pub async fn command_manually_copy_vote_account(
//     args: ManuallyCopyVoteAccount,
//     client: &Arc<RpcClient>,
//     program_id: Pubkey,
// ) -> Result<()> {
//     // Creates config account
//     let payer = Arc::new(
//         read_keypair_file(args.permissionless_parameters.payer_keypair_path)
//             .expect("Failed reading keypair file ( Payer )"),
//     );
//
//     let validator_history_program_id = spl_stake_pool::id();
//     let steward_config = args.permissionless_parameters.steward_config;
//     let index_to_update = args.validator_index_to_update;
//
//     let steward_accounts = get_all_steward_accounts(client, &program_id, &steward_config).await?;
//
//     let validator_to_update =
//         steward_accounts.validator_list_account.validators[index_to_update as usize];
//     let vote_account = validator_to_update.vote_account_address;
//
//     let validator_history_account =
//         get_validator_history_address(&vote_account, &validator_history_program_id);
//
//     let ix = Instruction {
//         program_id: validator_history::id(),
//         accounts: validator_history::accounts::CopyVoteAccount {
//             validator_history_account,
//             vote_account,
//             signer: payer.pubkey(),
//         }
//         .to_account_metas(None),
//         data: validator_history::instruction::CopyVoteAccount {}.data(),
//     };
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
//     if args
//         .permissionless_parameters
//         .transaction_parameters
//         .print_tx
//     {
//         print_base58_tx(&configured_ix)
//     } else {
//         let submit_stats =
//             submit_packaged_transactions(client, vec![configured_ix], &payer, Some(1), None)
//                 .await?;
//
//         print_errors_if_any(&submit_stats);
//     }
//
//     Ok(())
// }
//
