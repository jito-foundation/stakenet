use clap::{arg, command, Parser};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{pubkey::Pubkey, signature::read_keypair_file};
use stakenet_sdk::utils::accounts::get_all_validator_history_accounts;
use std::collections::HashMap;
use std::fs::File;
use std::hash::{Hash, Hasher};
use std::io::{BufRead, BufReader};
use std::str::FromStr;
use std::{path::PathBuf, time::Duration};
use validator_history::{ValidatorHistory, ValidatorHistoryEntry};

#[derive(Parser)]
#[command(about = "Initialize config account")]
pub struct BackfillValidatorAge {
    /// Path to oracle authority keypair
    #[arg(short, long, env, default_value = "~/.config/solana/id.json")]
    keypair_path: PathBuf,

    /// Path to oracle source data one CSV file
    #[arg(
        short,
        long,
        env,
        default_value = "utils/validator-history-cli/data/validator-age/one/data.csv"
    )]
    oracle_source_data_one: PathBuf,

    /// Path to oracle source data two CSV file
    #[arg(
        short,
        long,
        env,
        default_value = "utils/validator-history-cli/data/validator-age/two/data.csv"
    )]
    oracle_source_data_two: PathBuf,
}

pub async fn run(args: BackfillValidatorAge, rpc_url: String) {
    // Parse oracle keypair
    let _keypair = read_keypair_file(args.keypair_path).expect("Failed reading keypair file");

    // Build async client
    let client = RpcClient::new_with_timeout(rpc_url, Duration::from_secs(60));

    // Get current epoch
    let epoch_info = client
        .get_epoch_info()
        .await
        .expect("Failed to get epoch info");
    let _current_epoch = epoch_info.epoch;

    // Get all validator history accounts
    let accounts = get_all_validator_history_accounts(&client, validator_history::ID)
        .await
        .expect("Failed to fetch all validator history accounts");

    // Read oracle source data
    let source_one = read_oracle_data(args.oracle_source_data_one);
    let source_two = read_oracle_data(args.oracle_source_data_two);

    // Merge sources
    let merged_sources = merge_sources(source_one.as_slice(), source_two.as_slice());

    // Prepare validator histories with their vote accounts
    let validator_histories: Vec<(Pubkey, ValidatorHistory)> = accounts
        .iter()
        .map(|account| (account.vote_account, account.clone()))
        .collect();

    // Compute oracle validator ages using both oracle data and onchain data
    let validator_ages = compute_validator_ages(&merged_sources, &validator_histories);

    // TODO: Build instructions for updating validator ages
    for (vote_account, age) in validator_ages.iter() {
        println!(
            "Validator {}: total={}, since_inception={}",
            vote_account, age.total, age.since_program_inception
        );
    }
}

async fn _build_instruction(
    validator_history: &ValidatorHistory,
    validator_ages: &HashMap<Pubkey, ValidatorAge>,
) {
    // Find oracle age
    let oracle_validator_age = validator_ages.get(&validator_history.vote_account);
    if let Some(_oracle_age) = oracle_validator_age {
        // Find age onchain
        let _age_onchain = validator_history.validator_age;
        // TODO: Build instruction to update validator age if needed
    }
}

#[derive(Debug, Clone, Copy)]
struct ValidatorEpochKey {
    vote_account: Pubkey,
    epoch: u16,
}

impl PartialEq for ValidatorEpochKey {
    fn eq(&self, other: &Self) -> bool {
        self.vote_account == other.vote_account && self.epoch == other.epoch
    }
}

impl Eq for ValidatorEpochKey {}

impl Hash for ValidatorEpochKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.vote_account.hash(state);
        self.epoch.hash(state);
    }
}

#[derive(Debug, Clone, Copy)]
struct SourceData {
    vote_account: Pubkey,
    epoch: u16,
    credits: u32,
}

struct ValidatorAge {
    total: u32,
    since_program_inception: u32,
}

/// Finds the first (earliest) epoch with non-zero vote credits in the validator history
fn find_first_epoch_with_credits(validator_history: &ValidatorHistory) -> Option<u16> {
    let history = &validator_history.history;

    // If buffer is empty, return None
    if history.is_empty() {
        return None;
    }

    let mut first_epoch_with_credits = None;

    // Iterate through all entries in the circular buffer
    for entry in history.arr.iter() {
        // Skip default/uninitialized entries
        if entry.epoch == ValidatorHistoryEntry::default().epoch {
            continue;
        }

        // Check if this entry has non-zero vote credits
        if entry.epoch_credits > 0 && entry.epoch_credits != u32::MAX {
            // Update first_epoch_with_credits if this is earlier than what we have
            match first_epoch_with_credits {
                None => first_epoch_with_credits = Some(entry.epoch),
                Some(current_first) if entry.epoch < current_first => {
                    first_epoch_with_credits = Some(entry.epoch);
                }
                _ => {}
            }
        }
    }

    first_epoch_with_credits
}

/// Counts the number of epochs with non-zero vote credits in the validator history
/// starting from a given epoch
fn count_onchain_epochs_with_credits(validator_history: &ValidatorHistory, from_epoch: u16) -> u32 {
    let history = &validator_history.history;
    let mut count = 0;

    for entry in history.arr.iter() {
        // Skip default/uninitialized entries
        if entry.epoch == ValidatorHistoryEntry::default().epoch {
            continue;
        }

        // Only count entries from the specified epoch onwards
        if entry.epoch >= from_epoch && entry.epoch_credits > 0 && entry.epoch_credits != u32::MAX {
            count += 1;
        }
    }

    count
}

