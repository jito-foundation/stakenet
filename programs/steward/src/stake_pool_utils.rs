use std::ops::Deref;

#[cfg(feature = "idl-build")]
use anchor_lang::idl::types::*;
#[cfg(feature = "idl-build")]
use anchor_lang::IdlBuild;

use anchor_lang::{
    prelude::{AccountInfo, ProgramError, Pubkey},
    Result,
};
use borsh::{BorshDeserialize as Borsh0Deserialize, BorshSerialize as Borsh0Serialize};
use borsh1::{BorshDeserialize, BorshSerialize};

pub struct PreferredValidatorType(spl_stake_pool::instruction::PreferredValidatorType);
impl Borsh0Deserialize for PreferredValidatorType {
    fn deserialize_reader<R: std::io::Read>(reader: &mut R) -> std::io::Result<Self> {
        Ok(Self(
            spl_stake_pool::instruction::PreferredValidatorType::deserialize_reader(reader)?,
        ))
    }
}

impl Borsh0Serialize for PreferredValidatorType {
    fn serialize<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        self.0.serialize(writer)
    }
}

impl AsRef<spl_stake_pool::instruction::PreferredValidatorType> for PreferredValidatorType {
    fn as_ref(&self) -> &spl_stake_pool::instruction::PreferredValidatorType {
        &self.0
    }
}

impl From<spl_stake_pool::instruction::PreferredValidatorType> for PreferredValidatorType {
    fn from(val: spl_stake_pool::instruction::PreferredValidatorType) -> Self {
        Self(val)
    }
}

#[cfg(feature = "idl-build")]
impl IdlBuild for PreferredValidatorType {
    fn create_type() -> Option<IdlTypeDef> {
        Some(IdlTypeDef {
            name: "PreferredValidatorType".to_string(),
            ty: IdlTypeDefTy::Enum {
                variants: vec![
                    IdlEnumVariant {
                        name: "Deposit".to_string(),
                        fields: None,
                    },
                    IdlEnumVariant {
                        name: "Withdraw".to_string(),
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

// Below are nice to haves for deserializing accounts but not strictly necessary for on-chain logic
// A good amount of this is copied from anchor
#[derive(Clone)]
pub struct StakePool(spl_stake_pool::state::StakePool);

impl AsRef<spl_stake_pool::state::StakePool> for StakePool {
    fn as_ref(&self) -> &spl_stake_pool::state::StakePool {
        &self.0
    }
}

// This is necessary so we can use "anchor_spl::token::Mint::LEN"
// because rust does not resolve "anchor_spl::token::Mint::LEN" to
// "spl_token::state::Mint::LEN" automatically
impl StakePool {
    pub const LEN: usize = std::mem::size_of::<spl_stake_pool::state::StakePool>();
}

// You don't have to implement the "try_deserialize" function
// from this trait. It delegates to
// "try_deserialize_unchecked" by default which is what we want here
// because non-anchor accounts don't have a discriminator to check
impl anchor_lang::AccountDeserialize for StakePool {
    fn try_deserialize_unchecked(buf: &mut &[u8]) -> Result<Self> {
        Ok(Self(spl_stake_pool::state::StakePool::deserialize(buf)?))
    }
}

// AccountSerialize defaults to a no-op which is what we want here
// because it's a foreign program, so our program does not
// have permission to write to the foreign program's accounts anyway
impl anchor_lang::AccountSerialize for StakePool {}

impl anchor_lang::Owner for StakePool {
    fn owner() -> Pubkey {
        spl_stake_pool::ID
    }
}

// Implement the "std::ops::Deref" trait for better user experience
impl Deref for StakePool {
    type Target = spl_stake_pool::state::StakePool;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub fn deserialize_stake_pool(
    account_info: &AccountInfo,
) -> Result<spl_stake_pool::state::StakePool> {
    if account_info.owner != &spl_stake_pool::ID {
        return Err(ProgramError::InvalidAccountOwner.into());
    }
    let data = account_info.try_borrow_data()?;
    Ok(spl_stake_pool::state::StakePool::deserialize(
        &mut data.as_ref(),
    )?)
}

pub fn deserialize_validator_list(
    account_info: &AccountInfo,
) -> Result<spl_stake_pool::state::ValidatorList> {
    if account_info.owner != &spl_stake_pool::ID {
        return Err(ProgramError::InvalidAccountOwner.into());
    }
    let data = account_info.try_borrow_data()?;
    Ok(spl_stake_pool::state::ValidatorList::deserialize(
        &mut data.as_ref(),
    )?)
}

#[derive(Clone)]
pub struct ValidatorList(spl_stake_pool::state::ValidatorList);

impl AsRef<spl_stake_pool::state::ValidatorList> for ValidatorList {
    fn as_ref(&self) -> &spl_stake_pool::state::ValidatorList {
        &self.0
    }
}

// This is necessary so we can use "anchor_spl::token::Mint::LEN"
// because rust does not resolve "anchor_spl::token::Mint::LEN" to
// "spl_token::state::Mint::LEN" automatically
impl ValidatorList {
    pub const LEN: usize = std::mem::size_of::<spl_stake_pool::state::ValidatorList>();
}

// You don't have to implement the "try_deserialize" function
// from this trait. It delegates to
// "try_deserialize_unchecked" by default which is what we want here
// because non-anchor accounts don't have a discriminator to check
impl anchor_lang::AccountDeserialize for ValidatorList {
    fn try_deserialize_unchecked(buf: &mut &[u8]) -> Result<Self> {
        Ok(Self(spl_stake_pool::state::ValidatorList::deserialize(
            buf,
        )?))
    }
}

// AccountSerialize defaults to a no-op which is what we want here
// because it's a foreign program, so our program does not
// have permission to write to the foreign program's accounts anyway
impl anchor_lang::AccountSerialize for ValidatorList {}

impl anchor_lang::Owner for ValidatorList {
    fn owner() -> Pubkey {
        spl_stake_pool::ID
    }
}

impl Deref for ValidatorList {
    type Target = spl_stake_pool::state::ValidatorList;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
