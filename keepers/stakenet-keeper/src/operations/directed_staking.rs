// //! Directed Staking Operation
// //!
// //! This module handles directed staking operation.
//
// use std::sync::Arc;
//
// use log::error;
// use solana_client::nonblocking::rpc_client::RpcClient;
// use solana_pubkey::Pubkey;
// use solana_sdk::{signature::Keypair, signer::Signer};
// use stakenet_sdk::{
//     models::{errors::JitoTransactionError, submit_stats::SubmitStats},
//     utils::{instructions::compute_directed_stake_meta, transactions::submit_chunk_instructions},
// };
//
// use crate::{
//     operations::keeper_operations::{check_flag, KeeperOperations},
//     state::{keeper_config::KeeperConfig, keeper_state::KeeperState},
// };
//
// fn _get_operation() -> KeeperOperations {
//     KeeperOperations::DirectedStaking
// }
//
// fn _should_run() -> bool {
//     true
// }
//
// pub async fn fire(
//     keeper_config: &KeeperConfig,
//     keeper_state: &KeeperState,
// ) -> (KeeperOperations, u64, u64, u64) {
//     let client = keeper_config.client.clone();
//     let keypair = keeper_config.keypair.clone();
//     let program_id = &keeper_config.validator_history_program_id;
//     let priority_fee_in_microlamports = keeper_config.priority_fee_in_microlamports;
//
//     let operation = _get_operation();
//     let (mut runs_for_epoch, mut errors_for_epoch, mut txs_for_epoch) =
//         keeper_state.copy_runs_errors_and_txs_for_epoch(operation);
//
//     let should_run = _should_run() && check_flag(keeper_config.run_flags, operation);
//
//     if should_run {
//         match copy_directed_stake_targets(
//             client,
//             keypair,
//             program_id,
//             &keeper_config.token_mint,
//             &keeper_config.stake_pool,
//             &keeper_config.steward_config,
//             keeper_config.tx_retry_count,
//             keeper_config.tx_confirmation_seconds,
//             priority_fee_in_microlamports,
//         )
//         .await
//         {
//             Ok(stats) => {
//                 for message in stats.results.iter().chain(stats.results.iter()) {
//                     if let Err(e) = message {
//                         error!("ERROR: {}", e);
//                         // datapoint_error!(
//                         //     "priority-fee-commission-error",
//                         //     ("error", e.to_string(), String),
//                         // );
//                         errors_for_epoch += 1;
//                     } else {
//                         txs_for_epoch += 1;
//                     }
//                 }
//                 if stats.errors == 0 {
//                     runs_for_epoch += 1;
//                 }
//             }
//             Err(_e) => {
//                 // datapoint_error!(
//                 //     "priority-fee-commission-error",
//                 //     ("error", e.to_string(), String),
//                 // );
//                 errors_for_epoch += 1;
//             }
//         };
//     }
//
//     (operation, runs_for_epoch, errors_for_epoch, txs_for_epoch)
// }
//
// /// Copy directed stake targets to [`DirectedStakeMeta`] account
// #[allow(clippy::too_many_arguments)]
// async fn copy_directed_stake_targets(
//     client: Arc<RpcClient>,
//     keypair: Arc<Keypair>,
//     program_id: &Pubkey,
//     token_mint_address: &Pubkey,
//     stake_pool_address: &Pubkey,
//     steward_config: &Pubkey,
//     retry_count: u16,
//     confirmation_time: u64,
//     priority_fee_in_microlamports: u64,
// ) -> Result<SubmitStats, JitoTransactionError> {
//     let ixs = compute_directed_stake_meta(
//         client.clone(),
//         token_mint_address,
//         stake_pool_address,
//         steward_config,
//         &keypair.pubkey(),
//         program_id,
//     )
//     .await
//     .map_err(|e| JitoTransactionError::Custom(e.to_string()))?;
//
//     // Don't bundle under 160, such that TXs wont fail in larger bundles
//     let chunk_size = match ixs.len() {
//         0..=160 => 1,
//         _ => 8,
//     };
//
//     let submit_result = submit_chunk_instructions(
//         &client,
//         ixs,
//         &keypair,
//         priority_fee_in_microlamports,
//         retry_count,
//         confirmation_time,
//         None,
//         chunk_size,
//     )
//     .await;
//
//     submit_result.map_err(|e| e.into())
// }