/// Computes validator ages by combining oracle data and onchain data
/// - Uses oracle data up to the first onchain epoch
/// - Uses onchain data from the first onchain epoch onwards
fn compute_validator_ages(
    merged_source_data: &[SourceData],
    validator_histories: &[(Pubkey, ValidatorHistory)],
) -> HashMap<Pubkey, ValidatorAge> {
    let mut validator_ages: HashMap<Pubkey, ValidatorAge> = HashMap::new();

    // Create a map for quick lookup of validator histories
    let history_map: HashMap<Pubkey, &ValidatorHistory> = validator_histories
        .iter()
        .map(|(pubkey, history)| (*pubkey, history))
        .collect();

    // Group oracle data by vote account
    let mut oracle_data_by_validator: HashMap<Pubkey, Vec<&SourceData>> = HashMap::new();
    for data in merged_source_data {
        oracle_data_by_validator
            .entry(data.vote_account)
            .or_insert_with(Vec::new)
            .push(data);
    }

    // Process each validator
    for (vote_account, history) in history_map.iter() {
        let first_onchain_epoch = find_first_epoch_with_credits(history);

        let mut total_epochs = 0u32;

        // Count oracle epochs up to (but not including) the first onchain epoch
        if let Some(first_epoch) = first_onchain_epoch {
            if let Some(oracle_data) = oracle_data_by_validator.get(vote_account) {
                for data in oracle_data {
                    if data.credits > 0 && data.epoch < first_epoch {
                        total_epochs += 1;
                    }
                }
            }

            // Count onchain epochs from the first onchain epoch onwards
            let onchain_epochs = count_onchain_epochs_with_credits(history, first_epoch);
            total_epochs += onchain_epochs;
        } else {
            // No onchain data, use all oracle data
            if let Some(oracle_data) = oracle_data_by_validator.get(vote_account) {
                for data in oracle_data {
                    if data.credits > 0 {
                        total_epochs += 1;
                    }
                }
            }
        }

        if total_epochs > 0 {
            validator_ages.insert(
                *vote_account,
                ValidatorAge {
                    total: total_epochs,
                    since_program_inception: total_epochs, // Using total since we're based on actual data
                },
            );
        }
    }

    // Also process validators that are only in oracle data (not in validator histories)
    for (vote_account, oracle_data) in oracle_data_by_validator.iter() {
        if !history_map.contains_key(vote_account) {
            let mut total_epochs = 0u32;

            for data in oracle_data {
                if data.credits > 0 {
                    total_epochs += 1;
                }
            }

            if total_epochs > 0 {
                validator_ages.insert(
                    *vote_account,
                    ValidatorAge {
                        total: total_epochs,
                        since_program_inception: total_epochs, // Using total since we're based on actual data
                    },
                );
            }
        }
    }

    validator_ages
}

fn to_map(data: &[SourceData]) -> HashMap<ValidatorEpochKey, u32> {
    let mut map = HashMap::with_capacity(data.len());
    for item in data {
        let key = ValidatorEpochKey {
            vote_account: item.vote_account,
            epoch: item.epoch,
        };
        map.insert(key, item.credits);
    }
    map
}

/// Merges the two source files.
///
/// When both sources have credit values for the same vote pubkey at the same epoch,
/// we take the min of the two values. This is a more strict approach.
fn merge_sources(source_one: &[SourceData], source_two: &[SourceData]) -> Vec<SourceData> {
    // Allocate maps for merging
    let map_one = to_map(source_one);
    let map_two = to_map(source_two);
    let mut merged = HashMap::with_capacity(map_one.len() + map_two.len());

    // Add all entries from source one
    for (key, credits) in map_one {
        merged.insert(key, credits);
    }

    // Merge entries from source two, taking minimum when key exists
    for (key, credits) in map_two {
        merged
            .entry(key)
            .and_modify(|existing| *existing = (*existing).min(credits))
            .or_insert(credits);
    }

    // Collect
    merged
        .into_iter()
        .map(|(key, credits)| SourceData {
            vote_account: key.vote_account,
            epoch: key.epoch,
            credits,
        })
        .collect()
}

fn read_oracle_data(path: PathBuf) -> Vec<SourceData> {
    // Open file
    let file =
        File::open(&path).unwrap_or_else(|e| panic!("Failed to open file {:?}: {}", path, e));
    let reader = BufReader::new(file);
    let mut lines = reader.lines();

    // Skip header line
    lines.next();

    // Read and parse lines
    let mut data = Vec::new();
    for line in lines {
        // Read line
        let line = line.expect("Failed to read line");
        // Split csv
        let parts: Vec<&str> = line.split(',').collect();
        // Assert three fields
        if parts.len() != 3 {
            panic!("Skipping invalid line: {}", line);
        }
        // Parse vote pubkey
        let vote_account = Pubkey::from_str(parts[0])
            .unwrap_or_else(|e| panic!("Failed to parse vote account {}: {}", parts[0], e));
        // Parse epoch
        let epoch = parts[1]
            .parse::<u16>()
            .unwrap_or_else(|e| panic!("Failed to parse epoch {}: {}", parts[1], e));
        // Parse credits
        let credits = parts[2]
            .parse::<u32>()
            .unwrap_or_else(|e| panic!("Failed to parse credits {}: {}", parts[2], e));
        // Push
        data.push(SourceData {
            vote_account,
            epoch,
            credits,
        });
    }

    data
}
