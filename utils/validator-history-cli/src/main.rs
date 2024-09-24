use std::{path::PathBuf, str::FromStr, thread::sleep, time::Duration};

use anchor_lang::{AccountDeserialize, Discriminator, InstructionData, ToAccountMetas};
use clap::{arg, command, Parser, Subcommand};
use solana_client::{
    rpc_client::RpcClient,
    rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig},
    rpc_filter::{Memcmp, RpcFilterType},
};
use solana_program::instruction::Instruction;
use solana_sdk::{
    pubkey::Pubkey, signature::read_keypair_file, signer::Signer, transaction::Transaction,
};
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
    GetIndex(GetIndex),
}

#[derive(Parser)]
#[command(about = "Initialize config account")]
struct InitConfig {
    /// Path to keypair used to pay for account creation and execute transactions
    #[arg(short, long, env, default_value = "~/.config/solana/id.json")]
    keypair_path: PathBuf,

    /// Tip distribution program ID (Pubkey as base58 string)
    #[arg(short, long, env)]
    tip_distribution_program_id: Pubkey,

    /// New tip distribution authority (Pubkey as base58 string)
    ///
    /// If not provided, the initial keypair will be the authority
    #[arg(short, long, env, required(false))]
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
#[command(about = "Get validator history index")]
struct GetIndex {
    /// Validator to get index for
    validator: Pubkey,
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

