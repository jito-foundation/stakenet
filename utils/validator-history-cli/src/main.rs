use std::{collections::HashMap, path::PathBuf, thread::sleep, time::Duration};

use anchor_lang::{
    AccountDeserialize, AnchorDeserialize, Discriminator, InstructionData, ToAccountMetas,
};
use clap::{arg, command, Parser, Subcommand};
use ipinfo::{BatchReqOpts, IpInfo, IpInfoConfig};
use solana_client::{
    rpc_client::RpcClient,
    rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig},
    rpc_filter::{Memcmp, RpcFilterType},
};
use solana_program::instruction::Instruction;
use solana_sdk::{
    pubkey::Pubkey, signature::read_keypair_file, signer::Signer, transaction::Transaction,
};
use spl_stake_pool::state::{StakePool, ValidatorList};
use validator_history::{
    constants::MAX_ALLOC_BYTES, ClusterHistory, ClusterHistoryEntry, Config, ValidatorHistory,
    ValidatorHistoryEntry,
};

#[derive(Parser)]
#[command(about = "CLI for validator history program")]
struct Args {
    /// RPC URL for the cluster
    #[arg(
        short,
        long,
        env,
        default_value = "https://api.mainnet-beta.solana.com"
    )]
    json_rpc_url: String,

    #[command(subcommand)]
    commands: Commands,
}

#[derive(Subcommand)]
enum Commands {
    InitConfig(InitConfig),
    InitClusterHistory(InitClusterHistory),
    CrankerStatus(CrankerStatus),
    ClusterHistoryStatus,
    History(History),
    BackfillClusterHistory(BackfillClusterHistory),
    StakeByCountry(StakeByCountry),
}

#[derive(Parser)]
#[command(about = "Initialize config account")]
struct InitConfig {
    /// Path to keypair used to pay for account creation and execute transactions
    #[arg(short, long, env, default_value = "~/.config/solana/id.json")]
    keypair_path: PathBuf,

    /// Tip distribution program ID (Pubkey as base58 string)
    #[arg(long, env)]
    tip_distribution_program_id: Pubkey,

    /// New tip distribution authority (Pubkey as base58 string)
    ///
    /// If not provided, the initial keypair will be the authority
    #[arg(long, env, required(false))]
    tip_distribution_authority: Option<Pubkey>,

    // New stake authority (Pubkey as base58 string)
    ///
    /// If not provided, the initial keypair will be the authority
    #[arg(short, long, env, required(false))]
    stake_authority: Option<Pubkey>,
}

#[derive(Parser)]
#[command(about = "Initialize cluster history account")]
struct InitClusterHistory {
    /// Path to keypair used to pay for account creation and execute transactions
    #[arg(short, long, env, default_value = "~/.config/solana/id.json")]
    keypair_path: PathBuf,
}

#[derive(Parser, Debug)]
#[command(about = "Get cranker status")]
struct CrankerStatus {
    /// Epoch to get status for (default: current epoch)
    #[arg(short, long, env)]
    epoch: Option<u64>,
}

#[derive(Parser)]
#[command(about = "Get validator history")]
struct History {
    /// Validator to get history for
    validator: Pubkey,

    /// Start epoch
    #[arg(short, long, env)]
    start_epoch: Option<u64>,
}

#[derive(Parser)]
#[command(about = "Backfill cluster history")]
struct BackfillClusterHistory {
    /// Path to keypair used to pay for account creation and execute transactions
    #[arg(short, long, env, default_value = "~/.config/solana/id.json")]
    keypair_path: PathBuf,

    /// Epoch to backfill
    #[arg(short, long, env)]
    epoch: u64,

    /// Number of blocks in epoch
    #[arg(short, long, env)]
    blocks_in_epoch: u32,
}

#[derive(Parser)]
#[command(about = "Display JitoSOL stake percentage by country")]
struct StakeByCountry {
    /// Stake pool address
    #[arg(short, long, env)]
    stake_pool: Pubkey,

