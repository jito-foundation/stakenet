use std::path::PathBuf;

use anyhow::anyhow;
use clap::Parser;
use solana_client::{rpc_client::RpcClient, rpc_config::RpcSimulateTransactionConfig};
use solana_native_token::lamports_to_sol;
use solana_sdk::{
    pubkey::Pubkey, signature::read_keypair_file, signer::Signer, transaction::Transaction,
};
use stakenet_sdk::utils::{
    accounts::get_validator_history_address,
    instructions::get_create_validator_history_instructions,
};
use validator_history::ValidatorHistory;

#[derive(Parser)]
#[command(about = "Initialize validator history account for a vote account")]
pub struct InitValidatorHistory {
    /// Path to keypair used to pay for account creation and execute transactions
    #[arg(short, long, env, default_value = "~/.config/solana/id.json")]
    keypair_path: PathBuf,

    /// Vote account to create the validator history account for
    #[arg(long, env)]
    vote_account: Pubkey,
}

pub fn run(
    args: InitValidatorHistory,
    client: RpcClient,
    program_id: Pubkey,
) -> anyhow::Result<()> {
    let keypair = read_keypair_file(args.keypair_path)
        .map_err(|e| anyhow!("Failed reading keypair file: {e}"))?;

    let vote_account = client
        .get_account(&args.vote_account)
        .map_err(|e| anyhow!("Failed fetching vote account {}: {e}", args.vote_account))?;
    if vote_account.owner != solana_sdk::vote::program::ID {
        return Err(anyhow!(
            "{} is not a vote account (owner: {})",
            args.vote_account,
            vote_account.owner
        ));
    }

    let validator_history_address = get_validator_history_address(&args.vote_account, &program_id);
    println!("Vote account:      {}", args.vote_account);
    println!("Validator history: {validator_history_address}");

    if client
        .get_account_with_commitment(&validator_history_address, client.commitment())?
        .value
        .is_some()
    {
        println!("Validator history account already exists; nothing to do");
        return Ok(());
    }

    let rent = client.get_minimum_balance_for_rent_exemption(ValidatorHistory::SIZE)?;
    println!("Payer:             {}", keypair.pubkey());
    println!("Rent:              {} SOL", lamports_to_sol(rent));

    let instructions =
        get_create_validator_history_instructions(&args.vote_account, &program_id, &keypair);
    let mut transaction = Transaction::new_with_payer(&instructions, Some(&keypair.pubkey()));

    let simulation = client
        .simulate_transaction_with_config(
            &transaction,
            RpcSimulateTransactionConfig {
                sig_verify: false,
                replace_recent_blockhash: true,
                commitment: Some(client.commitment()),
                ..RpcSimulateTransactionConfig::default()
            },
        )?
        .value;
    if let Some(err) = simulation.err {
        for log in simulation.logs.unwrap_or_default() {
            eprintln!("  {log}");
        }
        return Err(anyhow!("Simulation failed: {err}"));
    }

    let blockhash = client
        .get_latest_blockhash()
        .map_err(|e| anyhow!("Failed to get recent blockhash: {e}"))?;
    transaction.sign(&[&keypair], blockhash);

    let signature = client
        .send_and_confirm_transaction_with_spinner(&transaction)
        .map_err(|e| anyhow!("Failed to send transaction: {e}"))?;
    println!("Signature: {signature}");

    Ok(())
}
