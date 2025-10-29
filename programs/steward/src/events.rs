#[cfg(feature = "idl-build")]
use anchor_lang::idl::{
    types::{IdlEnumVariant, IdlTypeDef, IdlTypeDefTy},
    IdlBuild,
};
use anchor_lang::prelude::{event, AnchorDeserialize, AnchorSerialize};
use anchor_lang::{solana_program::pubkey::Pubkey, Discriminator};
use borsh::{BorshDeserialize, BorshSerialize};

#[event]
#[derive(Debug, Clone)]

pub struct AutoRemoveValidatorEvent {
    pub validator_list_index: u64,
    pub vote_account: Pubkey,
    pub vote_account_closed: bool,
    pub stake_account_deactivated: bool,
    pub marked_for_immediate_removal: bool,
}

#[event]
#[derive(Debug, Clone)]
pub struct AutoAddValidatorEvent {
    pub validator_list_index: u64,
    pub vote_account: Pubkey,
}

#[event]
#[derive(Debug, Clone)]
pub struct EpochMaintenanceEvent {
    pub validator_index_to_remove: Option<u64>,
    pub validator_list_length: u64,
    pub num_pool_validators: u64,
    pub validators_to_remove: u64,
    pub validators_to_add: u64,
    pub maintenance_complete: bool,
}

#[event]
#[derive(Debug, Clone)]
pub struct StateTransition {
    pub epoch: u64,
    pub slot: u64,
    pub previous_state: String,
    pub new_state: String,
}

#[event]
#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub struct DecreaseComponents {
    pub scoring_unstake_lamports: u64,
    pub instant_unstake_lamports: u64,
    pub stake_deposit_unstake_lamports: u64,
    pub total_unstake_lamports: u64,
    pub directed_unstake_lamports: u64,
}

#[event]
#[derive(Debug, Clone)]
pub struct RebalanceEvent {
    pub vote_account: Pubkey,
    pub epoch: u16,
    pub rebalance_type_tag: RebalanceTypeTag,
    pub increase_lamports: u64,
    pub decrease_components: DecreaseComponents,
}

#[derive(BorshSerialize, BorshDeserialize, Debug, Clone)]
pub enum RebalanceTypeTag {
    None,
    Increase,
    Decrease,
}

#[cfg(feature = "idl-build")]
impl IdlBuild for RebalanceTypeTag {
    fn get_full_path() -> String {
        "RebalanceTypeTag".to_string()
    }

    fn create_type() -> Option<IdlTypeDef> {
        Some(IdlTypeDef {
            name: "RebalanceTypeTag".to_string(),
            ty: IdlTypeDefTy::Enum {
                variants: vec![
                    IdlEnumVariant {
                        name: "None".to_string(),
                        fields: None,
                    },
                    IdlEnumVariant {
                        name: "Increase".to_string(),
                        fields: None,
                    },
                    IdlEnumVariant {
                        name: "Decrease".to_string(),
                        fields: None,
                    },
                ],
            },
            docs: Default::default(),
            generics: Default::default(),
            serialization: Default::default(),
            repr: Default::default(),
        })
    }

    fn insert_types(_types: &mut std::collections::BTreeMap<String, IdlTypeDef>) {}
}

/// Deprecated: This struct is no longer emitted but is kept to allow parsing of old events.
/// Because the event discriminator is based on struct name, it's important to rename the struct if
/// fields are changed.
#[event]
#[derive(Debug, PartialEq)]
pub struct ScoreComponents {
    pub score: f64,
    pub yield_score: f64,
    pub mev_commission_score: f64,
    pub blacklisted_score: f64,
    pub superminority_score: f64,
    pub delinquency_score: f64,
    pub running_jito_score: f64,
    pub commission_score: f64,
    pub historical_commission_score: f64,
    pub vote_credits_ratio: f64,
    pub vote_account: Pubkey,
    pub epoch: u16,
}

/// Deprecated: This struct is no longer emitted but is kept to allow parsing of old events.
/// Because the event discriminator is based on struct name, it's important to rename the struct if
/// fields are changed.
#[event]
#[derive(Debug, PartialEq, Eq)]
pub struct InstantUnstakeComponents {
    pub instant_unstake: bool,
    pub delinquency_check: bool,
    pub commission_check: bool,
    pub mev_commission_check: bool,
    pub is_blacklisted: bool,
    pub vote_account: Pubkey,
    pub epoch: u16,
}