    /// Stake pool address
    #[arg(short, long, env)]
    country: Option<String>,

    /// IP Info Token
    #[arg(short, long, env)]
    ip_info_token: String,
}

fn command_init_config(args: InitConfig, client: RpcClient) {
    // Creates config account, sets tip distribution program address, and optionally sets authority for commission history program
    let keypair = read_keypair_file(args.keypair_path).expect("Failed reading keypair file");

    let mut instructions = vec![];
    let (config_pda, _) = Pubkey::find_program_address(&[Config::SEED], &validator_history::ID);
    instructions.push(Instruction {
        program_id: validator_history::ID,
        accounts: validator_history::accounts::InitializeConfig {
            config: config_pda,
            system_program: solana_program::system_program::id(),
            signer: keypair.pubkey(),
        }
        .to_account_metas(None),
        data: validator_history::instruction::InitializeConfig {
            authority: keypair.pubkey(),
        }
        .data(),
    });

    instructions.push(Instruction {
        program_id: validator_history::ID,
        accounts: validator_history::accounts::SetNewTipDistributionProgram {
            config: config_pda,
            new_tip_distribution_program: args.tip_distribution_program_id,
            admin: keypair.pubkey(),
        }
        .to_account_metas(None),
        data: validator_history::instruction::SetNewTipDistributionProgram {}.data(),
    });

    if let Some(new_authority) = args.tip_distribution_authority {
        instructions.push(Instruction {
            program_id: validator_history::ID,
            accounts: validator_history::accounts::SetNewAdmin {
                config: config_pda,
                new_admin: new_authority,
                admin: keypair.pubkey(),
            }
            .to_account_metas(None),
            data: validator_history::instruction::SetNewAdmin {}.data(),
        });
    }

    if let Some(new_authority) = args.stake_authority {
        instructions.push(Instruction {
            program_id: validator_history::ID,
            accounts: validator_history::accounts::SetNewOracleAuthority {
                config: config_pda,
                new_oracle_authority: new_authority,
                admin: keypair.pubkey(),
            }
            .to_account_metas(None),
            data: validator_history::instruction::SetNewOracleAuthority {}.data(),
        });
    }

    let blockhash = client
        .get_latest_blockhash()
        .expect("Failed to get recent blockhash");
    let transaction = Transaction::new_signed_with_payer(
        &instructions,
        Some(&keypair.pubkey()),
        &[&keypair],
        blockhash,
    );

    let signature = client
        .send_and_confirm_transaction_with_spinner(&transaction)
        .expect("Failed to send transaction");
    println!("Signature: {}", signature);
}

fn command_init_cluster_history(args: InitClusterHistory, client: RpcClient) {
    // Creates cluster history account
    let keypair = read_keypair_file(args.keypair_path).expect("Failed reading keypair file");

    let mut instructions = vec![];
    let (cluster_history_pda, _) =
        Pubkey::find_program_address(&[ClusterHistory::SEED], &validator_history::ID);
    instructions.push(Instruction {
        program_id: validator_history::ID,
        accounts: validator_history::accounts::InitializeClusterHistoryAccount {
            cluster_history_account: cluster_history_pda,
            system_program: solana_program::system_program::id(),
            signer: keypair.pubkey(),
        }
        .to_account_metas(None),
        data: validator_history::instruction::InitializeClusterHistoryAccount {}.data(),
    });
    // Realloc insturctions
    let num_reallocs = (ClusterHistory::SIZE - MAX_ALLOC_BYTES) / MAX_ALLOC_BYTES + 1;
    instructions.extend(vec![
        Instruction {
            program_id: validator_history::ID,
            accounts: validator_history::accounts::ReallocClusterHistoryAccount {
                cluster_history_account: cluster_history_pda,
                system_program: solana_program::system_program::id(),
                signer: keypair.pubkey(),
            }
            .to_account_metas(None),
            data: validator_history::instruction::ReallocClusterHistoryAccount {}.data(),
        };
        num_reallocs
    ]);

    let blockhash = client
        .get_latest_blockhash()
        .expect("Failed to get recent blockhash");
    let transaction = Transaction::new_signed_with_payer(
        &instructions,
        Some(&keypair.pubkey()),
        &[&keypair],
        blockhash,
    );

    let signature = client
        .send_and_confirm_transaction_with_spinner(&transaction)
        .expect("Failed to send transaction");
    println!("Signature: {}", signature);
}

