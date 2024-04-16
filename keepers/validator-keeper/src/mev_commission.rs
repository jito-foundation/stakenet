use std::{collections::HashMap, str::FromStr, sync::Arc};

use anchor_lang::{AccountDeserialize, InstructionData, ToAccountMetas};
use jito_tip_distribution::sdk::derive_tip_distribution_account_address;
use jito_tip_distribution::state::TipDistributionAccount;
use keeper_core::{
    build_create_and_update_instructions, get_multiple_accounts_batched,
    get_vote_accounts_with_retry, submit_create_and_update, Address, CreateTransaction,
    CreateUpdateStats, MultipleAccountsError, UpdateInstruction,
};
use log::error;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_response::RpcVoteAccountInfo;
use solana_program::{instruction::Instruction, pubkey::Pubkey};
use solana_sdk::{signature::Keypair, signer::Signer};
use validator_history::constants::MIN_VOTE_EPOCHS;
use validator_history::ValidatorHistoryEntry;
use validator_history::{constants::MAX_ALLOC_BYTES, Config, ValidatorHistory};

use crate::{get_validator_history_accounts_with_retry, KeeperError, PRIORITY_FEE};

#[derive(Clone)]
pub struct ValidatorMevCommissionEntry {
    pub vote_account: Pubkey,
    pub tip_distribution_account: Pubkey,
    pub validator_history_account: Pubkey,
    pub config: Pubkey,
    pub program_id: Pubkey,
    pub signer: Pubkey,
    pub epoch: u64,
}

impl ValidatorMevCommissionEntry {
    pub fn new(
        vote_account: &RpcVoteAccountInfo,
        epoch: u64,
        program_id: &Pubkey,
        tip_distribution_program_id: &Pubkey,
        signer: &Pubkey,
    ) -> Self {
        let vote_account = Pubkey::from_str(&vote_account.vote_pubkey)
            .map_err(|e| {
                error!("Invalid vote account pubkey");
                e
            })
            .expect("Invalid vote account pubkey");
        let (validator_history_account, _) = Pubkey::find_program_address(
            &[ValidatorHistory::SEED, &vote_account.to_bytes()],
            program_id,
        );
        let (tip_distribution_account, _) = derive_tip_distribution_account_address(
            tip_distribution_program_id,
            &vote_account,
            epoch,
        );
        let (config, _) = Pubkey::find_program_address(&[Config::SEED], program_id);
        Self {
            vote_account,
            tip_distribution_account,
            validator_history_account,
            config,
            program_id: *program_id,
            signer: *signer,
            epoch,
        }
    }
}

impl Address for ValidatorMevCommissionEntry {
    fn address(&self) -> Pubkey {
        self.validator_history_account
    }
}

impl CreateTransaction for ValidatorMevCommissionEntry {
    fn create_transaction(&self) -> Vec<Instruction> {
        let mut ixs = vec![Instruction {
            program_id: self.program_id,
            accounts: validator_history::accounts::InitializeValidatorHistoryAccount {
                validator_history_account: self.validator_history_account,
                vote_account: self.vote_account,
                system_program: solana_program::system_program::id(),
                signer: self.signer,
            }
            .to_account_metas(None),
            data: validator_history::instruction::InitializeValidatorHistoryAccount {}.data(),
        }];
        let num_reallocs = (ValidatorHistory::SIZE - MAX_ALLOC_BYTES) / MAX_ALLOC_BYTES + 1;
        ixs.extend(vec![
            Instruction {
                program_id: self.program_id,
                accounts: validator_history::accounts::ReallocValidatorHistoryAccount {
                    validator_history_account: self.validator_history_account,
                    vote_account: self.vote_account,
                    config: self.config,
                    system_program: solana_program::system_program::id(),
                    signer: self.signer,
                }
                .to_account_metas(None),
                data: validator_history::instruction::ReallocValidatorHistoryAccount {}.data(),
            };
            num_reallocs
        ]);
        ixs
    }
}

impl UpdateInstruction for ValidatorMevCommissionEntry {
    fn update_instruction(&self) -> Instruction {
        Instruction {
            program_id: self.program_id,
            accounts: validator_history::accounts::CopyTipDistributionAccount {
                validator_history_account: self.validator_history_account,
                vote_account: self.vote_account,
                tip_distribution_account: self.tip_distribution_account,
                config: self.config,
                signer: self.signer,
            }
            .to_account_metas(None),
            data: validator_history::instruction::CopyTipDistributionAccount { epoch: self.epoch }
                .data(),
        }
    }
}

pub async fn update_mev_commission(
    client: Arc<RpcClient>,
    keypair: Arc<Keypair>,
    validator_history_program_id: &Pubkey,
    tip_distribution_program_id: &Pubkey,
) -> Result<CreateUpdateStats, KeeperError> {
    let epoch = client.get_epoch_info().await?.epoch;

    let vote_accounts = get_vote_accounts_with_retry(&client, MIN_VOTE_EPOCHS, None).await?;
    let validator_histories =
        get_validator_history_accounts_with_retry(&client, *validator_history_program_id).await?;

    let validator_history_map =
        HashMap::from_iter(validator_histories.iter().map(|vh| (vh.vote_account, vh)));

    let entries = vote_accounts
        .iter()
        .map(|vote_account| {
            ValidatorMevCommissionEntry::new(
                vote_account,
                epoch,
                validator_history_program_id,
                tip_distribution_program_id,
                &keypair.pubkey(),
            )
        })
        .collect::<Vec<ValidatorMevCommissionEntry>>();

    let existing_entries = get_existing_entries(client.clone(), &entries).await?;

    let entries_to_update = existing_entries
        .into_iter()
        .filter(|entry| !mev_commission_uploaded(&validator_history_map, entry.address(), epoch))
        .collect::<Vec<ValidatorMevCommissionEntry>>();
    let (create_transactions, update_instructions) =
        build_create_and_update_instructions(&client, &entries_to_update).await?;

    submit_create_and_update(
        &client,
        create_transactions,
        update_instructions,
        &keypair,
        PRIORITY_FEE,
    )
    .await
    .map_err(|e| e.into())
}

