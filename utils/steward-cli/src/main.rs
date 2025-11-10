use std::{sync::Arc, time::Duration};

use anyhow::Result;
use clap::Parser;
use commands::{
    actions::{
        add_to_blacklist::command_add_to_blacklist,
        auto_add_validator_from_pool::command_auto_add_validator_from_pool,
        auto_remove_validator_from_pool::command_auto_remove_validator_from_pool,
        instant_remove_validator::command_instant_remove_validator,
        manually_copy_all_vote_accounts::command_manually_copy_all_vote_accounts,
        manually_copy_vote_accounts::command_manually_copy_vote_account,
        manually_remove_validator::command_manually_remove_validator, pause::command_pause,
        remove_bad_validators::command_remove_bad_validators,
        remove_from_blacklist::command_remove_from_blacklist, reset_state::command_reset_state,
        reset_validator_lamport_balances::command_reset_validator_lamport_balances,
        resume::command_resume, revert_staker::command_revert_staker,
        set_staker::command_set_staker, update_authority::command_update_authority,
        update_config::command_update_config,
        update_priority_fee_config::command_update_priority_fee_config,
        update_validator_list_balance::command_update_validator_list_balance,
    },
    command_args::{Args, Commands},
    cranks::{
        compute_delegations::command_crank_compute_delegations,
        compute_instant_unstake::command_crank_compute_instant_unstake,
        compute_score::command_crank_compute_score,
        epoch_maintenance::command_crank_epoch_maintenance, idle::command_crank_idle,
        rebalance::command_crank_rebalance, steward::command_crank_steward,
    },
    info::{
        get_jitosol_balance::command_get_jitosol_balance, view_config::command_view_config,
        view_directed_stake_meta::command_view_directed_stake_meta,
        view_directed_stake_tickets::command_view_directed_stake_tickets,
        view_directed_stake_whitelist::command_view_directed_stake_whitelist,
        view_next_index_to_remove::command_view_next_index_to_remove,
        view_priority_fee_config::command_view_priority_fee_config, view_state::command_view_state,
    },
    init::{init_steward::command_init_steward, realloc_state::command_realloc_state},
};
use dotenvy::dotenv;
use solana_client::nonblocking::rpc_client::RpcClient;

use crate::{
    cli_signer::CliSigner,
    commands::{
        actions::{
            add_to_directed_stake_whitelist::command_add_to_directed_stake_whitelist,
            close_directed_stake_ticket::command_close_directed_stake_ticket,
            close_directed_stake_whitelist::command_close_directed_stake_whitelist,
            close_steward::command_close_steward, migrate_state_to_v2::command_migrate_state_to_v2,
            remove_from_directed_stake_whitelist::command_remove_from_directed_stake_whitelist,
            update_directed_stake_ticket::command_update_directed_stake_ticket,
        },
        cranks::{
            compute_directed_stake_meta::command_crank_compute_directed_stake_meta,
            rebalance_directed::command_crank_rebalance_directed,
        },
        info::{
            view_blacklist::command_view_blacklist,
            view_directed_stake_ticket::command_view_directed_stake_ticket,
        },
        init::{
            init_directed_stake_meta::command_init_directed_stake_meta,
            init_directed_stake_ticket::command_init_directed_stake_ticket,
            init_directed_stake_whitelist::command_init_directed_stake_whitelist,
            realloc_directed_stake_meta::command_realloc_directed_stake_meta,
            realloc_directed_stake_whitelist::command_realloc_directed_stake_whitelist,
        },
    },
};