fn get_entry(validator_history: ValidatorHistory, epoch: u64) -> Option<ValidatorHistoryEntry> {
    // Util to fetch an entry for a specific epoch
    validator_history
        .history
        .arr
        .into_iter()
        .find(|entry| entry.epoch == epoch as u16)
}

fn formatted_entry(entry: ValidatorHistoryEntry) -> String {
    let commission_str = if entry.commission == ValidatorHistoryEntry::default().commission {
        "[NULL]".to_string()
    } else {
        entry.commission.to_string()
    };

    let epoch_credits_str = if entry.epoch_credits == ValidatorHistoryEntry::default().epoch_credits
    {
        "[NULL]".to_string()
    } else {
        entry.epoch_credits.to_string()
    };

    let mev_commission_str =
        if entry.mev_commission == ValidatorHistoryEntry::default().mev_commission {
            "[NULL]".to_string()
        } else {
            entry.mev_commission.to_string()
        };

    let mev_earned_str = if entry.mev_earned == ValidatorHistoryEntry::default().mev_earned {
        "[NULL]".to_string()
    } else {
        (entry.mev_earned as f64 / 100.0).to_string()
    };

    let stake_str = if entry.activated_stake_lamports
        == ValidatorHistoryEntry::default().activated_stake_lamports
    {
        "[NULL]".to_string()
    } else {
        entry.activated_stake_lamports.to_string()
    };

    let ip_str = if entry.ip == ValidatorHistoryEntry::default().ip {
        "[NULL]".to_string()
    } else {
        format!(
            "{}.{}.{}.{}",
            entry.ip[0], entry.ip[1], entry.ip[2], entry.ip[3]
        )
    };

    let client_type_str = if entry.client_type == ValidatorHistoryEntry::default().client_type {
        "[NULL]".to_string()
    } else {
        entry.client_type.to_string()
    };

    let client_version_str = if entry.version.major
        == ValidatorHistoryEntry::default().version.major
        && entry.version.minor == ValidatorHistoryEntry::default().version.minor
        && entry.version.patch == ValidatorHistoryEntry::default().version.patch
    {
        "[NULL]".to_string()
    } else {
        format!(
            "{}.{}.{}",
            entry.version.major, entry.version.minor, entry.version.patch
        )
    };

    let rank_str = if entry.rank == ValidatorHistoryEntry::default().rank {
        "[NULL]".to_string()
    } else {
        entry.rank.to_string()
    };

    let superminority_str =
        if entry.is_superminority == ValidatorHistoryEntry::default().is_superminority {
            "[NULL]".to_string()
        } else {
            entry.is_superminority.to_string()
        };

    let last_update_slot = if entry.vote_account_last_update_slot
        == ValidatorHistoryEntry::default().vote_account_last_update_slot
    {
        "[NULL]".to_string()
    } else {
        entry.vote_account_last_update_slot.to_string()
    };

    format!(
        "Commission: {}\t| Epoch Credits: {}\t| MEV Commission: {}\t| MEV Earned: {}\t| Stake: {}\t| Rank: {}\t| Superminority: {}\t| IP: {}\t| Client Type: {}\t| Client Version: {}\t| Last Updated: {}",
        commission_str,
        epoch_credits_str,
        mev_commission_str,
        mev_earned_str,
        stake_str,
        rank_str,
        superminority_str,
        ip_str,
        client_type_str,
        client_version_str,
        last_update_slot
    )
}

