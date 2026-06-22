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

// Mirrors the agave 4.x PackedMinor wire format from solana-version/src/v4.rs.
// bits 14-15 of the packed u16 hold a prerelease tag (0=stable,1=rc,2=beta,3=alpha);
// bits 0-13 hold the actual minor version.  For prerelease builds the wire
// `patch` field carries the prerelease number; actual patch is always 0.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(transparent)]
struct PackedMinor(#[serde(with = "serde_varint")] u16);

impl PackedMinor {
    const PRERELEASE_BITS_OFFSET: u32 = 14;
    const PRERELEASE_MASK: u16 = 3; // 2-bit tag occupying bits 14-15

    // Returns (minor, patch) with prerelease encoding stripped.
    fn try_unpack(self, patch: u16) -> Option<(u16, u16)> {
        let Self(packed) = self;
        let shifted = packed >> Self::PRERELEASE_BITS_OFFSET;
        // Bits above the 2-bit tag are reserved and must be zero.
        if shifted & !Self::PRERELEASE_MASK != 0 {
            return None;
        }
        let prerelease_tag = shifted & Self::PRERELEASE_MASK;
        let minor = packed & !(Self::PRERELEASE_MASK << Self::PRERELEASE_BITS_OFFSET);
        // For any prerelease variant the wire `patch` is the prerelease number;
        // the real patch is 0.
        let patch = if prerelease_tag == 0 { patch } else { 0 };
        Some((minor, patch))
    }
}

// Internal struct that matches the on-the-wire layout used by agave 4.x.
#[derive(Deserialize, Serialize)]
struct SerializedVersion {
    #[serde(with = "serde_varint")]
    major: u16,

    #[serde(rename = "minor")]
    packed_minor: PackedMinor,

    #[serde(with = "serde_varint")]
    patch: u16,

    commit: u32,

    feature_set: u32,

    #[serde(with = "serde_varint")]
    client: u16,
}

#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub major: u16,

    pub minor: u16, // decoded actual minor

    pub patch: u16, // decoded actual patch (0 for any prerelease build)

    pub commit: u32,

    pub feature_set: u32,

    pub client: u16,
}

