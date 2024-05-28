use crate::entries::stake_history_entry::StakeHistoryEntry;
/*
This program starts several threads to manage the creation of validator history accounts,
and the updating of the various data feeds within the accounts.
It will emits metrics for each data feed, if env var SOLANA_METRICS_CONFIG is set to a valid influx server.
*/
use crate::state::keeper_state::KeeperState;
use crate::{KeeperError, PRIORITY_FEE};
use keeper_core::{submit_instructions, SubmitStats, UpdateInstruction};
use log::*;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_response::RpcVoteAccountInfo;
use solana_metrics::datapoint_error;
use solana_sdk::{
    epoch_info::EpochInfo,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};
use std::{collections::HashMap, str::FromStr, sync::Arc};
use validator_history::{ValidatorHistory, ValidatorHistoryEntry};

use super::keeper_operations::KeeperOperations;

fn _get_operation() -> KeeperOperations {
    KeeperOperations::StakeUpload
}

fn _should_run(epoch_info: &EpochInfo, runs_for_epoch: u64) -> bool {
    // Run at 0.1%, 50% and 90% completion of epoch
    (epoch_info.slot_index > epoch_info.slots_in_epoch / 1000 && runs_for_epoch < 1)
        || (epoch_info.slot_index > epoch_info.slots_in_epoch * 5 / 10 && runs_for_epoch < 2)
        || (epoch_info.slot_index > epoch_info.slots_in_epoch * 9 / 10 && runs_for_epoch < 3)
}

async fn _process(
    client: &Arc<RpcClient>,
    keypair: &Arc<Keypair>,
    program_id: &Pubkey,
    keeper_state: &KeeperState,
) -> Result<SubmitStats, Box<dyn std::error::Error>> {
    update_stake_history(client, keypair, program_id, keeper_state).await
}

pub async fn fire(
    client: &Arc<RpcClient>,
    keypair: &Arc<Keypair>,
    program_id: &Pubkey,
    keeper_state: &KeeperState,
) -> (KeeperOperations, u64, u64) {
    let operation = _get_operation();
    let (mut runs_for_epoch, mut errors_for_epoch) =
        keeper_state.copy_runs_and_errors_for_epoch(operation.clone());

    let should_run = _should_run(&keeper_state.epoch_info, runs_for_epoch);

    if should_run {
        match _process(client, keypair, program_id, keeper_state).await {
            Ok(stats) => {
                for message in stats.results.iter().chain(stats.results.iter()) {
                    if let Err(e) = message {
                        datapoint_error!("stake-history-error", ("error", e.to_string(), String),);
                    }
                }

                if stats.errors == 0 {
                    runs_for_epoch += 1;
                }
            }
            Err(e) => {
                datapoint_error!("stake-history-error", ("error", e.to_string(), String),);
                errors_for_epoch += 1;
            }
        };
    }

    (operation, runs_for_epoch, errors_for_epoch)
}

// ----------------- OPERATION SPECIFIC FUNCTIONS -----------------

pub async fn update_stake_history(
    client: &Arc<RpcClient>,
    keypair: &Arc<Keypair>,
    program_id: &Pubkey,
    keeper_state: &KeeperState,
) -> Result<SubmitStats, Box<dyn std::error::Error>> {
    let epoch_info = &keeper_state.epoch_info;
    let vote_accounts = &keeper_state.vote_account_map.values().collect::<Vec<_>>();
    let validator_history_map = &keeper_state.validator_history_map;

    // Need to ensure that the response contains update stake amounts for the current epoch,
    // so we find the largest epoch a validator has voted on to confirm the data is fresh
    let max_vote_account_epoch = vote_accounts
        .iter()
        .flat_map(|vote_account| vote_account.epoch_credits.clone())
        .map(|(epoch, _, _)| epoch)
        .max()
        .unwrap_or(0);

    let (stake_rank_map, superminority_threshold) =
        get_stake_rank_map_and_superminority_count(vote_accounts);

    if max_vote_account_epoch != epoch_info.epoch {
        //TODO Go through with custom errors
        return Err(Box::new(KeeperError::Custom("EpochMismatch".into())));
    }

    let entries_to_update = vote_accounts
        .iter()
        .filter_map(|vote_account| {
            let rank = stake_rank_map[&vote_account.vote_pubkey.clone()];
            let is_superminority = rank <= superminority_threshold;

            if stake_entry_uploaded(validator_history_map, vote_account, epoch_info.epoch) {
                return None;
            }

            Some(StakeHistoryEntry::new(
                vote_account,
                program_id,
                &keypair.pubkey(),
                epoch_info.epoch,
                rank,
                is_superminority,
            ))
        })
        .collect::<Vec<_>>();

    let update_instructions = entries_to_update
        .iter()
        .map(|stake_history_entry| stake_history_entry.update_instruction())
        .collect::<Vec<_>>();

    let submit_result =
        submit_instructions(client, update_instructions, keypair, PRIORITY_FEE).await;

    submit_result.map_err(|e| e.into())
}

