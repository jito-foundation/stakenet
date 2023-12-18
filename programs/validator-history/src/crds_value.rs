use {
    crate::serde_varint,
    anchor_lang::solana_program::{
        pubkey::Pubkey,
        sanitize::{Sanitize, SanitizeError},
        short_vec,
    },
    serde::{Deserialize, Deserializer, Serialize},
    static_assertions::const_assert_eq,
    std::net::{IpAddr, Ipv4Addr, SocketAddr},
    thiserror::Error,
};

/////// From solana/gossip/src/crds_value.rs

pub const MAX_WALLCLOCK: u64 = 1_000_000_000_000_000;

/// CrdsData that defines the different types of items CrdsValues can hold
/// * Merge Strategy - Latest wallclock is picked
/// * LowestSlot index is deprecated
#[allow(clippy::large_enum_variant)]
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum CrdsData {
    LegacyContactInfo(LegacyContactInfo),
    Vote,
    LowestSlot,
    LegacySnapshotHashes,
    AccountsHashes,
    EpochSlots,
    LegacyVersion(LegacyVersion),
    Version(Version2),
    NodeInstance,
    DuplicateShred,
    SnapshotHashes,
    ContactInfo(ContactInfo),
}

/// Copied from solana/version/src/lib.rs

#[derive(Debug, Eq, PartialEq)]
enum ClientId {
    SolanaLabs,
    JitoLabs,
    Firedancer,
    // If new variants are added, update From<u16> and TryFrom<ClientId>.
    Unknown(u16),
}

#[derive(Default, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Version {
    #[serde(with = "serde_varint")]
    pub major: u16,
    #[serde(with = "serde_varint")]
    pub minor: u16,
    #[serde(with = "serde_varint")]
    pub patch: u16,
    pub commit: u32,      // first 4 bytes of the sha1 commit hash
    pub feature_set: u32, // first 4 bytes of the FeatureSet identifier
    #[serde(with = "serde_varint")]
    pub client: u16,
}

impl Version {
    pub fn as_semver_version(&self) -> semver::Version {
        semver::Version::new(self.major as u64, self.minor as u64, self.patch as u64)
    }

    #[allow(dead_code)]
    fn client(&self) -> ClientId {
        ClientId::from(self.client)
    }
}

impl From<u16> for ClientId {
    fn from(client: u16) -> Self {
        match client {
            0u16 => Self::SolanaLabs,
            1u16 => Self::JitoLabs,
            2u16 => Self::Firedancer,
            _ => Self::Unknown(client),
        }
    }
}

//////// Copied from solana/gossip/src/contact_info.rs

const SOCKET_TAG_TVU_QUIC: u8 = 12;
const_assert_eq!(SOCKET_CACHE_SIZE, 13);
const SOCKET_CACHE_SIZE: usize = SOCKET_TAG_TVU_QUIC as usize + 1usize;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Duplicate IP address: {0}")]
    DuplicateIpAddr(IpAddr),
    #[error("Duplicate socket: {0}")]
    DuplicateSocket(/*key:*/ u8),
    #[error("Invalid IP address index: {index}, num addrs: {num_addrs}")]
    InvalidIpAddrIndex { index: u8, num_addrs: usize },
    #[error("Invalid port: {0}")]
    InvalidPort(/*port:*/ u16),
    #[error("Invalid {0:?} (udp) and {1:?} (quic) sockets")]
    InvalidQuicSocket(Option<SocketAddr>, Option<SocketAddr>),
    #[error("IP addresses saturated")]
    IpAddrsSaturated,
    #[error("Multicast IP address: {0}")]
    MulticastIpAddr(IpAddr),
    #[error("Port offsets overflow")]
    PortOffsetsOverflow,
    #[error("Socket not found: {0}")]
    SocketNotFound(/*key:*/ u8),
    #[error("Unspecified IP address: {0}")]
    UnspecifiedIpAddr(IpAddr),
    #[error("Unused IP address: {0}")]
    UnusedIpAddr(IpAddr),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ContactInfo {
    pubkey: Pubkey,
    #[serde(with = "serde_varint")]
    wallclock: u64,
    // When the node instance was first created.
    // Identifies duplicate running instances.
    outset: u64,
    shred_version: u16,
    pub version: Version,
    // All IP addresses are unique and referenced at least once in sockets.
    #[serde(with = "short_vec")]
    pub addrs: Vec<IpAddr>,
    // All sockets have a unique key and a valid IP address index.
    #[serde(with = "short_vec")]
    sockets: Vec<SocketEntry>,
    #[serde(skip_serializing)]
    cache: [SocketAddr; SOCKET_CACHE_SIZE],
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
struct SocketEntry {
    key: u8,   // Protocol identifier, e.g. tvu, tpu, etc
    index: u8, // IpAddr index in the accompanying addrs vector.
    #[serde(with = "serde_varint")]
    offset: u16, // Port offset with respect to the previous entry.
}

// As part of deserialization, self.addrs and self.sockets should be cross
// verified and self.cache needs to be populated. This type serves as a
// workaround since serde does not have an initializer.
// https://github.com/serde-rs/serde/issues/642
#[derive(Deserialize)]
struct ContactInfoLite {
    pubkey: Pubkey,
    #[serde(with = "serde_varint")]
    wallclock: u64,
    outset: u64,
    shred_version: u16,
    version: Version,
    #[serde(with = "short_vec")]
    addrs: Vec<IpAddr>,
    #[serde(with = "short_vec")]
    sockets: Vec<SocketEntry>,
}

impl ContactInfo {
    #[inline]
    pub fn pubkey(&self) -> &Pubkey {
        &self.pubkey
    }

    #[inline]
    pub fn wallclock(&self) -> u64 {
        self.wallclock
    }
}

impl<'de> Deserialize<'de> for ContactInfo {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let node = ContactInfoLite::deserialize(deserializer)?;
        ContactInfo::try_from(node).map_err(serde::de::Error::custom)
    }
}

