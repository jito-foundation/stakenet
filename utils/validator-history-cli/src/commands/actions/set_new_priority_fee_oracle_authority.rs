use std::path::PathBuf;

use anchor_lang::{AccountDeserialize, InstructionData, ToAccountMetas};
use anyhow::anyhow;
use clap::Parser;
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    instruction::Instruction, pubkey::Pubkey, signature::read_keypair_file, signer::Signer,
    transaction::Transaction,
};
use validator_history::{state::Config, DNE_AUTHORITY};

#[derive(Parser)]
#[command(about = "Set new priority fee oracle authority on the config account")]
pub struct SetNewPriorityFeeOracleAuthority {
    /// Path to the admin keypair, used to sign and pay for the transaction
    #[arg(short, long, env, default_value = "~/.config/solana/id.json")]
    keypair_path: PathBuf,

    /// New priority fee oracle authority (Pubkey as base58 string)
    ///
    /// This is the only key permitted to call `update_priority_fee_history`, which writes
    /// `total_priority_fees`, `total_leader_slots`, and `blocks_produced`.
    #[arg(long, env)]
    new_priority_fee_oracle_authority: Pubkey,

    /// Print the change that would be made without sending a transaction
    #[arg(long, env, default_value = "false")]
    dry_run: bool,
}

pub fn run(
    args: SetNewPriorityFeeOracleAuthority,
    client: RpcClient,
    program_id: Pubkey,
) -> anyhow::Result<()> {
    let keypair = read_keypair_file(args.keypair_path)
        .map_err(|e| anyhow!("Failed reading keypair file: {e}"))?;

    let (config_pda, _) = Pubkey::find_program_address(&[Config::SEED], &program_id);

    // `SetNewPriorityFeeOracleAuthority` has a `has_one = admin` constraint, so a mismatched
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
    println!("Current: {}", config.priority_fee_oracle_authority);
    println!("New:     {}", args.new_priority_fee_oracle_authority);

    if config.priority_fee_oracle_authority == args.new_priority_fee_oracle_authority {
        println!("Priority fee oracle authority already set to this value; nothing to do");
        return Ok(());
    }

    // The instruction accepts any pubkey, including the default/system address, which would
    // make `update_priority_fee_history` permanently unsignable.
    if args.new_priority_fee_oracle_authority == DNE_AUTHORITY {
        return Err(anyhow!(
            "Refusing to set the priority fee oracle authority to {DNE_AUTHORITY}, which would \
             make update_priority_fee_history impossible to sign for"
        ));
    }

    if args.dry_run {
        println!("Dry run: no transaction submitted");
        return Ok(());
    }

    let instruction = Instruction {
        program_id,
        accounts: validator_history::accounts::SetNewPriorityFeeOracleAuthority {
            config: config_pda,
            new_priority_fee_oracle_authority: args.new_priority_fee_oracle_authority,
            admin: keypair.pubkey(),
        }
        .to_account_metas(None),
        data: validator_history::instruction::SetNewPriorityFeeOracleAuthority {}.data(),
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