fn command_cranker_status(args: CrankerStatus, client: RpcClient) {
    // Displays current epoch ValidatorHistory entry for each validator, and summary of updated fields
    let epoch = args.epoch.unwrap_or_else(|| {
        client
            .get_epoch_info()
            .expect("Failed to get epoch info")
            .epoch
    });

    // Config account
    let (config_pda, _) = Pubkey::find_program_address(&[Config::SEED], &validator_history::ID);
    // Fetch config account
    let config_account = client
        .get_account(&config_pda)
        .expect("Failed to get config account");
    let config = Config::try_deserialize(&mut config_account.data.as_slice())
        .expect("Failed to deserialize config account");

    // Fetch every ValidatorHistory account
    let gpa_config = RpcProgramAccountsConfig {
        filters: Some(vec![RpcFilterType::Memcmp(Memcmp::new_raw_bytes(
            0,
            ValidatorHistory::discriminator().into(),
        ))]),
        account_config: RpcAccountInfoConfig {
            encoding: Some(solana_account_decoder::UiAccountEncoding::Base64),
            ..RpcAccountInfoConfig::default()
        },
        ..RpcProgramAccountsConfig::default()
    };
    let mut validator_history_accounts = client
        .get_program_accounts_with_config(&validator_history::id(), gpa_config)
        .expect("Failed to get validator history accounts");

    let mut validator_histories = validator_history_accounts
        .iter_mut()
        .map(|(_, account)| {
            let validator_history = ValidatorHistory::try_deserialize(&mut account.data.as_slice())
                .expect("Failed to deserialize validator history account");
            validator_history
        })
        .collect::<Vec<_>>();

    assert_eq!(
        validator_histories.len(),
        config.counter as usize,
        "Number of validator history accounts does not match config counter"
    );

    validator_histories.sort_by(|a, b| a.index.cmp(&b.index));

    // For each validator history account, print out the status
    println!("Epoch {} Report", epoch);
    let mut ips = 0;
    let mut versions = 0;
    let mut types = 0;
    let mut mev_comms = 0;
    let mut mev_earned = 0;
    let mut comms = 0;
    let mut epoch_credits = 0;
    let mut stakes = 0;
    let mut ranks = 0;

    let default = ValidatorHistoryEntry::default();
    for validator_history in validator_histories {
        match get_entry(validator_history, epoch) {
            Some(entry) => {
                if entry.ip != default.ip {
                    ips += 1;
                }
                if !(entry.version.major == default.version.major
                    && entry.version.minor == default.version.minor
                    && entry.version.patch == default.version.patch)
                {
                    versions += 1;
                }
                if entry.client_type != default.client_type {
                    types += 1;
                }
                if entry.mev_commission != default.mev_commission {
                    mev_comms += 1;
                }
                if entry.mev_earned != default.mev_earned {
                    mev_earned += 1;
                }
                if entry.commission != default.commission {
                    comms += 1;
                }
                if entry.epoch_credits != default.epoch_credits {
                    epoch_credits += 1;
                }
                if entry.activated_stake_lamports != default.activated_stake_lamports {
                    stakes += 1;
                }
                if entry.rank != default.rank {
                    ranks += 1;
                }
                println!(
                    "{}.\tVote Account: {} | {}",
                    validator_history.index,
                    validator_history.vote_account,
                    formatted_entry(entry)
                );
            }
            None => {
                println!(
                    "{}.\tVote Account: {} | {}",
                    validator_history.index,
                    validator_history.vote_account,
                    formatted_entry(ValidatorHistoryEntry::default())
                );
            }
        };
    }
    println!("Total Validators:\t\t{}", config.counter);
    println!("Validators with IP:\t\t{}", ips);
    println!("Validators with Version:\t{}", versions);
    println!("Validators with Client Type:\t{}", types);
    println!("Validators with MEV Commission: {}", mev_comms);
    println!("Validators with MEV Earned: \t{}", mev_earned);
    println!("Validators with Commission:\t{}", comms);
    println!("Validators with Epoch Credits:\t{}", epoch_credits);
    println!("Validators with Stake:\t\t{}", stakes);
    println!("Validators with Rank:\t\t{}", ranks);
}

