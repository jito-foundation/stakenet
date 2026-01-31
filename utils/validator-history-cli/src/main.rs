use anchor_lang::{AccountDeserialize, Discriminator, InstructionData, ToAccountMetas};
use clap::{arg, command, Parser, Subcommand, ValueEnum};
use dotenvy::dotenv;
use ipinfo::{BatchReqOpts, IpInfo, IpInfoConfig};
use rusqlite::Connection;
use solana_client::{
    rpc_client::RpcClient,
    rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig},
    rpc_filter::{Memcmp, RpcFilterType},
};
use solana_program::instruction::Instruction;
use solana_sdk::{
    commitment_config::CommitmentConfig, pubkey::Pubkey, signature::read_keypair_file,
    signer::Signer, transaction::Transaction,
};

/// Commitment level for RPC queries
#[derive(Debug, Clone, Copy, Default, ValueEnum)]
pub enum CommitmentLevel {
    Processed,
    #[default]
    Confirmed,
    Finalized,
}

impl From<CommitmentLevel> for CommitmentConfig {
    fn from(level: CommitmentLevel) -> Self {
        match level {
            CommitmentLevel::Processed => CommitmentConfig::processed(),
            CommitmentLevel::Confirmed => CommitmentConfig::confirmed(),
            CommitmentLevel::Finalized => CommitmentConfig::finalized(),
        }
    }
}
use spl_stake_pool::state::{StakePool, ValidatorList};
use stakenet_keeper::operations::block_metadata::db::DBSlotInfo;
use std::{collections::HashMap, path::PathBuf, sync::Arc, thread::sleep, time::Duration};
use validator_history::{
    constants::MAX_ALLOC_BYTES, ClusterHistory, ClusterHistoryEntry, Config, ValidatorHistory,
    ValidatorHistoryEntry,
};
use validator_history_cli::{
    commands::{
        self,
        cranks::{
            copy_cluster_info::CrankCopyClusterInfo,
            copy_gossip_contact_info::CrankCopyGossipContactInfo,
            copy_tip_distribution_account::CrankCopyTipDistributionAccount,
            copy_vote_account::CrankCopyVoteAccount,
        },
    },
    validator_history_entry_output::ValidatorHistoryEntryOutput,
};

#[derive(Parser)]
#[command(about = "CLI for validator history program", version)]
struct Args {
    /// RPC URL for the cluster
    #[arg(
        short,
        long,
        env,
        default_value = "https://api.mainnet-beta.solana.com"
    )]
    json_rpc_url: String,

    /// Commitment level for RPC queries
    #[arg(long, global = true, env, default_value = "confirmed")]
    commitment: CommitmentLevel,

    #[command(subcommand)]
    commands: Commands,
}

#[derive(Subcommand)]
enum Commands {
    InitConfig(InitConfig),
    ReallocConfig(ReallocConfig),
    InitClusterHistory(InitClusterHistory),
    CrankerStatus(CrankerStatus),
    ClusterHistoryStatus(ClusterHistoryStatus),
    ViewConfig,
    History(History),
    BackfillClusterHistory(BackfillClusterHistory),
    BackfillValidatorAge(commands::backfill_validator_age::BackfillValidatorAge),
    StakeByCountry(StakeByCountry),
    GetConfig,
    UpdateOracleAuthority(UpdateOracleAuthority),
    DunePriorityFeeBackfill(DunePriorityFeeBackfill),
    UploadValidatorAge(UploadValidatorAge),

    // Cranks
    CrankCopyClusterInfo(CrankCopyClusterInfo),
    CrankCopyGossipContactInfo(CrankCopyGossipContactInfo),
    CrankCopyTipDistributionAccount(CrankCopyTipDistributionAccount),
    CrankCopyVoteAccount(CrankCopyVoteAccount),
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
#[command(about = "Realloc Config")]
struct ReallocConfig {
    /// Path to keypair used to pay for account creation and execute transactions
    #[arg(short, long, env, default_value = "~/.config/solana/id.json")]
    keypair_path: PathBuf,
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

