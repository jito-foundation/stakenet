use clap::ValueEnum;
use std::fmt::{Display, Formatter};

#[derive(ValueEnum, Debug, Clone, Copy)]
pub enum Cluster {
    Mainnet,
    Testnet,
    Localnet,
}

impl Display for Cluster {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Cluster::Mainnet => write!(f, "mainnet"),
            Cluster::Testnet => write!(f, "testnet"),
            Cluster::Localnet => write!(f, "localnet"),
        }
    }
}
