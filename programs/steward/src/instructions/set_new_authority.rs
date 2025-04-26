#[cfg(feature = "idl-build")]
use anchor_lang::idl::types::*;
use anchor_lang::prelude::*;
#[cfg(feature = "idl-build")]
use anchor_lang::IdlBuild;
use borsh::{BorshDeserialize, BorshSerialize};

use crate::{errors::StewardError, state::Config};

#[repr(u8)]
#[derive(BorshSerialize, BorshDeserialize, Debug, Clone, PartialEq)]
pub enum AuthorityType {
    SetAdmin = 0,
    SetBlacklistAuthority = 1,
    SetParametersAuthority = 2,
    SetPriorityFeeParameterAuthority = 3,
}

impl AuthorityType {
    pub fn to_u8(self) -> u8 {
        self as u8
    }
}

// Implement IdlBuild for AuthorityType
#[cfg(feature = "idl-build")]
impl IdlBuild for AuthorityType {
    fn create_type() -> Option<IdlTypeDef> {
        Some(IdlTypeDef {
            name: "AuthorityType".to_string(),
            ty: IdlTypeDefTy::Enum {
                variants: vec![
                    IdlEnumVariant {
                        name: "SetAdmin".to_string(),
                        fields: Some(IdlDefinedFields::Named(vec![IdlField {
                            name: "SetAdmin".to_string(),
                            docs: Default::default(),
                            ty: IdlType::Option(Box::new(IdlType::U8)),
                        }])),
                    },
                    IdlEnumVariant {
                        name: "SetBlacklistAuthority".to_string(),
                        fields: Some(IdlDefinedFields::Named(vec![IdlField {
                            name: "SetBlacklistAuthority".to_string(),
                            docs: Default::default(),
                            ty: IdlType::Option(Box::new(IdlType::U8)),
                        }])),
                    },
                    IdlEnumVariant {
                        name: "SetParameterAuthority".to_string(),
                        fields: Some(IdlDefinedFields::Named(vec![IdlField {
                            name: "SetParameterAuthority".to_string(),
                            docs: Default::default(),
                            ty: IdlType::Option(Box::new(IdlType::U8)),
                        }])),
                    },
                    IdlEnumVariant {
                        name: "SetPriorityFeeParameterAuthority".to_string(),
                        fields: Some(IdlDefinedFields::Named(vec![IdlField {
                            name: "SetPriorityFeeParameterAuthority".to_string(),
                            docs: Default::default(),
                            ty: IdlType::Option(Box::new(IdlType::U8)),
                        }])),
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

#[derive(Accounts)]
pub struct SetNewAuthority<'info> {
    #[account(mut)]
    pub config: AccountLoader<'info, Config>,

    /// CHECK: fine since we are not deserializing account
    pub new_authority: AccountInfo<'info>,

    #[account(mut)]
    pub admin: Signer<'info>,
}

pub fn handler(ctx: Context<SetNewAuthority>, authority_type: AuthorityType) -> Result<()> {
    let mut config = ctx.accounts.config.load_mut()?;
    if config.admin != *ctx.accounts.admin.key {
        return Err(StewardError::Unauthorized.into());
    }

    match authority_type {
        AuthorityType::SetAdmin => {
            config.admin = ctx.accounts.new_authority.key();
        }
        AuthorityType::SetBlacklistAuthority => {
            config.blacklist_authority = ctx.accounts.new_authority.key();
        }
        AuthorityType::SetParametersAuthority => {
            config.parameters_authority = ctx.accounts.new_authority.key();
        }
        AuthorityType::SetPriorityFeeParameterAuthority => {
            config.pf_setting_authority = ctx.accounts.new_authority.key();
        }
    }

    Ok(())
}