pub mod cli_signer;
pub mod commands;
pub mod utils;

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok(); // Loads in .env file
    let args = Args::parse();
    let client = Arc::new(RpcClient::new_with_timeout(
        args.json_rpc_url.clone(),
        Duration::from_secs(60),
    ));

    let steward_program_id = args.steward_program_id;
    let validator_history_program_id = args.validator_history_program_id;
    let global_signer = args.signer.as_deref();
    let result = match args.commands {
        // ---- Views ----
        Commands::ViewConfig(args) => command_view_config(args, &client, steward_program_id).await,
        Commands::ViewPriorityFeeConfig(args) => {
            command_view_priority_fee_config(args, &client, steward_program_id).await
        }
        Commands::ViewState(args) => command_view_state(args, &client, steward_program_id).await,
        Commands::ViewNextIndexToRemove(args) => {
            command_view_next_index_to_remove(args, &client, steward_program_id).await
        }
        Commands::ViewBlacklist(args) => {
            command_view_blacklist(
                args,
                &client,
                steward_program_id,
                validator_history_program_id,
            )
            .await
        }
        Commands::ViewDirectedStakeTicket(args) => {
            command_view_directed_stake_ticket(args, &client, steward_program_id).await
        }
        Commands::ViewDirectedStakeTickets(args) => {
            command_view_directed_stake_tickets(args, &client, steward_program_id).await
        }
        Commands::ViewDirectedStakeWhitelist(args) => {
            command_view_directed_stake_whitelist(args, &client, steward_program_id).await
        }
        Commands::ViewDirectedStakeMeta(args) => {
            command_view_directed_stake_meta(args, &client, steward_program_id).await
        }
        Commands::GetJitosolBalance(args) => {
            command_get_jitosol_balance(args, &client, steward_program_id).await
        }

        // --- Helpers ---
        Commands::ManuallyCopyVoteAccount(args) => {
            command_manually_copy_vote_account(args, &client, steward_program_id).await
        }

        // --- Actions ---
        Commands::CloseSteward(args) => {
            command_close_steward(args, &client, steward_program_id).await
        }
        Commands::InitSteward(args) => {
            command_init_steward(args, &client, steward_program_id).await
        }
        Commands::UpdateConfig(args) => {
            command_update_config(args, &client, steward_program_id).await
        }
        Commands::UpdatePriorityFeeConfig(args) => {
            command_update_priority_fee_config(args, &client, steward_program_id).await
        }
        Commands::UpdateAuthority(args) => {
            command_update_authority(args, &client, steward_program_id).await
        }
        Commands::SetStaker(args) => command_set_staker(args, &client, steward_program_id).await,
        Commands::RevertStaker(args) => {
            command_revert_staker(args, &client, steward_program_id).await
        }
        Commands::Pause(args) => command_pause(args, &client, steward_program_id).await,
        Commands::Resume(args) => command_resume(args, &client, steward_program_id).await,
        Commands::ReallocState(args) => {
            command_realloc_state(args, &client, steward_program_id).await
        }
        Commands::MigrateStateToV2(args) => {
            command_migrate_state_to_v2(args, &client, steward_program_id).await
        }
        Commands::ResetState(args) => command_reset_state(args, &client, steward_program_id).await,
        Commands::ResetValidatorLamportBalances(args) => {
            command_reset_validator_lamport_balances(args, &client, steward_program_id).await
        }
        Commands::ManuallyRemoveValidator(args) => {
            command_manually_remove_validator(args, &client, steward_program_id).await
        }
        Commands::ManuallyCopyAllVoteAccounts(args) => {
            command_manually_copy_all_vote_accounts(args, &client, steward_program_id).await
        }
        Commands::InstantRemoveValidator(args) => {
            command_instant_remove_validator(args, &client, steward_program_id).await
        }
        Commands::AutoRemoveValidatorFromPool(args) => {
            command_auto_remove_validator_from_pool(args, &client, steward_program_id).await
        }
        Commands::AutoAddValidatorFromPool(args) => {
            command_auto_add_validator_from_pool(args, &client, steward_program_id).await
        }
        Commands::RemoveBadValidators(args) => {
            command_remove_bad_validators(args, &client, steward_program_id).await
        }
        Commands::AddToBlacklist(args) => {
            // Use global signer - required for this command
            let signer_path = global_signer.expect("--signer flag is required for this command");
            // Create the appropriate signer based on the path
            let cli_signer = if signer_path == "ledger" {
                CliSigner::new_ledger()
            } else {
                CliSigner::new_keypair_from_path(signer_path)?
            };
            command_add_to_blacklist(args, &client, steward_program_id, &cli_signer).await
        }
        Commands::RemoveFromBlacklist(args) => {
            command_remove_from_blacklist(args, &client, steward_program_id).await
        }
        Commands::UpdateValidatorListBalance(args) => {
            command_update_validator_list_balance(&client, args, steward_program_id).await
        }
        Commands::InitDirectedStakeMeta(args) => {
            command_init_directed_stake_meta(args, &client, steward_program_id).await
        }
        Commands::ReallocDirectedStakeMeta(args) => {
            command_realloc_directed_stake_meta(args, &client, steward_program_id).await
        }
        Commands::InitDirectedStakeWhitelist(args) => {
            command_init_directed_stake_whitelist(args, &client, steward_program_id).await
        }
        Commands::ReallocDirectedStakeWhitelist(args) => {
            command_realloc_directed_stake_whitelist(args, &client, steward_program_id).await
        }
        Commands::InitDirectedStakeTicket(args) => {
            command_init_directed_stake_ticket(args, &client, steward_program_id).await
        }
        Commands::AddToDirectedStakeWhitelist(args) => {
            command_add_to_directed_stake_whitelist(args, &client, steward_program_id).await
        }
        Commands::UpdateDirectedStakeTicket(args) => {
            command_update_directed_stake_ticket(args, client.clone(), steward_program_id).await
        }
        Commands::RemoveFromDirectedStakeWhitelist(args) => {
            command_remove_from_directed_stake_whitelist(args, &client, steward_program_id).await
        }
        Commands::CloseDirectedStakeTicket(args) => {
            command_close_directed_stake_ticket(args, &client, steward_program_id).await
        }
        Commands::CloseDirectedStakeWhitelist(args) => {
            command_close_directed_stake_whitelist(args, &client, steward_program_id).await
        }

        // --- Cranks ---
        Commands::CrankSteward(args) => {
            command_crank_steward(args, &client, steward_program_id).await
        }
        Commands::CrankEpochMaintenance(args) => {
            command_crank_epoch_maintenance(args, &client, steward_program_id).await
        }
        Commands::CrankComputeScore(args) => {
            command_crank_compute_score(args, &client, steward_program_id).await
        }
        Commands::CrankComputeDelegations(args) => {
            command_crank_compute_delegations(args, &client, steward_program_id).await
        }
        Commands::ComputeDirectedStakeMeta(args) => {
            command_crank_compute_directed_stake_meta(args, &client, steward_program_id).await
        }
        Commands::CrankIdle(args) => command_crank_idle(args, &client, steward_program_id).await,
        Commands::CrankComputeInstantUnstake(args) => {
            command_crank_compute_instant_unstake(args, &client, steward_program_id).await
        }
        Commands::CrankRebalance(args) => {
            command_crank_rebalance(args, &client, steward_program_id).await
        }
        Commands::CrankRebalanceDirected(args) => {
            command_crank_rebalance_directed(args, &client, steward_program_id).await
        }
    };

    if let Err(e) = result {
        eprintln!("\n❌ Error: \n\n{:?}\n", e);
        std::process::exit(1);
    }

    Ok(())
}