fn command_history(args: History, client: RpcClient) {
    // Get single validator history account and display all epochs of history
    let (validator_history_pda, _) = Pubkey::find_program_address(
        &[ValidatorHistory::SEED, args.validator.as_ref()],
        &validator_history::ID,
    );
    let validator_history_account = client
        .get_account(&validator_history_pda)
        .expect("Failed to get validator history account");
    let validator_history =
        ValidatorHistory::try_deserialize(&mut validator_history_account.data.as_slice())
            .expect("Failed to deserialize validator history account");
    let start_epoch = args.start_epoch.unwrap_or_else(|| {
        validator_history
            .history
            .arr
            .iter()
            .filter_map(|entry| {
                if entry.epoch > 0 {
                    Some(entry.epoch as u64)
                } else {
                    None
                }
            })
            .min()
            .unwrap_or(0)
    });
    let current_epoch = client
        .get_epoch_info()
        .expect("Failed to get epoch info")
        .epoch;
    println!(
        "History for validator {} | Validator History Account {}",
        args.validator, validator_history_pda
    );
    for epoch in start_epoch..=current_epoch {
        match get_entry(validator_history, epoch) {
            Some(entry) => {
                println!("Epoch: {} | {}", epoch, formatted_entry(entry));
            }
            None => {
                println!("Epoch {}:\tNo history", epoch);
                continue;
            }
        }
    }
}

fn command_cluster_history(client: RpcClient) {
    let (cluster_history_pda, _) =
        Pubkey::find_program_address(&[ClusterHistory::SEED], &validator_history::ID);

    let cluster_history_account = client
        .get_account(&cluster_history_pda)
        .expect("Failed to get cluster history account");
    let cluster_history =
        ClusterHistory::try_deserialize(&mut cluster_history_account.data.as_slice())
            .expect("Failed to deserialize cluster history account");

    for entry in cluster_history.history.arr.iter() {
        println!(
            "Epoch: {} | Total Blocks: {}",
            entry.epoch, entry.total_blocks
        );

        if entry.epoch == ClusterHistoryEntry::default().epoch {
            break;
        }
    }
}

fn command_backfill_cluster_history(args: BackfillClusterHistory, client: RpcClient) {
    // Backfill cluster history account for a specific epoch
    let keypair = read_keypair_file(args.keypair_path).expect("Failed reading keypair file");
    sleep(Duration::from_secs(5));

    let mut instructions = vec![];
    let (cluster_history_pda, _) =
        Pubkey::find_program_address(&[ClusterHistory::SEED], &validator_history::ID);
    let (config, _) = Pubkey::find_program_address(&[Config::SEED], &validator_history::ID);
    let cluster_history_account = client
        .get_account(&cluster_history_pda)
        .expect("Failed to get cluster history account");
    let cluster_history =
        ClusterHistory::try_deserialize(&mut cluster_history_account.data.as_slice())
            .expect("Failed to deserialize cluster history account");

    if !cluster_history.history.is_empty()
        && cluster_history.history.last().unwrap().epoch + 1 != args.epoch as u16
    {
        panic!("Cannot set this epoch, you would mess up the ordering");
    }

    instructions.push(Instruction {
        program_id: validator_history::ID,
        accounts: validator_history::accounts::BackfillTotalBlocks {
            cluster_history_account: cluster_history_pda,
            config,
            oracle_authority: keypair.pubkey(),
        }
        .to_account_metas(None),
        data: validator_history::instruction::BackfillTotalBlocks {
            epoch: args.epoch,
            blocks_in_epoch: args.blocks_in_epoch,
        }
        .data(),
    });

    let blockhash = client
        .get_latest_blockhash()
        .expect("Failed to get recent blockhash");
    let transaction = Transaction::new_signed_with_payer(
        &instructions,
        Some(&keypair.pubkey()),
        &[&keypair],
        blockhash,
    );

    let signature = client
        .send_and_confirm_transaction_with_spinner(&transaction)
        .expect("Failed to send transaction");
    println!("Signature: {}", signature);
}