    /// Print account information in JSON format
    #[arg(
        long,
        default_value = "false",
        help = "This will print out account information in JSON format"
    )]
    pub print_json: bool,
}

#[derive(Parser)]
#[command(about = "Get cluster history status")]
struct ClusterHistoryStatus {
    /// Print account information in JSON format
    #[arg(
        long,
        default_value = "false",
        help = "This will print out account information in JSON format"
    )]
    pub print_json: bool,
}

#[derive(Parser)]
#[command(about = "Get validator history")]
struct History {
    /// Validator to get history for
    validator: Pubkey,

    /// Start epoch
    #[arg(short, long, env)]
    start_epoch: Option<u64>,

    /// Print account information in JSON format
    #[arg(
        long,
        default_value = "false",
        help = "This will print out account information in JSON format"
    )]
    pub print_json: bool,
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
#[command(about = "Update oracle authority")]
struct UpdateOracleAuthority {
    /// Path to keypair used to pay for account creation and execute transactions
    #[arg(short, long, env, default_value = "~/.config/solana/id.json")]
    keypair_path: PathBuf,

    /// New oracle authority (Pubkey as base58 string)
    #[arg(long, env)]
    oracle_authority: Pubkey,
}

#[derive(Parser)]
#[command(about = "Backfills the Priority Fee DB from Dune from the last 99 epochs")]
struct DunePriorityFeeBackfill {
    /// Path to the local SQLite file
    #[arg(long, env, default_value = "../../keepers/block_keeper.db3")]
    pub sqlite_path: PathBuf,

    /// Dune API key
    #[arg(long, env)]
    dune_api_key: String,

    #[arg(long, env, default_value = "5598354")]
    query_id: String,

    #[arg(long, env, default_value_t = 32000)]
    batch_size: usize,

    #[arg(long, env, default_value_t = 32000)]
    chunk_size: usize,

    #[arg(long, env, default_value_t = 0)]
    starting_offset: usize,
}

#[derive(Parser)]
#[command(about = "Upload validator age for a specific vote account")]
struct UploadValidatorAge {
    /// Path to oracle authority keypair
    #[arg(short, long, env, default_value = "~/.config/solana/id.json")]
    keypair_path: PathBuf,

    /// Vote account pubkey to update
    #[arg(short, long, env)]
    vote_account: Pubkey,

    /// Validator age value to set
    #[arg(short, long, env)]
    age: u32,

    /// Epoch when validator age was last updated (defaults to current epoch)
    #[arg(short, long, env)]
    epoch: Option<u16>,
}

#[derive(Parser)]
#[command(about = "Get Config info")]
struct GetConfig {}

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

