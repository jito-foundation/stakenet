use std::sync::Arc;

use anchor_lang::{InstructionData, ToAccountMetas};
use anyhow::{anyhow, Result};

use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::borsh1::try_from_slice_unchecked;
use solana_program::instruction::Instruction;
#[allow(deprecated)]
use solana_sdk::{
    pubkey::Pubkey, signature::read_keypair_file, signer::Signer, stake,
    stake::state::StakeStateV2, system_program, transaction::Transaction,
};
use spl_associated_token_account::{
    get_associated_token_address, instruction::create_associated_token_account_idempotent,
};
use spl_stake_pool::{
    find_stake_program_address, instruction::deposit_sol, minimum_delegation,
    minimum_reserve_lamports,
};
use stakenet_sdk::models::aggregate_accounts::AllStewardAccounts;
use stakenet_sdk::utils::{
    accounts::{get_all_steward_accounts, get_validator_history_address},
    transactions::{configure_instruction, print_base58_tx},
};
use validator_history::id as validator_history_id;

use crate::commands::command_args::AutoAddValidatorFromPool;

/// Lamports left in the payer wallet for fees, never spent on funding the reserve.
const PAYER_WALLET_RESERVE_LAMPORTS: u64 = 100_000_000;

/// Computes how many lamports the reserve is short of what
/// `spl_stake_pool::processor::process_add_validator_to_pool` requires, which is
/// `minimum_delegation(cluster_minimum) + rent_exempt(stake_account)` on top of the reserve's own
/// `minimum_reserve_lamports` floor.
async fn reserve_shortfall(
    client: &Arc<RpcClient>,
    steward_accounts: &AllStewardAccounts,
) -> Result<u64> {
    let reserve_stake_acc = &steward_accounts.reserve_stake_account;

    let stake_rent = client
        .get_minimum_balance_for_rent_exemption(StakeStateV2::size_of())
        .await?;
    let cluster_minimum_delegation = client.get_stake_minimum_delegation().await?;

    let reserve_floor =
        match try_from_slice_unchecked::<StakeStateV2>(reserve_stake_acc.data.as_slice()) {
            Ok(state) => state.meta().map(|meta| minimum_reserve_lamports(&meta)),
            Err(_) => None,
        }
        .unwrap_or(stake_rent);

    let required_lamports =
        minimum_delegation(cluster_minimum_delegation).saturating_add(stake_rent);

    // Work from the absolute reserve balance the program demands, not from a "spendable" figure.
    // A reserve below its own floor has a deficit that a saturating subtraction would clamp to
    // zero, under-funding the deposit by exactly that deficit.
    let required_reserve = required_lamports.saturating_add(reserve_floor);
    let shortfall = required_reserve.saturating_sub(reserve_stake_acc.lamports);

    println!("Reserve lamports:   {}", reserve_stake_acc.lamports);
    println!("Reserve floor:      {reserve_floor}");
    println!("Cost of this add:   {required_lamports}");
    println!("Required reserve:   {required_reserve}");
    println!("Shortfall:          {shortfall}");

    Ok(shortfall)
}

/// Builds the `DepositSol` instructions that top the pool reserve up by `amount` lamports.
fn build_reserve_deposit_ixs(
    payer: &Pubkey,
    steward_accounts: &AllStewardAccounts,
    amount: u64,
) -> Vec<Instruction> {
    let pool_token_address =
        get_associated_token_address(payer, &steward_accounts.stake_pool_account.pool_mint);

    vec![
        create_associated_token_account_idempotent(
            payer,
            payer,
            &steward_accounts.stake_pool_account.pool_mint,
            &spl_token::id(),
        ),
        deposit_sol(
            &spl_stake_pool::id(),
            &steward_accounts.stake_pool_address,
            &steward_accounts.stake_pool_withdraw_authority,
            &steward_accounts.stake_pool_account.reserve_stake,
            payer,
            &pool_token_address,
            &steward_accounts.stake_pool_account.manager_fee_account,
            &pool_token_address,
            &steward_accounts.stake_pool_account.pool_mint,
            &spl_token::id(),
            amount,
        ),
    ]
}