    let target_validators = vec![
        Pubkey::from_str("HTtwGKgQgsQCAFDPBgN7LabHTMEkUpmTnFqEo5cBcquR").unwrap(),
        Pubkey::from_str("We11J5D4iXcNbdMwCZX2o9RRkwaWBo1AGLADfubmeTb").unwrap(),
        Pubkey::from_str("F5b1wSUtpaYDnpjLQonCZC7iyFvizLcNqTactZbwSEXK").unwrap(),
        Pubkey::from_str("C6uqzABsRPmFd14iL9Ej36AbddVxXPJWV6jwbLZYdYJM").unwrap(),
        Pubkey::from_str("4PTVcstjs4s89uETGapnjTNPDa4XfHF9kLUu8WCeYNQb").unwrap(),
        Pubkey::from_str("GSXjVH8Hgfg1kJQbi4KD8P1Zme8ir3E4S3dF7cWL4UKs").unwrap(),
        Pubkey::from_str("4PNoTwgsAaxeq8G1MKhWn7WGsm4KTtfm3Vae3quvcDEs").unwrap(),
        Pubkey::from_str("8guXF5HQVU4g71ZCnn6aEJxQyb59NaEc4XCGjF5arsiH").unwrap(),
        Pubkey::from_str("5ni6KoVM62cRJNfFFKGdiyDfYbKWWAGZ21cfGZcj1y66").unwrap(),
        Pubkey::from_str("Bg65CcoUn4X6k2nxbrEAZ9ZT74hsBdD4Fw8ZfSr8gKjH").unwrap(),
        Pubkey::from_str("KXMGa7fRW78c6VVND6YwPbR9sEu8V3q9QL3ctLDioAT").unwrap(),
        Pubkey::from_str("2cRtnW1f6b8GSTWENTr2pQ1Bobz9QmCdq1oVyCmSmV1n").unwrap(),
        Pubkey::from_str("C6DEs3i448uhrsWMMnWYq7WsxkujcgADrCJQ4AMJ8ipj").unwrap(),
        Pubkey::from_str("GREENr9zSeapgunqdMeTg8MCh2cDDn2y3py1mBGUzJYe").unwrap(),
        Pubkey::from_str("5WPxGiB6zBXNJp8JN3WhSKDuTY3ZBX6dBDcbtVMQAJLX").unwrap(),
        Pubkey::from_str("A3MC4K2pxLXTEHVN5HFF9ikjiauGP7ioZws9FYsucAWF").unwrap(),
        Pubkey::from_str("2wTzvCfVGJuGUTvRD5qMtmxAaVJE43suTN31enfW44yb").unwrap(),
        Pubkey::from_str("7QahdHiCFvYpFc1hMSY5E3GKw4SvKhENBcwGJfuqf7mA").unwrap(),
        Pubkey::from_str("A9bYZWk2Sb3PYfE1itJb6q4D5xPbtaY7rAvgzLAnwumr").unwrap(),
        Pubkey::from_str("GZCv21mPm7HNwiC5Hq3j1DXz4njDQownDAfN7xziXbjN").unwrap(),
        Pubkey::from_str("FMKT6kHBkmPf3LjZV5tpo3oR4m32rF8nF8JtC3WWcWoN").unwrap(),
        Pubkey::from_str("HQuUQmerqwvBRFo1moWgNPpcc43EJxGUQZmgrrmqA9sA").unwrap(),
        Pubkey::from_str("CzmqDuqEpfnkptuLAcikmJrhCnhFXo8aUBj6Rto1SPAc").unwrap(),
        Pubkey::from_str("CT2CzbiNRz8ccgWQZR4BN7cpm3rDyWyQxUf5MNbMom7n").unwrap(),
        Pubkey::from_str("F2bcoVhE2he5DCruN4PKkWAriNXs2VWty94n9CCdWZ8s").unwrap(),
        Pubkey::from_str("8vT4MgBeZQmYN44DXwHd2BLM7Z6io186nJk3FQEwy58f").unwrap(),
        Pubkey::from_str("5FaFPTcpDpgzrzf7NPP3YzXBPMCCwUjvKUWzvGHA3tqW").unwrap(),
        Pubkey::from_str("7tr5vUb3j36k4p9bs2tr1GwRFoSsEWd4LEut8e8HJZEv").unwrap(),
        Pubkey::from_str("Y3JVETZQq3Be8zwKAassXVHnz23xt4V7MRFEsNPYyh8").unwrap(),
        Pubkey::from_str("EkPjdWCtnzinzkNpV1pCjaw4py5tDj8ihATUG3tkLWNN").unwrap(),
        Pubkey::from_str("Eajfs6oXGGkvjYsxkQZZJcDCLLkUajaHizfgg2xTsqyd").unwrap(),
        Pubkey::from_str("GB44NXtM7zGm6QnzQjzHZcRKSswkJbox8aJsKiXGbFJr").unwrap(),
        Pubkey::from_str("97QujuhmyxD19CpgdyNAcJXvKi6oDeyRPdNep6uXBbA4").unwrap(),
        Pubkey::from_str("AE6xdVD5e92ZgevjWAamoF5kuAu88AvmUv4RRdBcoyz3").unwrap(),
        Pubkey::from_str("P4f3F3VfMhKvpGQXg2MuvLfWmZui41gvcH9XKtYDiFX").unwrap(),
        Pubkey::from_str("DM8eVQwKYpFUq4MAC1XEeZMjV4T34LfvGkK9vca55GaY").unwrap(),
        Pubkey::from_str("462gkiydfX1ks7bzS71j4k5uLZaonTzcz5mfJpgLGe9k").unwrap(),
        Pubkey::from_str("2PEyBgsPYBQ8pMdXQtEaPGNqWQHE9GCnmV2tTVN4GMru").unwrap(),
        Pubkey::from_str("ASrBTnxfvt8uY164wRrM9xXP925yoHzoRBnVYU5s7DEs").unwrap(),
        Pubkey::from_str("EXrWdDxFaE3Sfsbh3TV5ToGhMqu53xrmeoVdvn467jUH").unwrap(),
        Pubkey::from_str("8ygeLBNceokp2HfjUdg3pzii8MaqmHpAuuh3S5yvJVph").unwrap(),
        Pubkey::from_str("GYMMgjX69RYVxN9wYNgE8YnjoZwVyencKFhWnd5hWGx6").unwrap(),
        Pubkey::from_str("6SQJfKXxqacQpYmCNtqvabV6kfHrPGqXm9q2iH7zsied").unwrap(),
        Pubkey::from_str("5eafxmzReWy8vWRmBcor4oShtQgDwHR3CGGPrfcxdU75").unwrap(),
        Pubkey::from_str("BJ3wS8o4eowhNTbWHZiE3vgin4da1aWiGwcAmpvtc8aN").unwrap(),
        Pubkey::from_str("BfbkjGQJjfA8DDD6Pkkdc9c2okMQ2pzrg7qpbiYs7TGC").unwrap(),
        Pubkey::from_str("AZu19pqPb66L5stguwCDEaGoKHBXDr2xzyPyoWxmx3MU").unwrap(),
        Pubkey::from_str("NomaDtKz3VQi85XssD6jpprLF5FPWghbAB1SSdWV1VN").unwrap(),
        Pubkey::from_str("HsUhGC6yKwr8kLC5gdqVLrDfKsGCd2ZAXiabP1WojuzA").unwrap(),
        Pubkey::from_str("9jVU1ET9Xxqnrsqjw8FGxmHgcXs6Hbj1HECckWqn2LUD").unwrap(),
        Pubkey::from_str("13dy8pb1z2wqnHFGxN8Mv4kbfA1TbSEhTBjHfuJ51X41").unwrap(),
    ];

    for vote_key in target_validators {
        let validator_history = validator_histories
            .iter()
            .find(|vh| vh.vote_account == vote_key)
            .unwrap();
        let validator_history_pda = Pubkey::find_program_address(
            &[ValidatorHistory::SEED, vote_key.as_ref()],
            &validator_history::ID,
        )
        .0;

        println!(
            "{},{},https://solscan.io/account/{}#data",
            validator_history_pda, validator_history.index, validator_history_pda
        );
    }
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

fn command_get_index(args: GetIndex, client: RpcClient) {
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

    println!("{},{}", validator_history_pda, validator_history.index);
}

fn main() {
    let args = Args::parse();
    let client = RpcClient::new_with_timeout(args.json_rpc_url.clone(), Duration::from_secs(60));
    match args.commands {
        Commands::InitConfig(args) => command_init_config(args, client),
        Commands::CrankerStatus(args) => command_cranker_status(args, client),
        Commands::InitClusterHistory(args) => command_init_cluster_history(args, client),
        Commands::ClusterHistoryStatus => command_cluster_history(client),
        Commands::History(args) => command_history(args, client),
        Commands::GetIndex(args) => command_get_index(args, client),
        Commands::BackfillClusterHistory(args) => command_backfill_cluster_history(args, client),
    };
}
