use std::path::PathBuf;

use anchor_lang::{InstructionData, ToAccountMetas};
use clap::Parser;
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    instruction::Instruction, pubkey::Pubkey, signature::read_keypair_file, signer::Signer,
    transaction::Transaction,
};
use validator_history::Config;

#[derive(Parser)]
#[command(about = "Set new tip distribution program on the config account")]
pub struct SetNewTipDistributionProgram {
    /// Path to keypair used to pay for account creation and execute transactions
    #[arg(short, long, env, default_value = "~/.config/solana/id.json")]
    keypair_path: PathBuf,

    /// New tip distribution program ID (Pubkey as base58 string)
    #[arg(long, env)]
    tip_distribution_program_id: Pubkey,
}

pub fn run(args: SetNewTipDistributionProgram, client: RpcClient) {
    let keypair = read_keypair_file(args.keypair_path).expect("Failed reading keypair file");

    let (config_pda, _) = Pubkey::find_program_address(&[Config::SEED], &validator_history::ID);
    let instruction = Instruction {
        program_id: validator_history::ID,
        accounts: validator_history::accounts::SetNewTipDistributionProgram {
            config: config_pda,
            new_tip_distribution_program: args.tip_distribution_program_id,
            admin: keypair.pubkey(),
        }
        .to_account_metas(None),
        data: validator_history::instruction::SetNewTipDistributionProgram {}.data(),
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
    println!("Signature: {signature}");
}
