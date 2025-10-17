use std::sync::Arc;

use crate::cli_signer::CliSigner;
use anchor_lang::{AccountDeserialize, InstructionData, ToAccountMetas};
use anyhow::Result;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::instruction::Instruction;
use squads_multisig::client::{
    get_multisig, proposal_create, vault_transaction_create, ProposalCreateAccounts,
    ProposalCreateArgs, VaultTransactionCreateAccounts,
};
use squads_multisig::pda::{get_proposal_pda, get_transaction_pda, get_vault_pda};
use squads_multisig::state::TransactionMessage;
use squads_multisig::vault_transaction::VaultTransactionMessageExt;

use crate::utils::transactions::{configure_instruction, maybe_print_tx};
#[allow(deprecated)]
use solana_sdk::{
    pubkey::Pubkey, signature::read_keypair_file, signer::Signer, system_program,
    transaction::Transaction,
};

use crate::commands::command_args::AddToBlacklist;
use stakenet_sdk::utils::accounts::get_validator_history_address;
use validator_history::{self, ValidatorHistory};

pub async fn command_add_to_blacklist(
    args: AddToBlacklist,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
    global_signer: Option<&str>,
) -> Result<()> {
    // Use global signer - required for this command
    let signer_path = global_signer.expect("--signer flag is required for this command");

    // Create the appropriate signer based on the path (USB for Ledger, file path for keypair)
    let authority = if signer_path.starts_with("usb://") {
        CliSigner::new_ledger(signer_path)
    } else {
        CliSigner::new_keypair_from_path(signer_path)?
    };

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

    let blacklist_ix = Instruction {
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

    // If Squads proposal flag is set, create a Squads proposal
    if args.squads_proposal {
        let multisig = args.squads_multisig;
        let squads_program_id = args
            .squads_program_id
            .unwrap_or(squads_multisig::squads_multisig_program::ID);

        println!("  Multisig Address: {}", multisig);
        println!("  Squads Program ID: {}", squads_program_id);

        // Fetch the multisig account to get the transaction index
        println!("  Fetching multisig account...");
        let multisig_account = get_multisig(client, &multisig).await.map_err(|e| {
            eprintln!("❌ Failed to fetch multisig account: {}", e);
            e
        })?;
        let transaction_index = multisig_account.transaction_index + 1;
        println!("  Next transaction index: {}", transaction_index);

        // Derive PDAs
        let vault_pda =
            get_vault_pda(&multisig, args.squads_vault_index, Some(&squads_program_id)).0;
        let transaction_pda =
            get_transaction_pda(&multisig, transaction_index, Some(&squads_program_id)).0;
        let proposal_pda =
            get_proposal_pda(&multisig, transaction_index, Some(&squads_program_id)).0;

        println!("  Vault PDA: {}", vault_pda);
        println!("  Transaction PDA: {}", transaction_pda);
        println!("  Proposal PDA: {}", proposal_pda);

        // Create the transaction message for the vault transaction
        let message = TransactionMessage::try_compile(&vault_pda, &[blacklist_ix], &[])?;

        // Create vault transaction instruction
        let vault_tx_ix = vault_transaction_create(
            VaultTransactionCreateAccounts {
                multisig,
                transaction: transaction_pda,
                creator: authority.pubkey(),
                rent_payer: authority.pubkey(),
                system_program: system_program::id(),
            },
            args.squads_vault_index,
            0, // num_ephemeral_signers
            &message,
            Some("Add validators to blacklist".to_string()),
            Some(squads_program_id),
        );

        // Create proposal instruction
        let proposal_ix = proposal_create(
            ProposalCreateAccounts {
                multisig,
                creator: authority.pubkey(),
                proposal: proposal_pda,
                rent_payer: authority.pubkey(),
                system_program: system_program::id(),
            },
            ProposalCreateArgs {
                transaction_index,
                draft: false,
            },
            Some(squads_program_id),
        );

        let blockhash = client.get_latest_blockhash().await?;

        let configured_ixs = configure_instruction(
            &[vault_tx_ix, proposal_ix],
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
            &configured_ixs,
            &args.permissioned_parameters.transaction_parameters,
        ) {
            let transaction = Transaction::new_signed_with_payer(
                &configured_ixs,
                Some(&authority.pubkey()),
                &[&authority],
                blockhash,
            );
            let signature = client
                .send_and_confirm_transaction_with_spinner(&transaction)
                .await?;

            println!("Squads proposal created!");
            println!("Signature: {}", signature);
        }
    } else {
        // Direct execution
        let blockhash = client.get_latest_blockhash().await?;

        let configured_ix = configure_instruction(
            &[blacklist_ix],
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
    }

    Ok(())
}