fn command_realloc_config(args: ReallocConfig, client: RpcClient) {
    let keypair = read_keypair_file(args.keypair_path).expect("Failed reading keypair file");
    let (config_pda, _) = Pubkey::find_program_address(&[Config::SEED], &validator_history::ID);

    let instructions = vec![Instruction {
        program_id: validator_history::ID,
        accounts: validator_history::accounts::ReallocConfigAccount {
            config_account: config_pda,
            system_program: solana_program::system_program::id(),
            payer: keypair.pubkey(),
        }
        .to_account_metas(None),
        data: validator_history::instruction::ReallocConfigAccount {}.data(),
    }];

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

fn format_option(opt: Option<String>) -> String {
    opt.unwrap_or_else(|| "None".to_string())
}

fn formatted_entry(entry: ValidatorHistoryEntry, print_json: bool) -> String {
    let entry_output = ValidatorHistoryEntryOutput::from(entry);

    if print_json {
        serde_json::to_string_pretty(&entry_output).unwrap_or_else(|_| "{}".to_string())
    } else {
        let mut field_descriptions = Vec::new();

        field_descriptions.push(format!(
            "Activated Stake Lamports: {}",
            format_option(entry_output.activated_stake_lamports)
        ));
        field_descriptions.push(format!(
            "MEV Commission: {}",
            format_option(entry_output.mev_commission)
        ));
        field_descriptions.push(format!(
            "Epoch Credits: {}",
            format_option(entry_output.epoch_credits)
        ));
        field_descriptions.push(format!(
            "Commission: {}",
            format_option(entry_output.commission)
        ));
        field_descriptions.push(format!(
            "Client Type: {}",
            format_option(entry_output.client_type)
        ));
        field_descriptions.push(format!(
            "Client Version: {}",
            format_option(entry_output.version)
        ));
        field_descriptions.push(format!("IP: {}", format_option(entry_output.ip)));
        field_descriptions.push(format!(
            "Merkle Root Upload Authority: {}",
            format_option(entry_output.merkle_root_upload_authority)
        ));
        field_descriptions.push(format!(
            "Superminority: {}",
            format_option(entry_output.is_superminority)
        ));
        field_descriptions.push(format!("Rank: {}", format_option(entry_output.rank)));
        field_descriptions.push(format!(
            "Last Update: {}",
            format_option(entry_output.vote_account_last_update_slot)
        ));
        field_descriptions.push(format!(
            "MEV Earned: {}",
            format_option(entry_output.mev_earned)
        ));
        field_descriptions.push(format!(
            "Priority Fee Commission: {}",
            format_option(entry_output.priority_fee_commission)
        ));
        field_descriptions.push(format!(
            "Priority Fee Tips: {}",
            format_option(entry_output.priority_fee_tips)
        ));
        field_descriptions.push(format!(
            "Total Priority Fees: {}",
            format_option(entry_output.total_priority_fees)
        ));
        field_descriptions.push(format!(
            "Total Leader Slots: {}",
            format_option(entry_output.total_leader_slots)
        ));
        field_descriptions.push(format!(
            "Blocks Produced: {}",
            format_option(entry_output.blocks_produced)
        ));
        field_descriptions.push(format!(
            "Block Data Updated At Slot: {}",
            format_option(entry_output.block_data_updated_at_slot)
        ));
        field_descriptions.push(format!(
            "Priority Fee Merkle Root Upload Authority: {}",
            format_option(entry_output.priority_fee_merkle_root_upload_authority)
        ));

        field_descriptions.join(" | ")
    }
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
            ValidatorHistory::DISCRIMINATOR.into(),
        ))]),
        account_config: RpcAccountInfoConfig {
            encoding: Some(solana_account_decoder::UiAccountEncoding::Base64),
            ..RpcAccountInfoConfig::default()
        },
        ..RpcProgramAccountsConfig::default()
    };
    let validator_history_accounts = client
        .get_program_accounts_with_config(&validator_history::id(), gpa_config)
        .expect("Failed to get validator history accounts");

    let mut validator_histories = validator_history_accounts
        .into_iter()
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
    let default = ValidatorHistoryEntry::default();
    let mut results = Vec::with_capacity(validator_histories.len());
    let mut ips = 0;
    let mut versions = 0;
    let mut types = 0;
    let mut mev_comms = 0;
    let mut mev_earned = 0;
    let mut comms = 0;
    let mut epoch_credits = 0;
    let mut stakes = 0;
    let mut ranks = 0;

    if !args.print_json {
        println!("Epoch {epoch} Report");
    }

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

                if args.print_json {
                    let json_str = formatted_entry(entry, args.print_json);
                    match serde_json::from_str::<serde_json::Value>(&json_str) {
                        Ok(validator_data) => {
                            results.push(serde_json::json!({
                                        "vote_account_index":
                            validator_history.index,
                                        "vote_account":
                            validator_history.vote_account.to_string(),
                                        "validator_history": validator_data
                                    }));
                        }
                        Err(_) => {
                            results.push(serde_json::json!({
                                        "vote_account_index":
                            validator_history.index,
                                        "vote_account":
                            validator_history.vote_account.to_string(),
                                "validator_history": json_str
                            }));
                        }
                    }
                } else {
                    println!(
                        "{}.\tVote Account: {} | {}",
                        validator_history.index,
                        validator_history.vote_account,
                        formatted_entry(entry, false)
                    );
                }
            }
            None => {
                if args.print_json {
                    let json_str =
                        formatted_entry(ValidatorHistoryEntry::default(), args.print_json);
                    results.push(serde_json::json!({
                                        "vote_account_index":
                            validator_history.index,
                                        "vote_account":
                            validator_history.vote_account.to_string(),
                        "validator_history": json_str,
                    }));
                } else {
                    println!(
                        "{}.\tVote Account: {} | {}",
                        validator_history.index,
                        validator_history.vote_account,
                        formatted_entry(ValidatorHistoryEntry::default(), false)
                    );
                }
            }
        }
    }

    if args.print_json {
        // Print everything as one JSON object
        let output = serde_json::json!({
            "epoch": epoch,
            "total_validators": config.counter,
            "validators_with_ip": ips,
            "validators_with_version": versions,
            "validators_with_client_type": types,
            "validators_with_mev_commission": mev_comms,
            "validators_with_mev_earned": mev_earned,
            "validators_with_commission": comms,
            "validators_with_epoch_credits": epoch_credits,
            "validators_with_stake": stakes,
            "validators_with_rank": ranks,
            "validators": results,
        });

        println!("{}", serde_json::to_string_pretty(&output).unwrap());
    } else {
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

    if args.print_json {
        let mut results = Vec::new();

        for epoch in start_epoch..=current_epoch {
            match get_entry(validator_history, epoch) {
                Some(entry) => {
                    let json_str = formatted_entry(entry, args.print_json);
                    match serde_json::from_str::<serde_json::Value>(&json_str) {
                        Ok(validator_data) => {
                            results.push(serde_json::json!({
                                "epoch": epoch,
                                "validator_history": validator_data
                            }));
                        }
                        Err(_) => {
                            results.push(serde_json::json!({
                                "epoch": epoch,
                                "validator_history": json_str
                            }));
                        }
                    }
                }
                None => {
                    results.push(serde_json::json!({
                        "epoch": epoch,
                        "validator_history": null
                    }));
                }
            }
        }

        // Print everything as one JSON object
        let output = serde_json::json!({
            "validator": args.validator.to_string(),
            "validator_history_account": validator_history_pda.to_string(),
            "epochs": results
        });

        println!("{}", serde_json::to_string_pretty(&output).unwrap());
    } else {
        println!(
            "History for validator {} | Validator History Account {}",
            args.validator, validator_history_pda
        );
        println!(
            "Validator Age: {} | Validator Age Last Updated Epoch: {}",
            validator_history.validator_age, validator_history.validator_age_last_updated_epoch
        );

        for epoch in start_epoch..=current_epoch {
            match get_entry(validator_history, epoch) {
                Some(entry) => {
                    println!(
                        "Epoch: {} | {}",
                        epoch,
                        formatted_entry(entry, args.print_json)
                    );
                }
                None => {
                    println!("Epoch {}:\tNo history", epoch);
                }
            }
        }
    }
}

