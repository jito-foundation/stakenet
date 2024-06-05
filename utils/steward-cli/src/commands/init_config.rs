use anchor_lang::{InstructionData, ToAccountMetas};
use jito_steward::{Staker, UpdateParametersArgs};
use solana_client::rpc_client::RpcClient;
use solana_program::instruction::Instruction;
use solana_sdk::{
    pubkey::Pubkey,
    signature::{read_keypair_file, Keypair},
    signer::Signer,
    transaction::Transaction,
};

use super::commands::InitConfig;

pub fn command_init_config(args: InitConfig, client: RpcClient, program_id: Pubkey) {
    // Creates config account
    let authority = read_keypair_file(args.authority_keypair_path)
        .expect("Failed reading keypair file ( Authority )");

    let staker_keypair = {
        if let Some(staker_keypair_path) = args.staker_keypair_path {
            read_keypair_file(staker_keypair_path).expect("Failed reading keypair file ( Staker )")
        } else {
            authority.insecure_clone()
        }
    };

    let steward_config = {
        if let Some(steward_config_keypair_path) = args.steward_config_keypair_path {
            read_keypair_file(steward_config_keypair_path)
                .expect("Failed reading keypair file ( Steward Config )")
        } else {
            Keypair::new()
        }
    };

    let (steward_staker, _) = Pubkey::find_program_address(
        &[Staker::SEED, steward_config.pubkey().as_ref()],
        &program_id,
    );

    let update_parameters_args: UpdateParametersArgs =
        args.config_parameters.to_update_parameters_args();

    // Check if already created
    match client.get_account(&steward_config.pubkey()) {
        Ok(config_account) => {
            if config_account.owner == program_id {
                println!("Config account already exists");
                return;
            }
        }
        Err(_) => { /* Account does not exist, continue */ }
    }

    let init_ix = Instruction {
        program_id: program_id,
        accounts: jito_steward::accounts::InitializeConfig {
            config: steward_config.pubkey(),
            stake_pool: args.stake_pool,
            staker: steward_staker,
            stake_pool_program: spl_stake_pool::id(),
            system_program: anchor_lang::solana_program::system_program::id(),
            signer: staker_keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::InitializeConfig {
            authority: authority.pubkey(),
            update_parameters_args,
        }
        .data(),
    };

    let blockhash = client
        .get_latest_blockhash()
        .expect("Failed to get recent blockhash");

    let transaction = Transaction::new_signed_with_payer(
        &[init_ix],
        Some(&authority.pubkey()),
        &[&authority, &steward_config, &staker_keypair],
        blockhash,
    );

    let signature = client
        .send_and_confirm_transaction_with_spinner(&transaction)
        .expect("Failed to send transaction");
    println!("Signature: {}", signature);
    println!("Steward Config: {}", steward_config.pubkey());
}