impl TryFrom<ContactInfoLite> for ContactInfo {
    type Error = Error;

    fn try_from(node: ContactInfoLite) -> Result<Self, Self::Error> {
        let socket_addr_unspecified: SocketAddr =
            SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), /*port:*/ 0u16);
        let ContactInfoLite {
            pubkey,
            wallclock,
            outset,
            shred_version,
            version,
            addrs,
            sockets,
        } = node;
        let node = ContactInfo {
            pubkey,
            wallclock,
            outset,
            shred_version,
            version,
            addrs,
            sockets,
            cache: [socket_addr_unspecified; SOCKET_CACHE_SIZE],
        };

        Ok(node)
    }
}

///////// from legacy_contact_info.rs

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub struct LegacyContactInfo {
    id: Pubkey,
    /// gossip address
    pub gossip: SocketAddr,
    /// address to connect to for replication
    tvu: SocketAddr,
    /// address to forward shreds to
    tvu_forwards: SocketAddr,
    /// address to send repair responses to
    repair: SocketAddr,
    /// transactions address
    tpu: SocketAddr,
    /// address to forward unprocessed transactions to
    tpu_forwards: SocketAddr,
    /// address to which to send bank state requests
    tpu_vote: SocketAddr,
    /// address to which to send JSON-RPC requests
    rpc: SocketAddr,
    /// websocket for JSON-RPC push notifications
    rpc_pubsub: SocketAddr,
    /// address to send repair requests to
    serve_repair: SocketAddr,
    /// latest wallclock picked
    wallclock: u64,
    /// node shred version
    shred_version: u16,
}

impl Sanitize for LegacyContactInfo {
    fn sanitize(&self) -> std::result::Result<(), SanitizeError> {
        if self.wallclock >= MAX_WALLCLOCK {
            return Err(SanitizeError::ValueOutOfBounds);
        }
        Ok(())
    }
}
impl LegacyContactInfo {
    #[inline]
    pub fn pubkey(&self) -> &Pubkey {
        &self.id
    }

    #[inline]
    pub fn wallclock(&self) -> u64 {
        self.wallclock
    }
}

///////// from crds_value.rs

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct LegacyVersion {
    pub from: Pubkey,
    pub wallclock: u64,
    pub version: LegacyVersion1,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Version2 {
    pub from: Pubkey,
    pub wallclock: u64,
    pub version: LegacyVersion2,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct LegacyVersion1 {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
    pub commit: Option<u32>, // first 4 bytes of the sha1 commit hash
}

impl Sanitize for LegacyVersion1 {}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct LegacyVersion2 {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
    pub commit: Option<u32>, // first 4 bytes of the sha1 commit hash
    pub feature_set: u32,    // first 4 bytes of the FeatureSet identifier
}

impl From<LegacyVersion1> for LegacyVersion2 {
    fn from(legacy_version: LegacyVersion1) -> Self {
        Self {
            major: legacy_version.major,
            minor: legacy_version.minor,
            patch: legacy_version.patch,
            commit: legacy_version.commit,
            feature_set: 0,
        }
    }
}
