use anchor_lang::{InstructionData, ToAccountMetas};
use clap::{arg, command, Parser};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::compute_budget::ComputeBudgetInstruction;
use solana_sdk::instruction::Instruction;
use solana_sdk::signature::Keypair;
use solana_sdk::signer::Signer;
use solana_sdk::transaction::Transaction;
use solana_sdk::{pubkey::Pubkey, signature::read_keypair_file};
use stakenet_sdk::utils::accounts::get_all_validator_history_accounts;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::str::FromStr;
use std::{path::PathBuf, time::Duration};
use tokio::time;
use validator_history::{ValidatorHistory, ValidatorHistoryEntry};

#[derive(Parser)]
#[command(about = "Backfill validator ages onchain from historic oracle data")]
pub struct BackfillValidatorAge {
    /// Path to oracle authority keypair
    #[arg(short, long, env, default_value = "~/.config/solana/id.json")]
    keypair_path: PathBuf,

    /// Path to oracle source CSV file
    #[arg(
        short,
        long,
        env,
        default_value = "/data/validator-age/oracle/data.csv"
    )]
    oracle_source: PathBuf,
}

#[derive(Debug, Clone, Copy)]
struct SourceData {
    vote_account: Pubkey,
    epoch: u16,
    credits: u32,
}

pub async fn run(args: BackfillValidatorAge, rpc_url: String) {
    println!("/////////////////////////////////////////////////");
    println!("// Starting Backfill ////////////////////////////");
    println!("/////////////////////////////////////////////////");
    // Parse oracle keypair
    let keypair = read_keypair_file(args.keypair_path).expect("Failed reading keypair file");
    // Build async client
    let client = RpcClient::new_with_timeout(rpc_url, Duration::from_secs(60));
    // Read oracle source data
    println!("\nReading oracle data ...");
    let oracle = read_oracle_data(args.oracle_source);
    // Get current epoch
    let epoch_info = client
        .get_epoch_info()
        .await
        .expect("Failed to get epoch info");
    let current_epoch = epoch_info.epoch;
    // Get all validator history accounts
    println!("Fetching onchain history accounts ...");
    let accounts = get_all_validator_history_accounts(&client, validator_history::ID)
        .await
        .expect("Failed to fetch all validator history accounts");
    // Filter for valid vote accounts
    let accounts = validate_validator_history_accounts(&client, accounts.as_slice()).await;
    // Compute oracle validator ages using both oracle data and onchain data
    let validator_ages = compute_validator_ages(&oracle, &accounts);
    // Build and submit instructions
    for chunk in validator_ages.chunks(10) {
        let instructions = chunk
            .iter()
            .map(|tup| build_instruction(*tup, keypair.pubkey(), current_epoch as u16))
            .collect::<Vec<_>>();
        // Retry up to 3 times on failure
        let mut retry_count = 0;
        let max_retries: u8 = 3;
        loop {
            match build_and_submit_transaction(&client, instructions.as_slice(), &keypair).await {
                Ok(sig) => {
                    println!("Transaction successful: {sig:?}");
                    break;
                }
                Err(err) => {
                    retry_count += 1;
                    if retry_count >= max_retries {
                        println!(
                            "Transaction failed after {max_retries} retries: {err:?}"
                        );
                        break;
                    }
                    println!(
                        "Transaction failed (attempt {retry_count}/{max_retries}): {err:?}. Retrying..."
                    );
                    time::sleep(Duration::from_secs(2)).await;
                }
            }
        }
    }
}

async fn build_and_submit_transaction(
    client: &RpcClient,
    instructions: &[Instruction],
    signer: &Keypair,
) -> Result<solana_sdk::signature::Signature, solana_client::client_error::ClientError> {
    let instructions = [
        &[ComputeBudgetInstruction::set_compute_unit_limit(1_400_000)],
        instructions,
    ]
    .concat();
    let hash = client
        .get_latest_blockhash()
        .await
        .expect("Failed to fetch latest blockhash");
    let transaction = Transaction::new_signed_with_payer(
        instructions.as_slice(),
        Some(&signer.pubkey()),
        &[signer],
        hash,
    );
    client
        .send_and_confirm_transaction_with_spinner(&transaction)
        .await
}

/// Asserts that vote accounts exist and belong to the vote program
async fn validate_validator_history_accounts(
    client: &RpcClient,
    accounts: &[ValidatorHistory],
) -> Vec<ValidatorHistory> {
    let mut num_valid = 0;
    let mut num_closed = 0;
    let mut num_open_but_invalid = 0;
    let mut validated = vec![];
    for account in accounts {
        let vote_account = client.get_account(&account.vote_account).await;
        if let Ok(vote_account) = vote_account {
            if vote_account.owner == solana_sdk::vote::program::ID {
                validated.push(*account);
                num_valid += 1;
            } else {
                println!(
                    "Vote account exist but does not belong to the vote program: {:?}",
                    account.vote_account
                );
                num_open_but_invalid += 1;
            }
        } else {
            println!("Vote account does not exist: {:?}", account.vote_account);
            num_closed += 1;
        }
    }
    println!("\nNumber of valid vote accounts: {num_valid}");
    println!(
        "Number of open but invalid vote accounts: {num_open_but_invalid}"
    );
    println!("Number of closed vote accounts: {num_closed}");
    validated
}