impl Serialize for Version {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        SerializedVersion {
            major: self.major,
            packed_minor: PackedMinor(self.minor),
            patch: self.patch,
            commit: self.commit,
            feature_set: self.feature_set,
            client: self.client,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Version {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let SerializedVersion {
            major,
            packed_minor,
            patch,
            commit,
            feature_set,
            client,
        } = SerializedVersion::deserialize(deserializer)?;

        let (minor, patch) = packed_minor
            .try_unpack(patch)
            .ok_or_else(|| serde::de::Error::custom("invalid PackedMinor: reserved bits set"))?;

        Ok(Version {
            major,
            minor,
            patch,
            commit,
            feature_set,
            client,
        })
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // PackedMinor::try_unpack
    // -----------------------------------------------------------------------

    #[test]
    fn test_packed_minor_stable() {
        // tag=0 → minor and patch pass through unchanged
        assert_eq!(PackedMinor(0x0000).try_unpack(0), Some((0, 0)));
        assert_eq!(PackedMinor(0x0000).try_unpack(5), Some((0, 5)));
        assert_eq!(PackedMinor(0x3FFF).try_unpack(100), Some((0x3FFF, 100)));
    }

    #[test]
    fn test_packed_minor_release_candidate() {
        // tag=1 (bits 14-15 = 0b01 → 0x4000) → actual patch = 0
        assert_eq!(PackedMinor(0x4000).try_unpack(1), Some((0, 0))); // minor=0, rc.1
        assert_eq!(PackedMinor(0x4001).try_unpack(2), Some((1, 0))); // minor=1, rc.2
        assert_eq!(PackedMinor(0x7FFF).try_unpack(42), Some((0x3FFF, 0)));
    }

    #[test]
    fn test_packed_minor_beta() {
        // tag=2 (bits 14-15 = 0b10 → 0x8000) → actual patch = 0
        assert_eq!(PackedMinor(0x8000).try_unpack(3), Some((0, 0)));
        assert_eq!(PackedMinor(0xBFFF).try_unpack(u16::MAX), Some((0x3FFF, 0)));
    }

    #[test]
    fn test_packed_minor_alpha() {
        // tag=3 (bits 14-15 = 0b11 → 0xC000) → actual patch = 0
        assert_eq!(PackedMinor(0xC000).try_unpack(7), Some((0, 0)));
        assert_eq!(PackedMinor(0xFFFF).try_unpack(u16::MAX), Some((0x3FFF, 0)));
    }

    // -----------------------------------------------------------------------
    // Version deserialization — wire bytes
    //
    // Wire layout (bincode / LEB128 varint):
    //   major       varint u16
    //   minor       varint u16  (PackedMinor: bits 14-15 = prerelease tag)
    //   patch       varint u16  (prerelease number for non-stable, real patch for stable)
    //   commit      u32 LE
    //   feature_set u32 LE
    //   client      varint u16
    // -----------------------------------------------------------------------

    fn zero_suffix() -> [u8; 9] {
        // commit(4) + feature_set(4) + client(1 varint zero) = 9 zero bytes
        [0u8; 9]
    }

    #[test]
    fn test_version_bytes_stable_zero() {
        // 0.0.0 stable → 12 zero bytes
        let bytes = [0u8; 12];
        let v: Version = bincode::deserialize(&bytes).unwrap();
        assert_eq!(v.major, 0);
        assert_eq!(v.minor, 0);
        assert_eq!(v.patch, 0);

        let roundtrip = bincode::serialize(&v).unwrap();
        assert_eq!(roundtrip, bytes);
    }

    #[test]
    fn test_version_bytes_4_0_0_rc_1() {
        // 4.0.0-rc.1 as broadcast by agave 4.x nodes:
        //   major       = 4        → [0x04]
        //   packed_minor= 16384    → LEB128 [0x80, 0x80, 0x01]  (0x4000, rc tag)
        //   patch       = 1        → [0x01]                      (rc number)
        //   commit/feat/client = 0 → [0x00 × 9]
        let bytes: &[u8] = &[
            0x04, // major = 4
            0x80, 0x80, 0x01, // packed_minor = 16384 (0x4000, rc tag in bits 14-15)
            0x01, // wire patch = 1 (rc.1 number)
            0x00, 0x00, 0x00, 0x00, // commit = 0
            0x00, 0x00, 0x00, 0x00, // feature_set = 0
            0x00, // client = 0
        ];
        let v: Version = bincode::deserialize(bytes).unwrap();

        // After PackedMinor decode: minor=0 (low 14 bits of 0x4000), patch=0 (prerelease)
        assert_eq!(v.major, 4);
        assert_eq!(v.minor, 0);
        assert_eq!(v.patch, 0);
    }

    #[test]
    fn test_version_bytes_4_0_1_stable() {
        // 4.0.1 stable (to contrast with 4.0.0-rc.1 above):
        //   major       = 4  → [0x04]
        //   packed_minor= 0  → [0x00]   (no prerelease tag)
        //   patch       = 1  → [0x01]   (real patch version)
        let mut bytes = vec![0x04u8, 0x00, 0x01];
        bytes.extend_from_slice(&zero_suffix());

        let v: Version = bincode::deserialize(&bytes).unwrap();
        assert_eq!(v.major, 4);
        assert_eq!(v.minor, 0);
        assert_eq!(v.patch, 1);
    }

    #[test]
    fn test_version_bytes_4_0_0_stable() {
        // 4.0.0 stable: same byte length as 4.0.1, just patch=0
        let mut bytes = vec![0x04u8, 0x00, 0x00];
        bytes.extend_from_slice(&zero_suffix());

        let v: Version = bincode::deserialize(&bytes).unwrap();
        assert_eq!(v.major, 4);
        assert_eq!(v.minor, 0);
        assert_eq!(v.patch, 0);
    }

    #[test]
    fn test_version_rc_vs_stable_different_byte_lengths() {
        // 4.0.0-rc.1 uses 14 bytes; 4.0.1 stable uses 12 bytes.
        // The old code (raw u16 for minor) would store both as version 4.0.1,
        // even though their wire representations are completely different.
        let rc_bytes: &[u8] = &[
            0x04, 0x80, 0x80, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];
        let stable_bytes: &[u8] = &[
            0x04, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];

        assert_eq!(rc_bytes.len(), 14);
        assert_eq!(stable_bytes.len(), 12);

        let rc: Version = bincode::deserialize(rc_bytes).unwrap();
        let stable: Version = bincode::deserialize(stable_bytes).unwrap();

        // rc.1 correctly decodes to 4.0.0, NOT 4.0.1
        assert_eq!((rc.major, rc.minor, rc.patch), (4, 0, 0));
        // stable 4.0.1 decodes to 4.0.1
        assert_eq!((stable.major, stable.minor, stable.patch), (4, 0, 1));

        assert_ne!(rc, stable);
    }
}
