use solana_sdk::{instruction::Instruction, pubkey::Pubkey};

pub trait CreateTransaction {
    fn create_transaction(&self) -> Vec<Instruction>;
}

pub trait UpdateInstruction {
    fn update_instruction(&self) -> Instruction;
}

pub trait Address {
    fn address(&self) -> Pubkey;
}