fn build_instruction(validator_age: (Pubkey, u32), signer: Pubkey, epoch: u16) -> Instruction {
    let (vote_pubkey, age) = validator_age;
    let config =
        stakenet_sdk::utils::accounts::get_validator_history_config_address(&validator_history::ID);
    let validator_history_pda = stakenet_sdk::utils::accounts::get_validator_history_address(
        &vote_pubkey,
        &validator_history::ID,
    );
    let accounts = validator_history::accounts::UploadValidatorAge {
        validator_history_account: validator_history_pda,
        vote_account: vote_pubkey,
        config,
        oracle_authority: signer,
    };
    let data = validator_history::instruction::UploadValidatorAge {
        validator_age: age,
        validator_age_last_updated_epoch: epoch,
    };
    Instruction {
        program_id: validator_history::ID,
        accounts: accounts.to_account_metas(None),
        data: data.data(),
    }
}

/// Find the earliest epoch with non-zero vote credits in the validator history
fn find_first_epoch_with_credits(validator_history: &ValidatorHistory) -> Option<u16> {
    let history = &validator_history.history;
    let default = ValidatorHistoryEntry::default();
    let mut first_epoch_with_credits = None;
    for entry in history.arr.iter() {
        if entry.epoch == default.epoch {
            continue;
        }
        if entry.epoch_credits == default.epoch_credits {
            continue;
        }
        if entry.epoch_credits > 0 {
            match first_epoch_with_credits {
                None => first_epoch_with_credits = Some(entry.epoch),
                Some(current_first) => {
                    if entry.epoch < current_first {
                        first_epoch_with_credits = Some(entry.epoch);
                    }
                }
            }
        }
    }
    first_epoch_with_credits
}

/// Count the number of epochs with non-zero vote credits in the validator history
/// starting from a given epoch
fn count_onchain_epochs_with_credits(validator_history: &ValidatorHistory) -> u32 {
    let history = &validator_history.history;
    let default = ValidatorHistoryEntry::default();
    let mut count = 0;
    for entry in history.arr.iter() {
        if entry.epoch == default.epoch {
            continue;
        }
        if entry.epoch_credits > 0 && entry.epoch_credits != default.epoch_credits {
            count += 1;
        }
    }
    count
}

/// Compute validator ages by combining oracle data and onchain data
///
/// Count oracle data up to the first onchain epoch
/// and then onchain data from the first onchain epoch onwards
fn compute_validator_ages(
    oracle: &[SourceData],
    validator_histories: &[ValidatorHistory],
) -> Vec<(Pubkey, u32)> {
    // Group oracle data by vote account
    let mut oracle_data_by_validator: HashMap<Pubkey, Vec<&SourceData>> = HashMap::new();
    for data in oracle {
        oracle_data_by_validator
            .entry(data.vote_account)
            .or_default()
            .push(data);
    }
    // Process each validator
    let mut validator_ages: Vec<(Pubkey, u32)> = Vec::new();
    for history in validator_histories.iter() {
        // Find first epoch onchain with credits
        let first_onchain_epoch = find_first_epoch_with_credits(history);
        // Count epochs
        let mut total_epochs = 0u32;
        if let Some(first_epoch) = first_onchain_epoch {
            // Count oracle epochs
            if let Some(oracle_data) = oracle_data_by_validator.get(&history.vote_account) {
                for data in oracle_data {
                    if data.credits > 0 && data.epoch < first_epoch {
                        total_epochs += 1;
                    }
                }
            }
            // Count onchain epochs
            let onchain_epochs = count_onchain_epochs_with_credits(history);
            total_epochs += onchain_epochs;
        } else {
            // No onchain data, use all oracle data
            if let Some(oracle_data) = oracle_data_by_validator.get(&history.vote_account) {
                for data in oracle_data {
                    if data.credits > 0 {
                        total_epochs += 1;
                    }
                }
            }
        }
        if total_epochs > 0 {
            validator_ages.push((history.vote_account, total_epochs));
        }
    }
    validator_ages
}

fn read_oracle_data(path: PathBuf) -> Vec<SourceData> {
    // Open file
    let file =
        File::open(&path).unwrap_or_else(|e| panic!("Failed to open file {path:?}: {e}"));
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
            panic!("Skipping invalid line: {line}");
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
