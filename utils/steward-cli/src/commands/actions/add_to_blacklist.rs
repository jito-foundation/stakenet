use std::sync::Arc;

use anchor_lang::{AccountDeserialize, InstructionData, ToAccountMetas};
use anyhow::Result;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::instruction::Instruction;

use crate::utils::transactions::{configure_instruction, maybe_print_tx};
use solana_sdk::{
    pubkey::Pubkey, signature::read_keypair_file, signer::Signer, transaction::Transaction,
};

use crate::commands::command_args::AddToBlacklist;
use stakenet_sdk::utils::accounts::get_validator_history_address;
use validator_history::{self, ValidatorHistory};

pub async fn command_add_to_blacklist(
    args: AddToBlacklist,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    let authority = read_keypair_file(args.permissioned_parameters.authority_keypair_path)
        .expect("Failed reading keypair file ( Authority )");

    let authority_pubkey = if args.permissioned_parameters.transaction_parameters.print_tx
        || args
            .permissioned_parameters
            .transaction_parameters
            .print_gov_tx
    {
        let config_account = client
            .get_account(&args.permissioned_parameters.steward_config)
            .await?;
        let config = jito_steward::Config::try_deserialize(&mut config_account.data.as_slice())?;
        config.blacklist_authority
    } else {
        authority.pubkey()
    };

    // Build list of indices, starting with those passed directly
    let mut indices = args.validator_history_indices_to_blacklist.clone();
    // Fetch indices for each vote account provided
    println!("Vote Account\tHistory Address\tIndex");
    for vote_account in args.vote_accounts_to_blacklist.iter() {
        let history_address = get_validator_history_address(vote_account, &validator_history::id());
        let (vh_index, account_exists) = match client.get_account(&history_address).await {
            Ok(account) => match ValidatorHistory::try_deserialize(&mut account.data.as_slice()) {
                Ok(vh) => (vh.index.to_string(), true),
                Err(_) => ("N/A".to_string(), false),
            },
            Err(_) => ("N/A".to_string(), false),
        };
        println!(
            "{}\thttps://solscan.io/account/{}\t{}",
            vote_account, history_address, vh_index
        );
        if account_exists {
            indices.push(vh_index.parse()?);
        }
    }

    let ix = Instruction {
        program_id,
        accounts: jito_steward::accounts::AddValidatorsToBlacklist {
            config: args.permissioned_parameters.steward_config,
            authority: authority_pubkey,
        }
        .to_account_metas(None),
        data: jito_steward::instruction::AddValidatorsToBlacklist {
            validator_history_blacklist: indices,
        }
        .data(),
    };

    let blockhash = client.get_latest_blockhash().await?;

    let configured_ix = configure_instruction(
        &[ix],
        args.permissioned_parameters
            .transaction_parameters
            .priority_fee,
        args.permissioned_parameters
            .transaction_parameters
            .compute_limit,
        args.permissioned_parameters
            .transaction_parameters
            .heap_size,
    );

    if !maybe_print_tx(
        &configured_ix,
        &args.permissioned_parameters.transaction_parameters,
    ) {
        let transaction = Transaction::new_signed_with_payer(
            &configured_ix,
            Some(&authority.pubkey()),
            &[&authority],
            blockhash,
        );
        let signature = client
            .send_and_confirm_transaction_with_spinner(&transaction)
            .await?;

        println!("Signature: {}", signature);
    }

    Ok(())
}
