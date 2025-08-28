use crate::commands::command_args::ComputeDirectedStakeMeta;
use anchor_lang::InstructionData;
use anchor_lang::ToAccountMetas;
use anchor_lang::{AnchorDeserialize, AnchorSerialize, Discriminator};
use anyhow::Result;
use borsh::BorshDeserialize;
use jito_steward::state::directed_stake::DirectedStakeMeta;
use jito_steward::state::directed_stake::DirectedStakePreference;
use jito_steward::utils::U8Bool;
use jito_steward::DirectedStakeTicket;
use solana_account_decoder::UiAccountEncoding;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_filter::{Memcmp, MemcmpEncodedBytes, RpcFilterType};
use solana_program::instruction::Instruction;
use solana_sdk::account::Account;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::read_keypair_file;
use solana_sdk::signature::Signature;
use solana_sdk::signer::Signer;
use solana_sdk::transaction::Transaction;
use spl_stake_pool::state::StakePool;
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;

pub async fn build_copy_target_transaction(
    client: &Arc<RpcClient>,
    program_id: Pubkey,
    vote_pubkey: Pubkey,
    target_lamports: u64,
    authority_keypair_path: &Path,
) -> Result<Transaction> {
    let authority_keypair = read_keypair_file(authority_keypair_path)
        .map_err(|e| anyhow::anyhow!("Failed to read keypair: {}", e))?;
    let authority_pubkey = authority_keypair.pubkey();

    let (directed_stake_meta_pda, _bump) =
        Pubkey::find_program_address(&[DirectedStakeMeta::SEED], &program_id);

    let instruction = Instruction {
        program_id,
        accounts: jito_steward::accounts::CopyDirectedStakeTargets {
            config: program_id,
            directed_stake_meta: directed_stake_meta_pda,
            authority: authority_pubkey,
        }
        .to_account_metas(None),
        data: jito_steward::instruction::CopyDirectedStakeTargets {
            vote_pubkey,
            total_target_lamports: target_lamports,
        }
        .data(),
    };
    let blockhash = client.get_latest_blockhash().await?;
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&authority_pubkey),
        &[&authority_keypair],
        blockhash,
    );
    Ok(transaction)
}

pub async fn send_copy_target_transaction(
    client: &Arc<RpcClient>,
    transaction: Transaction,
) -> Result<Signature> {
    std::thread::sleep(std::time::Duration::from_secs(2));
    //Ok(Signature::default())
    let signature = client.send_and_confirm_transaction(&transaction).await?;
    Ok(signature)
}

