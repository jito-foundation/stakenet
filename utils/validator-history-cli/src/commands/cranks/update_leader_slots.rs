use std::{collections::HashMap, path::PathBuf, str::FromStr, sync::Arc};

use anyhow::anyhow;
use clap::Parser;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{pubkey::Pubkey, signature::read_keypair_file, signer::Signer};
use stakenet_keeper::entries::leader_slots_entry::LeaderSlotsEntry;
use stakenet_sdk::{
    models::entries::UpdateInstruction,
    utils::{accounts::get_all_validator_history_accounts, transactions::submit_instructions},
};
use validator_history::ValidatorHistory;

#[derive(Parser)]
#[command(about = "Crank to copy scheduled leader slot counts to validator history accounts")]
pub struct CrankUpdateLeaderSlots {
    /// Path to the oracle authority keypair for transaction signing
    #[arg(short, long, env, default_value = "~/.config/solana/id.json")]
    keypair_path: PathBuf,

    /// Epoch to write the entries for. Must not be in the future.
    /// Defaults to the current epoch.
    #[arg(long, env)]
    epoch: Option<u64>,

    /// Write every entry, even when the on-chain count already matches the schedule
    #[arg(long, env, default_value = "false")]
    force: bool,
}

pub async fn run(
    args: CrankUpdateLeaderSlots,
    rpc_url: String,
    program_id: Pubkey,
) -> anyhow::Result<()> {
    let keypair = read_keypair_file(args.keypair_path)
        .map_err(|e| anyhow!("Failed reading keypair file: {e}"))?;
    let keypair = Arc::new(keypair);
    let client = Arc::new(RpcClient::new(rpc_url));

    let epoch_info = client.get_epoch_info().await?;
    let epoch = args.epoch.unwrap_or(epoch_info.epoch);

    // The program rejects future epochs with `EpochOutOfRange`; fail early with a clearer message.
    if epoch > epoch_info.epoch {
        return Err(anyhow!(
            "Cannot write epoch {epoch} because the current epoch is {}",
            epoch_info.epoch
        ));
    }

    println!("Target epoch: {epoch}");

    let epoch_schedule = client.get_epoch_schedule().await?;
    let first_slot_in_epoch = epoch_schedule.get_first_slot_in_epoch(epoch);

    // The leader schedule is keyed by validator identity, so we need the identity-to-vote
    // mapping from `getVoteAccounts` to find each leader's validator history account.
    let vote_accounts = client.get_vote_accounts().await?;
    let identity_to_vote_map: HashMap<&str, &str> = vote_accounts
        .current
        .iter()
        .chain(vote_accounts.delinquent.iter())
        .map(|vote_account| {
            (
                vote_account.node_pubkey.as_str(),
                vote_account.vote_pubkey.as_str(),
            )
        })
        .collect();

    let leader_schedule = client
        .get_leader_schedule(Some(first_slot_in_epoch))
        .await?
        .ok_or_else(|| anyhow!("No leader schedule available for epoch {epoch}"))?;

    println!("Found {} leaders in the schedule", leader_schedule.len());

    let validator_histories = get_all_validator_history_accounts(&client, program_id).await?;
    let validator_history_map: HashMap<Pubkey, ValidatorHistory> = validator_histories
        .into_iter()
        .map(|validator_history| (validator_history.vote_account, validator_history))
        .collect();

    let mut missing_vote_account = 0usize;
    let mut missing_history = 0usize;
    let mut already_set = 0usize;

    let entries: Vec<LeaderSlotsEntry> = leader_schedule
        .iter()
        .filter_map(|(identity, slots)| {
            let Some(vote_account) = identity_to_vote_map.get(identity.as_str()) else {
                missing_vote_account += 1;
                return None;
            };

            let vote_account = Pubkey::from_str(vote_account).ok()?;

            let Some(validator_history) = validator_history_map.get(&vote_account) else {
                missing_history += 1;
                return None;
            };

            // The schedule can't exceed `slots_in_epoch`, which the program rejects above `u32`.
            let total_leader_slots = slots.len() as u32;

            let uploaded = validator_history
                .history
                .arr
                .iter()
                .find(|entry| entry.epoch == epoch as u16)
                .is_some_and(|entry| entry.total_leader_slots == total_leader_slots);

            if uploaded && !args.force {
                already_set += 1;
                return None;
            }

            Some(LeaderSlotsEntry::new(
                vote_account,
                &program_id,
                &keypair.pubkey(),
                epoch,
                total_leader_slots,
            ))
        })
        .collect();

    println!(
        "Skipped {missing_vote_account} leaders without a vote account, {missing_history} without a validator history account, {already_set} already up to date"
    );
    println!("Updating {} validator history accounts", entries.len());

    if entries.is_empty() {
        return Ok(());
    }

    let update_instructions = entries
        .iter()
        .map(|entry| entry.update_instruction())
        .collect::<Vec<_>>();

    let submit_result = submit_instructions(
        &client,
        update_instructions,
        &keypair,
        0,
        50,
        0,
        Some(300_000),
        false,
    )
    .await;

    println!("Submit Result: {submit_result:?}");

    Ok(())
}