pub async fn command_auto_add_validator_from_pool(
    args: AutoAddValidatorFromPool,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    // Creates config account
    let payer = Arc::new(
        read_keypair_file(args.permissionless_parameters.payer_keypair_path)
            .expect("Failed reading keypair file ( Payer )"),
    );

    let validator_history_program_id = validator_history_id();
    let steward_config = args.permissionless_parameters.steward_config;

    let steward_accounts = get_all_steward_accounts(client, &program_id, &steward_config).await?;

    let vote_account = args.vote_account;
    let history_account =
        get_validator_history_address(&vote_account, &validator_history_program_id);

    let (stake_address, _) = find_stake_program_address(
        &spl_stake_pool::id(),
        &vote_account,
        &steward_accounts.stake_pool_address,
        None,
    );

    // The stake pool program refuses the add if the reserve cannot cover the new stake account,
    // so optionally fund the shortfall in the same transaction.
    let mut ixs: Vec<Instruction> = vec![];
    if args.fund_reserve {
        let shortfall = reserve_shortfall(client, &steward_accounts).await?;

        if shortfall == 0 {
            println!("Reserve already covers this add, skipping deposit");
        } else {
            let payer_balance = client.get_balance(&payer.pubkey()).await?;
            let affordable = payer_balance.saturating_sub(PAYER_WALLET_RESERVE_LAMPORTS);

            if affordable < shortfall {
                return Err(anyhow!(
                    "Payer cannot fund reserve shortfall: needed={shortfall} affordable={affordable} payer_balance={payer_balance}"
                ));
            }

            println!(
                "Depositing {shortfall} lamports ({:.4} SOL) into the reserve",
                shortfall as f64 / 1e9
            );
            ixs.extend(build_reserve_deposit_ixs(
                &payer.pubkey(),
                &steward_accounts,
                shortfall,
            ));
        }
    }

    ixs.push(Instruction {
        program_id,
        accounts: jito_steward::accounts::AutoAddValidator {
            validator_history_account: history_account,
            steward_state: steward_accounts.state_address,
            config: args.permissionless_parameters.steward_config,
            stake_pool_program: spl_stake_pool::id(),
            stake_pool: steward_accounts.stake_pool_address,
            reserve_stake: steward_accounts.stake_pool_account.reserve_stake,
            withdraw_authority: steward_accounts.stake_pool_withdraw_authority,
            validator_list: steward_accounts.validator_list_address,
            stake_account: stake_address,
            vote_account,
            rent: solana_sdk::sysvar::rent::id(),
            clock: solana_sdk::sysvar::clock::id(),
            stake_history: solana_sdk::sysvar::stake_history::id(),
            stake_config: stake::config::ID,
            system_program: system_program::id(),
            stake_program: stake::program::id(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::AutoAddValidatorToPool {}.data(),
    });

    let blockhash = client
        .get_latest_blockhash()
        .await
        .expect("Failed to get recent blockhash");

    let configured_ix = configure_instruction(
        &ixs,
        args.permissionless_parameters
            .transaction_parameters
            .priority_fee,
        args.permissionless_parameters
            .transaction_parameters
            .compute_limit,
        args.permissionless_parameters
            .transaction_parameters
            .heap_size,
    );

    let transaction = Transaction::new_signed_with_payer(
        &configured_ix,
        Some(&payer.pubkey()),
        &[&payer],
        blockhash,
    );

    if args
        .permissionless_parameters
        .transaction_parameters
        .print_tx
    {
        print_base58_tx(&configured_ix)
    } else {
        let signature = client
            .send_and_confirm_transaction_with_spinner(&transaction)
            .await?;

        println!("Signature: {signature}");
    }

    Ok(())
}
