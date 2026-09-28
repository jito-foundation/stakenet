use std::path::PathBuf;

use anchor_lang::{AccountDeserialize, InstructionData, ToAccountMetas};
use anyhow::anyhow;
use clap::Parser;
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    instruction::Instruction, pubkey::Pubkey, signature::read_keypair_file, signer::Signer,
    transaction::Transaction,
};
use validator_history::state::Config;

#[derive(Parser)]
#[command(about = "Set new priority fee distribution program on the config account")]
pub struct SetNewPriorityFeeDistributionProgram {
    /// Path to the admin keypair, used to sign and pay for the transaction
    #[arg(short, long, env, default_value = "~/.config/solana/id.json")]
    keypair_path: PathBuf,

    /// New priority fee distribution program ID (Pubkey as base58 string)
    #[arg(long, env)]
    new_priority_fee_distribution_program: Pubkey,

    /// Print the change that would be made without sending a transaction
    #[arg(long, env, default_value = "false")]
    dry_run: bool,
}

pub fn run(args: SetNewPriorityFeeDistributionProgram, client: RpcClient) -> anyhow::Result<()> {
    let keypair = read_keypair_file(args.keypair_path)
        .map_err(|e| anyhow!("Failed reading keypair file: {e}"))?;

    let program_id = validator_history::ID;
    let (config_pda, _) = Pubkey::find_program_address(&[Config::SEED], &program_id);

    // `SetNewPriorityFeeDistributionProgram` has a `has_one = admin` constraint, so a mismatched
    // signer fails on-chain. Check it up front so the error is actionable.
    let config_account = client
        .get_account(&config_pda)
        .map_err(|e| anyhow!("Failed fetching config account {config_pda}: {e}"))?;
    let config = Config::try_deserialize(&mut config_account.data.as_slice())
        .map_err(|e| anyhow!("Failed deserializing config account {config_pda}: {e}"))?;
    if config.admin != keypair.pubkey() {
        return Err(anyhow!(
            "Signer {} is not the config admin (expected {})",
            keypair.pubkey(),
            config.admin
        ));
    }

    println!("Config:  {config_pda}");
    println!("Admin:   {}", config.admin);
    println!("Current: {}", config.priority_fee_distribution_program);
    println!("New:     {}", args.new_priority_fee_distribution_program);

    if config.priority_fee_distribution_program == args.new_priority_fee_distribution_program {
        println!("Priority fee distribution program already set to this value; nothing to do");
        return Ok(());
    }

    // The instruction does not deserialize the target, so a non-program account would silently
    // break `copy_priority_fee_distribution` PDA derivation later.
    match client.get_account(&args.new_priority_fee_distribution_program) {
        Ok(account) if account.executable => {}
        Ok(_) => {
            return Err(anyhow!(
                "Account {} exists but is not executable; expected a program",
                args.new_priority_fee_distribution_program
            ));
        }
        Err(e) => {
            return Err(anyhow!(
                "Failed fetching {}: {e}",
                args.new_priority_fee_distribution_program
            ));
        }
    }

    if args.dry_run {
        println!("Dry run: no transaction submitted");
        return Ok(());
    }

    let instruction = Instruction {
        program_id,
        accounts: validator_history::accounts::SetNewPriorityFeeDistributionProgram {
            config: config_pda,
            new_priority_fee_distribution_program: args.new_priority_fee_distribution_program,
            admin: keypair.pubkey(),
        }
        .to_account_metas(None),
        data: validator_history::instruction::SetNewPriorityFeeDistributionProgram {}.data(),
    };

    let blockhash = client
        .get_latest_blockhash()
        .map_err(|e| anyhow!("Failed to get recent blockhash: {e}"))?;
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&keypair.pubkey()),
        &[&keypair],
        blockhash,
    );

    let signature = client
        .send_and_confirm_transaction_with_spinner(&transaction)
        .map_err(|e| anyhow!("Failed to send transaction: {e}"))?;
    println!("Signature: {signature}");

    Ok(())
}
