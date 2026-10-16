use std::{num::NonZeroU32, sync::Arc};

use anchor_lang::{AccountDeserialize, InstructionData, ToAccountMetas};
use anyhow::Result;
use clap::Parser;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::instruction::Instruction;
#[allow(deprecated)]
use solana_sdk::{
    pubkey::Pubkey, signature::read_keypair_file, signer::Signer, stake, system_program, sysvar,
    transaction::Transaction,
};
use spl_stake_pool::find_stake_program_address;
use stakenet_sdk::utils::{
    accounts::{get_all_steward_accounts, get_validator_history_address},
    transactions::configure_instruction,
};
use validator_history::id as validator_history_id;

use crate::{commands::command_args::PermissionedParameters, utils::transactions::maybe_print_tx};

#[derive(Parser)]
#[command(about = "Admin-only: adds a validator to the pool")]
pub struct ManuallyAddValidator {
    #[command(flatten)]
    pub permissioned_parameters: PermissionedParameters,

    /// Validator vote account to add
    #[arg(long, env)]
    pub vote_account: Pubkey,

    /// Optional validator seed for the stake account derivation
    #[arg(long, env)]
    pub validator_seed: Option<u32>,

    /// Skip the check that the ValidatorHistory account exists.
    /// Adding a validator without one will stall the state machine at the next
    /// scoring cycle - only use this if you know what you're doing.
    #[arg(long, env, default_value = "false")]
    pub skip_validator_history_check: bool,

    /// Skip the check that the steward is not mid-scoring-cycle.
    /// Adding during ComputeScores risks the validator being pulled into the
    /// current cohort if scoring restarts - only use this if you know what you're doing.
    #[arg(long, env, default_value = "false")]
    pub skip_state_check: bool,
}

/// Admin passthrough to `spl_stake_pool::add_validator_to_pool`.
///
/// Unlike `auto-add-validator-from-pool`, this does not enforce
/// `minimum_stake_lamports` / `minimum_voting_epochs`, so it can add a validator
/// that the permissionless path would reject.
///
/// Because the steward has no on-chain way to add the validator history account
/// later, and `compute_score` / `compute_instant_unstake` / `rebalance` all require
/// it once the validator enters the scoring cohort, we refuse to build the
/// instruction unless that account already exists. Override with
/// `--skip-validator-history-check` only if you know what you're doing.
pub async fn command_manually_add_validator(
    args: ManuallyAddValidator,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    let steward_config = args.permissioned_parameters.steward_config;
    let vote_account = args.vote_account;

    // Determine authority pubkey. When printing, allow a provided flag or fall back
    // to the on-chain admin, so no keypair is required to produce a Squads payload.
    let authority_pubkey = if args.permissioned_parameters.transaction_parameters.print_tx
        || args
            .permissioned_parameters
            .transaction_parameters
            .print_gov_tx
    {
        if let Some(pubkey) = args.permissioned_parameters.authority_pubkey {
            pubkey
        } else {
            let config_account = client.get_account(&steward_config).await?;
            let config =
                jito_steward::Config::try_deserialize(&mut config_account.data.as_slice())?;
            config.admin
        }
    } else {
        read_keypair_file(&args.permissioned_parameters.authority_keypair_path)
            .expect("Failed reading keypair file ( Authority )")
            .pubkey()
    };

    let validator_history_program_id = validator_history_id();
    let history_account =
        get_validator_history_address(&vote_account, &validator_history_program_id);

    // Safety gate: adding a validator without a ValidatorHistory account will stall
    // the state machine at the next scoring cycle, since progress.is_complete() can
    // never be satisfied for an index whose instruction always fails.
    if !args.skip_validator_history_check {
        let history_exists = client
            .get_account_with_commitment(&history_account, client.commitment())
            .await?
            .value
            .is_some();

        if !history_exists {
            return Err(anyhow::anyhow!(
                "ValidatorHistory account {history_account} does not exist for vote account \
                 {vote_account}.\n\n\
                 Adding this validator to the pool now will stall the steward state machine at \
                 the next scoring cycle: compute_score, compute_instant_unstake and rebalance all \
                 require this account, and the cycle cannot complete while any validator's \
                 instruction fails.\n\n\
                 Create it first (requires >= 5 epochs of vote credits), then re-run:\n  \
                 validator-history-cli init-validator-history --vote-account {vote_account}\n\n\
                 To bypass this check, pass --skip-validator-history-check."
            ));
        }
    }

    let steward_accounts = get_all_steward_accounts(client, &program_id, &steward_config).await?;

    let validator_seed = NonZeroU32::new(args.validator_seed.unwrap_or_default());

    let (stake_address, _) = find_stake_program_address(
        &spl_stake_pool::id(),
        &vote_account,
        &steward_accounts.stake_pool_address,
        validator_seed,
    );

    let ix = Instruction {
        program_id,
        accounts: jito_steward::accounts::AddValidatorToPool {
            admin: authority_pubkey,
            config: steward_config,
            state_account: steward_accounts.state_address,
            stake_pool_program: spl_stake_pool::id(),
            stake_pool: steward_accounts.stake_pool_address,
            reserve_stake: steward_accounts.stake_pool_account.reserve_stake,
            withdraw_authority: steward_accounts.stake_pool_withdraw_authority,
            validator_list: steward_accounts.validator_list_address,
            stake_account: stake_address,
            vote_account,
            rent: sysvar::rent::id(),
            clock: sysvar::clock::id(),
            stake_history: sysvar::stake_history::id(),
            stake_config: stake::config::ID,
            system_program: system_program::id(),
            stake_program: stake::program::id(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::AddValidatorToPool {
            validator_seed: args.validator_seed,
        }
        .data(),
    };

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

    // If printing, do so and return early without requiring the authority keypair
    if maybe_print_tx(
        &configured_ix,
        &args.permissioned_parameters.transaction_parameters,
    ) {
        return Ok(());
    }

    let authority = read_keypair_file(&args.permissioned_parameters.authority_keypair_path)
        .expect("Failed reading keypair file ( Authority )");

    let blockhash = client.get_latest_blockhash().await?;

    let transaction = Transaction::new_signed_with_payer(
        &configured_ix,
        Some(&authority.pubkey()),
        &[&authority],
        blockhash,
    );

    let signature = client
        .send_and_confirm_transaction_with_spinner(&transaction)
        .await?;

    println!("Signature: {signature}");

    Ok(())
}