fn command_view_config(client: RpcClient) {
    // Get single validator history account and display all epochs of history
    let (config_pda, _) = Pubkey::find_program_address(&[Config::SEED], &validator_history::ID);
    let config_account = client
        .get_account(&config_pda)
        .expect("Failed to get validator history account");
    let config = Config::try_deserialize(&mut config_account.data.as_slice())
        .expect("Failed to deserialize validator history account");
    println!("------- Config -------\n");
    println!("📚 Accounts 📚");
    println!("Admin: {}", config.admin);
    println!("Oracle Authority: {}", config.oracle_authority);
    println!(
        "Tip Distribution Program: {}",
        config.tip_distribution_program
    );
    println!("Config Account: {}\n", config_pda);
    println!("↺ State ↺");
    println!("Validator History Account Counter: {}\n", config.counter);
}

fn command_cluster_history(args: ClusterHistoryStatus, client: RpcClient) {
    let (cluster_history_pda, _) =
        Pubkey::find_program_address(&[ClusterHistory::SEED], &validator_history::ID);

    let cluster_history_account = client
        .get_account(&cluster_history_pda)
        .expect("Failed to get cluster history account");
    let cluster_history =
        ClusterHistory::try_deserialize(&mut cluster_history_account.data.as_slice())
            .expect("Failed to deserialize cluster history account");

    let mut results = Vec::with_capacity(cluster_history.history.arr.len());

    for entry in cluster_history.history.arr.iter() {
        if args.print_json {
            results.push(serde_json::json!({
                "epoch": entry.epoch,
                "total_blocks": entry.total_blocks,
            }));
        } else {
            println!(
                "Epoch: {} | Total Blocks: {}",
                entry.epoch, entry.total_blocks
            );
        }

        if entry.epoch == ClusterHistoryEntry::default().epoch {
            break;
        }
    }

    if args.print_json {
        let output = serde_json::json!({
            "cluster_history_status": results
        });
        println!("{}", serde_json::to_string_pretty(&output).unwrap());
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

fn command_update_oracle_authority(args: UpdateOracleAuthority, client: RpcClient) {
    // Update oracle authority for config account
    let keypair = read_keypair_file(args.keypair_path).expect("Failed reading keypair file");

    let mut instructions = vec![];
    let (config_pda, _) = Pubkey::find_program_address(&[Config::SEED], &validator_history::ID);
    instructions.push(Instruction {
        program_id: validator_history::ID,
        accounts: validator_history::accounts::SetNewOracleAuthority {
            config: config_pda,
            new_oracle_authority: args.oracle_authority,
            admin: keypair.pubkey(),
        }
        .to_account_metas(None),
        data: validator_history::instruction::SetNewOracleAuthority {}.data(),
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
    let stake_pool = match borsh::from_slice::<StakePool>(stake_pool_acc_raw.data.as_slice()) {
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
        match borsh::from_slice::<ValidatorList>(validator_list_acc_raw.data.as_slice()) {
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

fn command_get_config(client: RpcClient) {
    let (config_pda, _) = Pubkey::find_program_address(&[Config::SEED], &validator_history::ID);

    match client.get_account(&config_pda) {
        Ok(account) => match Config::try_deserialize(&mut account.data.as_slice()) {
            Ok(config) => {
                println!("Validator History Config:");
                println!("  Pubkey: {}", config_pda);
                println!(
                    "  Tip Distribution Program: {}",
                    config.tip_distribution_program
                );
                println!(
                    "  Priority Fee Distribution Program: {}",
                    config.priority_fee_distribution_program
                );
                println!("  Admin: {}", config.admin);
                println!("  Oracle Authority: {}", config.oracle_authority);
                println!(
                    "  Priority Fee Oracle Authority: {}",
                    config.priority_fee_oracle_authority
                );
                println!("  Counter: {}", config.counter);
                println!("  Bump: {}", config.bump);
            }
            Err(err) => {
                eprintln!("Error deserializing config: {err}");
            }
        },
        Err(err) => {
            eprintln!("Error fetching config account: {err}");
        }
    }
}

async fn command_dune_priority_fee_backfill(args: DunePriorityFeeBackfill, client: RpcClient) {
    let epoch_schedule = client
        .get_epoch_schedule()
        .expect("Could not get epoch schedule");

    // Move the blocking operations into spawn_blocking
    let entries_written = tokio::task::spawn_blocking(move || {
        let mut connection = Connection::open(args.sqlite_path).expect("Failed to open database");

        DBSlotInfo::fetch_and_insert_from_dune(
            &mut connection,
            &args.dune_api_key,
            &args.query_id,
            &epoch_schedule,
            args.chunk_size,
            args.batch_size,
            args.starting_offset,
        )
    })
    .await
    .expect("Task panicked")
    .expect("Error running backfill");

    println!("Total entries written: {}", entries_written);
}

fn command_upload_validator_age(args: UploadValidatorAge, client: RpcClient) {
    // Upload validator age for a specific vote account
    let keypair = read_keypair_file(args.keypair_path).expect("Failed reading keypair file");

    // Get current epoch if not specified
    let epoch = args.epoch.unwrap_or_else(|| {
        let epoch_info = client.get_epoch_info().expect("Failed to get epoch info");
        epoch_info.epoch as u16
    });

    // Get validator history account address
    let (validator_history_pda, _) = Pubkey::find_program_address(
        &[ValidatorHistory::SEED, args.vote_account.as_ref()],
        &validator_history::ID,
    );

    // Get config account address
    let (config_pda, _) = Pubkey::find_program_address(&[Config::SEED], &validator_history::ID);

    let instruction = Instruction {
        program_id: validator_history::ID,
        accounts: validator_history::accounts::UploadValidatorAge {
            validator_history_account: validator_history_pda,
            vote_account: args.vote_account,
            config: config_pda,
            oracle_authority: keypair.pubkey(),
        }
        .to_account_metas(None),
        data: validator_history::instruction::UploadValidatorAge {
            validator_age: args.age,
            validator_age_last_updated_epoch: epoch,
        }
        .data(),
    };

    let blockhash = client
        .get_latest_blockhash()
        .expect("Failed to get recent blockhash");
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&keypair.pubkey()),
        &[&keypair],
        blockhash,
    );

    let signature = client
        .send_and_confirm_transaction_with_spinner(&transaction)
        .expect("Failed to send transaction");

    println!("Successfully uploaded validator age:");
    println!("  Vote Account: {}", args.vote_account);
    println!("  Validator Age: {}", args.age);
    println!("  Last Updated Epoch: {}", epoch);
    println!("  Signature: {}", signature);
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    env_logger::init();
    let args = Args::parse();
    let commitment_config = args.commitment.into();
    let client = RpcClient::new_with_timeout_and_commitment(
        args.json_rpc_url.clone(),
        Duration::from_secs(60),
        commitment_config,
    );
    match args.commands {
        Commands::InitConfig(args) => command_init_config(args, client),
        Commands::ReallocConfig(args) => command_realloc_config(args, client),
        Commands::CrankerStatus(args) => command_cranker_status(args, client),
        Commands::InitClusterHistory(args) => command_init_cluster_history(args, client),
        Commands::ClusterHistoryStatus(args) => command_cluster_history(args, client),
        Commands::ViewConfig => command_view_config(client),
        Commands::History(args) => command_history(args, client),
        Commands::BackfillClusterHistory(args) => command_backfill_cluster_history(args, client),
        Commands::UpdateOracleAuthority(args) => command_update_oracle_authority(args, client),
        Commands::StakeByCountry(args) => command_stake_by_country(args, client).await,
        Commands::GetConfig => command_get_config(client),
        Commands::DunePriorityFeeBackfill(args) => {
            command_dune_priority_fee_backfill(args, client).await
        }
        Commands::UploadValidatorAge(args) => command_upload_validator_age(args, client),
        Commands::BackfillValidatorAge(command_args) => {
            commands::backfill_validator_age::run(command_args, args.json_rpc_url).await
        }
        Commands::CrankCopyClusterInfo(command_args) => {
            commands::cranks::copy_cluster_info::run(command_args, args.json_rpc_url).await?
        }
        Commands::CrankCopyGossipContactInfo(command_args) => {
            let client = solana_client::nonblocking::rpc_client::RpcClient::new_with_timeout(
                args.json_rpc_url.clone(),
                Duration::from_secs(60),
            );
            let client = Arc::new(client);
            commands::cranks::copy_gossip_contact_info::run(command_args, client).await?
        }
        Commands::CrankCopyTipDistributionAccount(command_args) => {
            commands::cranks::copy_tip_distribution_account::run(command_args, args.json_rpc_url)
                .await?
        }
        Commands::CrankCopyVoteAccount(command_args) => {
            commands::cranks::copy_vote_account::run(command_args, args.json_rpc_url).await?
        }
    };

    Ok(())
}
