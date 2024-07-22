use anchor_lang::idl::{
    types::{IdlEnumVariant, IdlTypeDef, IdlTypeDefTy},
    IdlBuild,
};
use anchor_lang::prelude::{event, AnchorDeserialize, AnchorSerialize};
use anchor_lang::solana_program::pubkey::Pubkey;
use borsh::{BorshDeserialize, BorshSerialize};

#[event]
pub struct AutoRemoveValidatorEvent {
    pub validator_list_index: u64,
    pub vote_account: Pubkey,
    pub vote_account_closed: bool,
    pub stake_account_deactivated: bool,
    pub marked_for_immediate_removal: bool,
}

#[event]
pub struct AutoAddValidatorEvent {
    pub validator_list_index: u64,
    pub vote_account: Pubkey,
}

#[event]
pub struct EpochMaintenanceEvent {
    pub validator_index_to_remove: Option<u64>,
    pub validator_list_length: u64,
    pub num_pool_validators: u64,
    pub validators_to_remove: u64,
    pub validators_to_add: u64,
    pub maintenance_complete: bool,
}

#[event]
#[derive(Debug)]
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
}

#[event]
pub struct RebalanceEvent {
    pub vote_account: Pubkey,
    pub epoch: u16,
    pub rebalance_type_tag: RebalanceTypeTag,
    pub increase_lamports: u64,
    pub decrease_components: DecreaseComponents,
}

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub enum RebalanceTypeTag {
    None,
    Increase,
    Decrease,
}

impl IdlBuild for RebalanceTypeTag {
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
}