async fn command_stake_by_country(args: StakeByCountry, client: RpcClient) {
    let ip_config = IpInfoConfig {
        token: Some(args.ip_info_token),
        ..Default::default()
    };

    let stake_pool_acc_raw = match client.get_account(&args.stake_pool) {
        Ok(account) => account,
        Err(err) => {
            eprintln!("Error fetching stake pool account: {err}");
            return;
        }
    };
    let stake_pool = match StakePool::deserialize(&mut stake_pool_acc_raw.data.as_slice()) {
        Ok(pool) => pool,
        Err(err) => {
            eprintln!("Error deserializing stake pool: {err}");
            return;
        }
    };

    let validator_list_acc_raw = match client.get_account(&stake_pool.validator_list) {
        Ok(account) => account,
        Err(err) => {
            eprintln!("Error fetching validator list account: {err}");
            return;
        }
    };
    let validator_list =
        match ValidatorList::deserialize(&mut validator_list_acc_raw.data.as_slice()) {
            Ok(list) => list,
            Err(err) => {
                eprintln!("Error deserializing validator list: {err}");
                return;
            }
        };

    let validator_count = validator_list.validators.len();
    println!("Processing {validator_count} validators...");

    let mut ip_info = match IpInfo::new(ip_config) {
        Ok(ip_info) => ip_info,
        Err(err) => {
            eprintln!("Error initializing ip info: {err}");
            return;
        }
    };

    let mut validator_map: HashMap<Pubkey, u64> = HashMap::new();

    // Group validators by chunks for batch processing
    let validator_history_pdas: Vec<Vec<Pubkey>> = validator_list
        .validators
        .into_iter()
        .map(|validator| {
            let stake_lamports = validator.stake_lamports().unwrap();
            validator_map
                .entry(validator.vote_account_address)
                .and_modify(|stake| *stake += stake_lamports)
                .or_insert(stake_lamports);
            let (validator_history_pda, _) = Pubkey::find_program_address(
                &[
                    ValidatorHistory::SEED,
                    validator.vote_account_address.as_ref(),
                ],
                &validator_history::ID,
            );
            validator_history_pda
        })
        .collect::<Vec<Pubkey>>()
        .chunks(100)
        .map(|chunk| chunk.to_vec())
        .collect();

    let mut validator_ip_map: HashMap<Pubkey, String> = HashMap::new();
    let mut country_map: HashMap<String, u64> = HashMap::new();

    for (chunk_idx, validator_history_pda_chunk) in validator_history_pdas.iter().enumerate() {
        println!(
            "Processing chunk {}/{} with {} validator histories",
            chunk_idx + 1,
            validator_history_pdas.len(),
            validator_history_pda_chunk.len()
        );

        let validator_history_acc_raws =
            match client.get_multiple_accounts(validator_history_pda_chunk) {
                Ok(accounts) => accounts,
                Err(err) => {
                    eprintln!("Error fetching validator history accounts: {err}");
                    continue;
                }
            };

        let validator_histories: Vec<ValidatorHistory> = validator_history_acc_raws
            .iter()
            .enumerate()
            .filter_map(|(i, validator_history_acc)| {
                if let Some(validator_history_account) = validator_history_acc {
                    match ValidatorHistory::try_deserialize(
                        &mut validator_history_account.data.as_slice(),
                    ) {
                        Ok(history) => Some(history),
                        Err(err) => {
                            eprintln!("Error deserializing validator history at index {i}: {err}");
                            None
                        }
                    }
                } else {
                    // Account not found
                    None
                }
            })
            .collect();

        println!(
            "Found {} valid validator histories",
            validator_histories.len()
        );

        let validator_ips: Vec<String> = validator_histories
            .iter()
            .filter_map(|validator_history| {
                if let Some(latest_history) = validator_history.history.last() {
                    let ip_addr = std::net::IpAddr::from(latest_history.ip);
                    validator_ip_map.insert(validator_history.vote_account, ip_addr.to_string());
                    Some(ip_addr.to_string())
                } else {
                    None
                }
            })
            .collect();

        if validator_ips.is_empty() {
            println!("No valid IPs found in this batch, skipping...");
            continue;
        }

        println!("Looking up {} IPs...", validator_ips.len());

        let ip_strs: Vec<&str> = validator_ips.iter().map(|s| s.as_str()).collect();
        if let Ok(batch_results) = ip_info
            .lookup_batch(&ip_strs, BatchReqOpts::default())
            .await
        {
            println!(
                "Successfully retrieved country data for {} IPs",
                batch_results.len()
            );
            // Process the results immediately within this loop iteration
            for (vote_account, ip_address) in validator_ip_map.iter() {
                if let Some(stake_amount) = validator_map.get(vote_account) {
                    if let Some(ip_details) = batch_results.get(ip_address) {
                        match &ip_details.country_name {
                            Some(country_name) => {
                                country_map
                                    .entry(country_name.clone())
                                    .and_modify(|amount| *amount += stake_amount)
                                    .or_insert(*stake_amount);
                            }
                            None => {
                                // Country name not available
                                eprintln!("No country data for IP {}", ip_address);
                            }
                        }
                    }
                }
            }
        }
    }

    if country_map.is_empty() {
        println!("No data collected. Please check error messages above.");
        return;
    }

    let total_stake: u64 = country_map.values().sum();

    match &args.country {
        Some(country) => {
            println!("JitoSOL Stake for Country: {country}");
            match country_map.get(country) {
                Some(stake) => {
                    let percentage = (*stake as f64 / total_stake as f64) * 100.0;
                    println!("Lamports: {stake}, Percentage: {:.2}%", percentage);
                }
                None => {
                    println!("Country not found: {}", country);
                    println!(
                        "Available countries: {}",
                        country_map
                            .keys()
                            .map(|key| key.as_str())
                            .collect::<Vec<&str>>()
                            .join(", ")
                    );
                }
            }
        }
        None => {
            let mut countries: Vec<(&String, &u64)> = country_map.iter().collect();
            countries.sort_by(|a, b| b.1.cmp(a.1));

            println!("JitoSOL Stake by Country (Sorted by Percentage):");
            println!("Total stake: {total_stake} lamports");
            for (country, stake) in countries {
                let percentage = (*stake as f64 / total_stake as f64) * 100.0;
                println!(
                    "Country: {}, Lamports: {}, Percentage: {:.2}%",
                    country, stake, percentage
                );
            }
        }
    }
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let client = RpcClient::new_with_timeout(args.json_rpc_url.clone(), Duration::from_secs(60));
    match args.commands {
        Commands::InitConfig(args) => command_init_config(args, client),
        Commands::CrankerStatus(args) => command_cranker_status(args, client),
        Commands::InitClusterHistory(args) => command_init_cluster_history(args, client),
        Commands::ClusterHistoryStatus => command_cluster_history(client),
        Commands::History(args) => command_history(args, client),
        Commands::BackfillClusterHistory(args) => command_backfill_cluster_history(args, client),
        Commands::StakeByCountry(args) => command_stake_by_country(args, client).await,
    };
}