pub async fn update_mev_earned(
    client: &Arc<RpcClient>,
    keypair: &Arc<Keypair>,
    validator_history_program_id: &Pubkey,
    tip_distribution_program_id: &Pubkey,
) -> Result<CreateUpdateStats, KeeperError> {
    let epoch = client.get_epoch_info().await?.epoch;

    let vote_accounts = get_vote_accounts_with_retry(client, MIN_VOTE_EPOCHS, None).await?;
    let validator_histories =
        get_validator_history_accounts_with_retry(client, *validator_history_program_id).await?;

    let validator_history_map =
        HashMap::from_iter(validator_histories.iter().map(|vh| (vh.vote_account, vh)));

    let entries = vote_accounts
        .iter()
        .map(|vote_account| {
            ValidatorMevCommissionEntry::new(
                vote_account,
                epoch.saturating_sub(1), // TDA derived from the prev epoch since the merkle roots are uploaded shortly after rollover
                validator_history_program_id,
                tip_distribution_program_id,
                &keypair.pubkey(),
            )
        })
        .collect::<Vec<ValidatorMevCommissionEntry>>();

    let uploaded_merkleroot_entries =
        get_entries_with_uploaded_merkleroot(client, &entries).await?;

    let entries_to_update = uploaded_merkleroot_entries
        .into_iter()
        .filter(|entry| !mev_earned_uploaded(&validator_history_map, entry.address(), epoch - 1))
        .collect::<Vec<ValidatorMevCommissionEntry>>();

    let (create_transactions, update_instructions) =
        build_create_and_update_instructions(client, &entries_to_update).await?;

    submit_create_and_update(
        client,
        create_transactions,
        update_instructions,
        keypair,
        PRIORITY_FEE,
    )
    .await
    .map_err(|e| e.into())
}

async fn get_existing_entries(
    client: Arc<RpcClient>,
    entries: &[ValidatorMevCommissionEntry],
) -> Result<Vec<ValidatorMevCommissionEntry>, MultipleAccountsError> {
    /* Filters tip distribution tuples to the addresses, then fetches accounts to see which ones exist */
    let tip_distribution_addresses = entries
        .iter()
        .map(|entry| entry.tip_distribution_account)
        .collect::<Vec<Pubkey>>();

    let accounts = get_multiple_accounts_batched(&tip_distribution_addresses, &client).await?;
    let result = accounts
        .iter()
        .enumerate()
        .filter_map(|(i, account_data)| {
            if account_data.is_some() {
                Some(entries[i].clone())
            } else {
                None
            }
        })
        .collect::<Vec<ValidatorMevCommissionEntry>>();
    // Fetch existing tip distribution accounts for this epoch
    Ok(result)
}

async fn get_entries_with_uploaded_merkleroot(
    client: &Arc<RpcClient>,
    entries: &[ValidatorMevCommissionEntry],
) -> Result<Vec<ValidatorMevCommissionEntry>, MultipleAccountsError> {
    /* Filters tip distribution tuples to the addresses, then fetches accounts to see which ones have an uploaded merkle root */
    let tip_distribution_addresses = entries
        .iter()
        .map(|entry| entry.tip_distribution_account)
        .collect::<Vec<Pubkey>>();

    let accounts = get_multiple_accounts_batched(&tip_distribution_addresses, client).await?;
    let result = accounts
        .iter()
        .enumerate()
        .filter_map(|(i, account_data)| {
            if let Some(account_data) = account_data {
                let mut data: &[u8] = &account_data.data;
                if let Ok(tda) = TipDistributionAccount::try_deserialize(&mut data) {
                    if tda.merkle_root.is_some() {
                        return Some(entries[i].clone());
                    }
                }
            }
            None
        })
        .collect::<Vec<ValidatorMevCommissionEntry>>();
    // Fetch tip distribution accounts with uploaded merkle roots for this epoch
    Ok(result)
}

fn mev_commission_uploaded(
    validator_history_map: &HashMap<Pubkey, &ValidatorHistory>,
    vote_account: Pubkey,
    epoch: u64,
) -> bool {
    if let Some(validator_history) = validator_history_map.get(&vote_account) {
        if let Some(latest_entry) = validator_history.history.last() {
            return latest_entry.epoch == epoch as u16
                && latest_entry.mev_commission != ValidatorHistoryEntry::default().mev_commission;
        }
    }
    false
}

fn mev_earned_uploaded(
    validator_history_map: &HashMap<Pubkey, &ValidatorHistory>,
    vote_account: Pubkey,
    epoch: u64,
) -> bool {
    if let Some(validator_history) = validator_history_map.get(&vote_account) {
        if let Some(latest_entry) = validator_history
            .history
            .epoch_range(epoch as u16, epoch as u16)[0]
        {
            return latest_entry.epoch == epoch as u16
                && latest_entry.mev_earned != ValidatorHistoryEntry::default().mev_earned;
        }
    };
    false
}
