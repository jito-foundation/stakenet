use std::sync::Arc;

use anchor_lang::{InstructionData, ToAccountMetas};
use anyhow::Result;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::instruction::Instruction;

use solana_sdk::{
    pubkey::Pubkey, signature::read_keypair_file, signer::Signer, transaction::Transaction,
};

use crate::{
    commands::command_args::Surgery,
    utils::{
        accounts::{
            get_steward_config_account, get_steward_state_account, get_steward_state_address,
            get_validator_list_account,
        },
        transactions::configure_instruction,
    },
};

pub async fn command_surgery(
    args: Surgery,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    let validator_list_index: u64 = args.validator_list_index as u64;
    let mark_for_removal: u8 = {
        if args.mark_for_removal {
            0xFF // TRUE
        } else {
            0x00 // FALSE
        }
    };
    let immediate: u8 = {
        if args.immediate {
            0xFF // TRUE
        } else {
            0x00 // FALSE
        }
    };

    let authority = read_keypair_file(args.permissioned_parameters.authority_keypair_path)
        .expect("Failed reading keypair file ( Authority )");

    let steward_config_address = args.permissioned_parameters.steward_config;
    let steward_state_address = get_steward_state_address(&program_id, &steward_config_address);

    let steward_config_account =
        get_steward_config_account(client, &steward_config_address).await?;
    let validator_list_account =
        get_validator_list_account(client, &steward_config_account.validator_list).await?;

    {
        println!("Submit: {}", args.submit_ix);

        println!("Validator list index: {}", validator_list_index);
        println!("Mark for removal: {}", mark_for_removal);
        println!("Immediate: {}", immediate);

        let validator_to_mark = validator_list_account
            .validators
            .get(validator_list_index as usize)
            .unwrap();

        println!("Validator to mark: {:?}", validator_to_mark);
    }

    if args.submit_ix {
        let ix = Instruction {
            program_id,
            accounts: jito_steward::accounts::AdminMarkForRemoval {
                state_account: steward_state_address,
                config: steward_config_address,
                authority: authority.pubkey(),
            }
            .to_account_metas(None),
            data: jito_steward::instruction::AdminMarkForRemoval {
                validator_list_index,
                mark_for_removal,
                immediate,
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
