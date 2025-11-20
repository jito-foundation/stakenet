use std::sync::Arc;

use anchor_lang::{AccountDeserialize, InstructionData, ToAccountMetas};
use anyhow::Result;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::instruction::Instruction;
use solana_sdk::{pubkey::Pubkey, signer::Signer, transaction::Transaction};
use squads_multisig::{
    client::{
        get_multisig, proposal_create, vault_transaction_create, ProposalCreateAccounts,
        ProposalCreateArgs, VaultTransactionCreateAccounts,
    },
    pda::{get_proposal_pda, get_transaction_pda, get_vault_pda},
    state::TransactionMessage,
    vault_transaction::VaultTransactionMessageExt,
};
use stakenet_sdk::utils::transactions::{configure_instruction, print_base58_tx};

use crate::{
    cli_signer::CliSigner,
    commands::command_args::{AuthoritySubcommand, UpdateAuthority},
    utils::transactions::maybe_print_tx,
};

pub async fn command_update_authority(
    args: UpdateAuthority,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
    cli_signer: &CliSigner,
) -> Result<()> {
    let (permissioned_parameters, new_authority, authority_type) = match args.command {
        AuthoritySubcommand::Blacklist {
            permissioned_parameters,
            new_authority,
        } => (
            permissioned_parameters,
            new_authority,
            jito_steward::instructions::set_new_authority::AuthorityType::SetBlacklistAuthority,
        ),
        AuthoritySubcommand::Admin {
            permissioned_parameters,
            new_authority,
        } => (
            permissioned_parameters,
            new_authority,
            jito_steward::instructions::set_new_authority::AuthorityType::SetAdmin,
        ),
        AuthoritySubcommand::Parameters {
            permissioned_parameters,
            new_authority,
        } => (
            permissioned_parameters,
            new_authority,
            jito_steward::instructions::set_new_authority::AuthorityType::SetParametersAuthority,
        ),
        AuthoritySubcommand::PriorityFeeParameters {
            permissioned_parameters,
            new_authority,
        } => (
            permissioned_parameters,
            new_authority,
            jito_steward::instructions::set_new_authority::AuthorityType::SetPriorityFeeParameterAuthority,
        ),
        AuthoritySubcommand::DirectedStakeMetaUpload {
            permissioned_parameters,
            new_authority,
        } => (
            permissioned_parameters,
            new_authority,
            jito_steward::instructions::set_new_authority::AuthorityType::SetDirectedStakeMetaUploadAuthority,
        ),
        AuthoritySubcommand::DirectedStakeWhitelist {
            permissioned_parameters,
            new_authority,
        } => (
            permissioned_parameters,
            new_authority,
            jito_steward::instructions::set_new_authority::AuthorityType::SetDirectedStakeWhitelistAuthority,
        ),
    };

    let config_account = client
        .get_account(&permissioned_parameters.steward_config)
        .await?;
    let config = jito_steward::Config::try_deserialize(&mut config_account.data.as_slice())?;
    let admin = config.admin;

    let steward_config = permissioned_parameters.steward_config;

    let ix = Instruction {
        program_id,
        accounts: jito_steward::accounts::SetNewAuthority {
            config: steward_config,
            new_authority,
            admin,
        }
        .to_account_metas(None),
        data: jito_steward::instruction::SetNewAuthority { authority_type }.data(),
    };

    if args.squads_proposal {
        let multisig = args.squads_multisig;
        let squads_program_id = args
            .squads_program_id
            .unwrap_or(squads_multisig::squads_multisig_program::ID);

        println!("  Multisig Address: {}", multisig);
        println!("  Squads Program ID: {}", squads_program_id);

        // Fetch the multisig account to get the transaction index
        println!("  Fetching multisig account...");
        let multisig_account = get_multisig(client, &multisig).await?;
        let transaction_index = multisig_account.transaction_index + 1;
        println!("  Next transaction index: {}", transaction_index);

        // Derive PDAs
        let vault_pda =
            get_vault_pda(&multisig, args.squads_vault_index, Some(&squads_program_id)).0;
        let transaction_pda =
            get_transaction_pda(&multisig, transaction_index, Some(&squads_program_id)).0;
        let proposal_pda =
            get_proposal_pda(&multisig, transaction_index, Some(&squads_program_id)).0;

        if vault_pda != admin {
            return Err(anyhow::anyhow!(
                "Vault PDA {} does not match configured admin {}",
                vault_pda,
                admin
            ));
        }

        println!("  Vault PDA: {}", vault_pda);
        println!("  Transaction PDA: {}", transaction_pda);
        println!("  Proposal PDA: {}", proposal_pda);

        // Create the transaction message for the vault transaction
        let message = TransactionMessage::try_compile(&vault_pda, &[ix], &[])?;

        // Create vault transaction instruction
        let vault_tx_ix = vault_transaction_create(
            VaultTransactionCreateAccounts {
                multisig,
                transaction: transaction_pda,
                creator: cli_signer.pubkey(),
                rent_payer: cli_signer.pubkey(),
                system_program: solana_system_interface::program::id(),
            },
            args.squads_vault_index,
            0, // num_ephemeral_signers
            &message,
            Some("Set new authority".to_string()),
            Some(squads_program_id),
        );

        // Create proposal instruction
        let proposal_ix = proposal_create(
            ProposalCreateAccounts {
                multisig,
                creator: cli_signer.pubkey(),
                proposal: proposal_pda,
                rent_payer: cli_signer.pubkey(),
                system_program: solana_system_interface::program::id(),
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
            permissioned_parameters.transaction_parameters.priority_fee,
            permissioned_parameters.transaction_parameters.compute_limit,
            permissioned_parameters.transaction_parameters.heap_size,
        );

        if !maybe_print_tx(
            &configured_ixs,
            &permissioned_parameters.transaction_parameters,
        ) {
            let transaction = Transaction::new_signed_with_payer(
                &configured_ixs,
                Some(&cli_signer.pubkey()),
                &[&cli_signer],
                blockhash,
            );
            let signature = client
                .send_and_confirm_transaction_with_spinner(&transaction)
                .await?;

            println!("Squads proposal created!");
            println!("Signature: {}", signature);
        }
    } else {
        let blockhash = client.get_latest_blockhash().await?;

        let configured_ix = configure_instruction(
            &[ix],
            permissioned_parameters.transaction_parameters.priority_fee,
            permissioned_parameters.transaction_parameters.compute_limit,
            permissioned_parameters.transaction_parameters.heap_size,
        );

        if maybe_print_tx(
            &configured_ix,
            &permissioned_parameters.transaction_parameters,
        ) {
            return Ok(());
        }

        let transaction = Transaction::new_signed_with_payer(
            &configured_ix,
            Some(&cli_signer.pubkey()),
            &[&cli_signer],
            blockhash,
        );

        if permissioned_parameters.transaction_parameters.print_tx {
            print_base58_tx(&configured_ix)
        } else {
            let signature = client
                .send_and_confirm_transaction_with_spinner(&transaction)
                .await?;

            println!("Signature: {}", signature);
        }
    }

    Ok(())
}