fn stake_entry_uploaded(
    validator_history_map: &HashMap<Pubkey, ValidatorHistory>,
    vote_account: &RpcVoteAccountInfo,
    epoch: u64,
) -> bool {
    let vote_account = Pubkey::from_str(&vote_account.vote_pubkey)
        .map_err(|e| {
            error!("Invalid vote account pubkey");
            e
        })
        .expect("Invalid vote account pubkey");
    if let Some(validator_history) = validator_history_map.get(&vote_account) {
        if let Some(latest_entry) = validator_history.history.last() {
            return latest_entry.epoch == epoch as u16
                && latest_entry.is_superminority
                    != ValidatorHistoryEntry::default().is_superminority
                && latest_entry.rank != ValidatorHistoryEntry::default().rank
                && latest_entry.activated_stake_lamports
                    != ValidatorHistoryEntry::default().activated_stake_lamports;
        }
    }
    false
}

/*
Calculates ordering of validators by stake, assigning a 0..N rank (validator 0 has the most stake),
and returns the index at which all validators before are in the superminority. 0-indexed.
*/
fn get_stake_rank_map_and_superminority_count(
    vote_accounts: &[&RpcVoteAccountInfo],
) -> (HashMap<String, u32>, u32) {
    let mut stake_vec = vote_accounts
        .iter()
        .map(|va| (va.vote_pubkey.clone(), va.activated_stake))
        .collect::<Vec<_>>();

    let total_stake = stake_vec.iter().map(|(_, stake)| *stake).sum::<u64>();
    stake_vec.sort_by(|a, b| b.1.cmp(&a.1));

    let mut cumulative_stake = 0;
    let mut superminority_threshold = 0;
    for (i, (_, stake)) in stake_vec.iter().enumerate() {
        cumulative_stake += stake;
        if cumulative_stake > total_stake / 3 {
            superminority_threshold = i as u32;
            break;
        }
    }
    let stake_rank_map = HashMap::from_iter(
        stake_vec
            .into_iter()
            .enumerate()
            .map(|(i, (vote_pubkey, _))| (vote_pubkey, i as u32)),
    );

    (stake_rank_map, superminority_threshold)
}

// /*
//     Utility to recompute the superminority and rank fields for all validators from start_epoch to end_epoch.
//     Will over-write the on-chain data, so should only be used when the on-chain data is corrupted.
// */
// pub async fn _recompute_superminority_and_rank(
//     client: Arc<RpcClient>,
//     keypair: Arc<Keypair>,
//     program_id: &Pubkey,
//     start_epoch: u64,
//     end_epoch: u64,
// ) -> Result<(), KeeperError> {
//     // Fetch every ValidatorHistory account
//     let gpa_config = RpcProgramAccountsConfig {
//         filters: Some(vec![RpcFilterType::Memcmp(Memcmp::new_raw_bytes(
//             0,
//             ValidatorHistory::discriminator().into(),
//         ))]),
//         account_config: RpcAccountInfoConfig {
//             encoding: Some(solana_account_decoder::UiAccountEncoding::Base64),
//             ..RpcAccountInfoConfig::default()
//         },
//         ..RpcProgramAccountsConfig::default()
//     };
//     let validator_history_accounts = client
//         .get_program_accounts_with_config(&validator_history::id(), gpa_config)
//         .await
//         .expect("Failed to get validator history accounts");

//     let validator_histories = validator_history_accounts
//         .iter()
//         .map(|(_, account)| {
//             let validator_history = ValidatorHistory::try_deserialize(&mut account.data.as_slice())
//                 .expect("Failed to deserialize validator history account");
//             validator_history
//         })
//         .collect::<Vec<_>>();

//     for epoch in start_epoch..=end_epoch {
//         // Get entry for each validator for this epoch
//         let vote_accounts: Vec<RpcVoteAccountInfo> = validator_histories
//             .iter()
//             .filter_map(|validator| {
//                 validator
//                     .history
//                     .arr
//                     .iter()
//                     .find(|entry| {
//                         entry.epoch == epoch as u16 && entry.activated_stake_lamports != u64::MAX
//                     })
//                     .map(|entry| {
//                         // All values except vote_pubkey and activated_stake are unused
//                         RpcVoteAccountInfo {
//                             vote_pubkey: validator.vote_account.to_string(),
//                             activated_stake: entry.activated_stake_lamports,
//                             epoch_credits: vec![],
//                             commission: 0,
//                             root_slot: 0,
//                             node_pubkey: "".to_string(),
//                             epoch_vote_account: false,
//                             last_vote: 0,
//                         }
//                     })
//                     .into()
//             })
//             .collect();

//         let (stake_rank_map, superminority_threshold) =
//             get_stake_rank_map_and_superminority_count(&vote_accounts);

//         let stake_history_entries = vote_accounts
//             .iter()
//             .map(|va| {
//                 let rank = stake_rank_map[&va.vote_pubkey.clone()];
//                 let is_superminority = rank <= superminority_threshold;
//                 StakeHistoryEntry::new(
//                     va,
//                     program_id,
//                     &keypair.pubkey(),
//                     epoch,
//                     rank,
//                     is_superminority,
//                 )
//             })
//             .collect::<Vec<_>>();

//         let update_instructions = stake_history_entries
//             .iter()
//             .map(|entry| entry.update_instruction())
//             .collect::<Vec<_>>();

//         match submit_instructions(&client, update_instructions, &keypair, PRIORITY_FEE).await {
//             Ok(_) => println!("completed epoch {}", epoch),
//             Err(e) => return Err(e.into()),
//         };
//     }

//     Ok(())
// }