pub async fn command_compute_directed_stake_meta(
    args: ComputeDirectedStakeMeta,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    let discriminator = DirectedStakeTicket::DISCRIMINATOR;
    let mut validator_target_delegations: HashMap<Pubkey, u64> = HashMap::new();
    let memcmp_filter = RpcFilterType::Memcmp(Memcmp::new(
        0,
        MemcmpEncodedBytes::Base58(solana_sdk::bs58::encode(discriminator).into_string()),
    ));

    let _accounts = client
        .get_program_accounts_with_config(
            &program_id,
            solana_client::rpc_config::RpcProgramAccountsConfig {
                filters: Some(vec![memcmp_filter]),
                account_config: solana_client::rpc_config::RpcAccountInfoConfig {
                    encoding: Some(UiAccountEncoding::Base64),
                    commitment: Some(CommitmentConfig::confirmed()),
                    data_slice: None,
                    min_context_slot: None,
                },
                with_context: Some(true),
                sort_results: None,
            },
        )
        .await?;

    // TODO: If it exists, we need the previous epoch's directed stake meta to get the 
    // total_stake_lamports for each validator

    // Mock data
    let whale_pubkey = Pubkey::from_str("HUSZemFZ1xsELqoSzC6t2a3u956ZZcBgdr7HLijEMtHM").unwrap();
    let kamino_reserve_pubkey =
        Pubkey::from_str("6sga1yRArgQRqa8Darhm54EBromEpV3z8iDAvMTVYXB3").unwrap();
    let drift_vault = Pubkey::from_str("2AG6YN9Wi7JDrFcLNhaEP2NrXyZKFj7EjMPdkvwPdRR1").unwrap();
    let forward_industries_validator_pubkey =
        Pubkey::from_str("3JD3jMmnR6g88qff2WZ3cMHJRjJMUk9yVZtmYTYeFrXf").unwrap();
    let galaxy_validator_pubkey =
        Pubkey::from_str("CvSb7wdQAFpHuSpTYTJnX5SYH4hCfQ9VuGnqrKaKwycB").unwrap();
    let security_council_pubkey =
        Pubkey::from_str("9eZbWiHsPRsxLSiHxzg2pkXsAuQMwAjQrda7C7e21Fw6").unwrap();
    let validator_pubkey =
        Pubkey::from_str("A4hyMd3FyvUJSRafDUSwtLLaQcxRP4r1BRC9w2AJ1to2").unwrap();
    let drift_validator_pubkey =
        Pubkey::from_str("DriFTm3wM9ugxhCA1K3wVQMSdC4Dv4LNmyZMmZiuHRpp").unwrap();
    let directed_stake_ticket = DirectedStakeTicket::new(
        kamino_reserve_pubkey,
        security_council_pubkey,
        U8Bool::from(true),
        &[DirectedStakePreference::new(validator_pubkey, 10000)],
    );
    let drift_directed_stake_ticket = DirectedStakeTicket::new(
        drift_vault,
        drift_validator_pubkey,
        U8Bool::from(true),
        &[DirectedStakePreference::new(drift_validator_pubkey, 10000)],
    );

    let whale_directed_stake_ticket = DirectedStakeTicket::new(
        whale_pubkey,
        whale_pubkey,
        U8Bool::from(true),
        &[
            DirectedStakePreference::new(galaxy_validator_pubkey, 5000),
            DirectedStakePreference::new(forward_industries_validator_pubkey, 5000),
        ],
    );

    let data = directed_stake_ticket.try_to_vec().unwrap();
    let drift_ticket_data = drift_directed_stake_ticket.try_to_vec().unwrap();
    let whale_ticket_data = whale_directed_stake_ticket.try_to_vec().unwrap();
    let accounts = vec![
        (
            Pubkey::new_from_array([1; 32]),
            Account {
                lamports: 1000000000,
                data: data.clone(),
                owner: program_id,
                ..Account::default()
            },
        ),
        (
            Pubkey::new_from_array([2; 32]),
            Account {
                lamports: 1000000000,
                data: drift_ticket_data,
                owner: program_id,
                ..Account::default()
            },
        ),
        (
            Pubkey::new_from_array([3; 32]),
            Account {
                lamports: 1000000000,
                data: whale_ticket_data,
                owner: program_id,
                ..Account::default()
            },
        ),
    ];

    println!("DirectedStakeTickets:");

    for (pubkey, account) in &accounts {
        let ticket = DirectedStakeTicket::try_from_slice(&mut account.data.as_slice()).unwrap();
        let (jitosol_balance, _jitosol_ui_amount) = match client
            .get_token_account_balance(&ticket.ticket_update_authority)
            .await
        {
            Ok(balance) => (balance.amount.clone(), balance.ui_amount.unwrap_or(0.0)),
            Err(_) => ("0".to_string(), 0.0),
        };

        println!("\tTicket: {}", pubkey);
        println!("\t\tUpdate Authority: {}", &ticket.ticket_update_authority);
        println!(
            "\t\tUpdate Authority Balance (JitoSOL lamports): {}",
            jitosol_balance
        );

        let address = Pubkey::from_str("Jito4APyf642JPZPx3hGc6WWJ8zPKtRbRs4P815Awbb").unwrap();
        let stake_pool_account = client.get_account(&address).await?;
        let stake_pool = StakePool::deserialize(&mut stake_pool_account.data.as_slice()).unwrap();

        let total_lamports: u64 = stake_pool.total_lamports;
        let pool_token_supply: u64 = stake_pool.pool_token_supply;
        let conversion_rate_bps: u64 = (total_lamports as u128)
            .checked_mul(10_000)
            .unwrap()
            .checked_div(pool_token_supply as u128)
            .unwrap() as u64;

        for preference in ticket.staker_preferences {
            if preference.vote_pubkey != Pubkey::default() {
                let total_lamports: u64 = jitosol_balance.parse::<u64>().unwrap();
                let allocation_jito_sol = preference.get_allocation(total_lamports);
                println!("\t\tPreferred Vote Account: {}", preference.vote_pubkey);
                println!("\t\t\tStake Share bps: {}", preference.stake_share_bps);
                println!(
                    "\t\t\tAllocation (JitoSOL lamports): {}",
                    allocation_jito_sol
                );
                let allocation_lamports = allocation_jito_sol
                    .checked_mul(conversion_rate_bps as u128)
                    .unwrap()
                    .checked_div(10_000)
                    .unwrap() as u64;
                println!("\t\t\tAllocation (SOL lamports): {}", allocation_lamports);
                let current_allocation = validator_target_delegations
                    .get(&preference.vote_pubkey)
                    .unwrap_or(&0);
                validator_target_delegations.insert(
                    preference.vote_pubkey,
                    current_allocation.saturating_add(allocation_lamports),
                );
            }
        }
    }
    println!("Directed Stake Meta:");
    validator_target_delegations
        .iter()
        .for_each(|(vote_pubkey, lamports)| {
            println!("\tVote Pubkey: {}", vote_pubkey);
            println!("\t\tTarget Delegation (SOL lamports): {}", lamports);
        });

    if args.copy_targets {
        let mut pending_keys: Vec<Pubkey> = validator_target_delegations.keys().cloned().collect();
        let mut keys_to_delete: Vec<Pubkey> = Vec::new();
        let empty_progress_bar = " ".repeat(60);
        let mut progress_bar = empty_progress_bar.to_string();
        let tx_progress_weight = 60 / validator_target_delegations.len();
        let mut progress = 0;
        let start_time = std::time::Instant::now();
        let mut total_duration = 0;
        let mut error_log: String = String::new();
        let mut success_log: String = String::new();
        println!("\nSending transactions...");
        if args.progress_bar {
            print!(
                "\r[{}] {}s elapsed ({}/{}) sent",
                progress_bar,
                0,
                validator_target_delegations.len() - pending_keys.len(),
                validator_target_delegations.len()
            );
            std::io::stdout().flush().unwrap();
        }
        for i in 0..pending_keys.len() {
            let tx = build_copy_target_transaction(
                client,
                program_id,
                pending_keys[i],
                validator_target_delegations[&pending_keys[i]],
                &args.authority_keypair_path,
            )
            .await?;
            let maybe_signature = send_copy_target_transaction(client, tx.clone()).await;
            total_duration = start_time.elapsed().as_secs();
            progress = if (progress + tx_progress_weight) > 60 {
                60
            } else {
                progress + tx_progress_weight
            };
            if args.progress_bar {
                progress_bar = "\x1b[32m".to_string()
                    + &"━".repeat(progress)
                    + &"\x1b[0m"
                    + &" ".repeat(60_usize.saturating_sub(progress));
                print!(
                    "\r[{}] {}s elapsed ({}/{}) sent",
                    progress_bar,
                    total_duration,
                    i + 1,
                    validator_target_delegations.len()
                );
            }
            match maybe_signature {
                Ok(signature) => {
                    keys_to_delete.push(pending_keys[i]);
                    success_log.push_str(&format!(" {} succeeded\n", signature));
                }
                Err(e) => {
                    let tx_hash = &tx.signatures[0];
                    error_log.push_str(&format!("\n{} failed\n", tx_hash));
                    error_log.push_str(&e.to_string());
                    if progress == 60 {
                        println!("\n{}", error_log);
                    }
                    std::io::stdout().flush().unwrap();
                    continue;
                }
            }
            std::io::stdout().flush().unwrap();
        }
        if progress == 60 && !success_log.is_empty() {
            println!("\n{}", success_log);
        }
        if progress == 60 && !error_log.is_empty() {
            println!("\n{}", error_log);
        }
        keys_to_delete.iter().for_each(|key| {
            pending_keys.retain(|k| k != key);
        });
        println!(
            "\n({}/{}) copy stake target transactions succeeded",
            validator_target_delegations.len() - pending_keys.len(),
            validator_target_delegations.len()
        );
    }
    Ok(())
}
