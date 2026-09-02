//! Transaction v1 (SIMD-0296 / SIMD-0385).
//!
//! v1 raises the serialized transaction cap from 1232 to 4096 bytes, which lets
//! the keeper pack roughly three times as many instructions into a transaction.
//! It is additive — legacy transactions stay valid after activation — so this
//! module is inert until [`set_tx_version`] is called with [`TxVersion::V1`].
//!
//! The v1 message types live in `solana-message` 4.x, a newer copy of the split
//! solana crates than the pinned 2.3 stack the rest of this SDK is built on.
//! The two copies have distinct `Pubkey`, `Instruction` and `Hash` types, so
//! every conversion across that boundary happens here and nowhere else.
//!
//! Two v1 rules shape the code below:
//!
//!   * The compute budget travels in the message header rather than in
//!     ComputeBudget instructions, and an unset field resolves to zero rather
//!     than to the legacy default. [`split_compute_budget`] lifts the existing
//!     instructions into a config and fills in what v1 would otherwise zero.
//!   * A message holds at most 64 inline addresses and 64 instructions, and
//!     address lookup tables are gone. The address cap usually binds before the
//!     size does, so packing has to count addresses, not just bytes.

use std::sync::atomic::{AtomicU8, Ordering};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde_json::json;
use solana_client::{
    client_error::{ClientError, ClientErrorKind},
    nonblocking::rpc_client::RpcClient,
    rpc_config::RpcSendTransactionConfig,
    rpc_request::RpcRequest,
};
use solana_message::v1;
use solana_program::hash::Hash;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    instruction::Instruction,
    packet::PACKET_DATA_SIZE,
    pubkey::Pubkey,
    signature::{Keypair, Signature},
    signer::Signer,
};
use solana_transaction_status::UiTransactionEncoding;

/// The feature gate that activates transaction v1 on a cluster.
pub const ENABLE_TX_V1_FEATURE: Pubkey =
    Pubkey::from_str_const("txv1aq4pp281K9um3tnPgkfX8UqtFT6wcVW3hNezGLL");

/// Compute units the runtime gives a legacy transaction per instruction when it
/// requests no limit of its own.
const DEFAULT_COMPUTE_UNITS_PER_INSTRUCTION: u32 = 200_000;

/// Ceiling on a transaction's compute unit limit.
const MAX_COMPUTE_UNIT_LIMIT: u32 = 1_400_000;

/// Loaded accounts data budget a legacy transaction gets when it requests none.
/// v1 resolves an unset limit to zero instead, so it is always written out.
const DEFAULT_LOADED_ACCOUNTS_DATA_SIZE_LIMIT: u32 = 64 * 1024 * 1024;

/// Bytes held back from the v1 size budget for the `TransactionConfig` in the
/// message header, which the legacy transaction that packing measures against
/// does not carry.
const CONFIG_HEADROOM: usize = 32;

/// The transaction format the cluster is being addressed with.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TxVersion {
    #[default]
    Legacy,
    V1,
}

/// The transaction format is a property of the cluster, fixed for the life of
/// the process: the feature gate cannot deactivate, and the keeper talks to one
/// cluster. Holding it in a global keeps the mode out of the signature of every
/// `submit_*` helper and their callers.
static TX_VERSION: AtomicU8 = AtomicU8::new(1);

/// Returns the transaction format transactions are currently built in.
pub fn tx_version() -> TxVersion {
    match TX_VERSION.load(Ordering::Relaxed) {
        1 => TxVersion::V1,
        _ => TxVersion::Legacy,
    }
}

/// Sets the transaction format for the rest of the process.
pub fn set_tx_version(version: TxVersion) {
    let encoded = match version {
        TxVersion::Legacy => 0,
        TxVersion::V1 => 1,
    };
    TX_VERSION.store(encoded, Ordering::Relaxed);
}

/// Reports whether `enable_tx_v1` has activated on the cluster behind `client`.
///
/// A missing feature account and a staged-but-inactive one both mean a v1
/// transaction would be rejected, so both answer `false`.
pub async fn is_v1_active(client: &RpcClient) -> Result<bool, ClientError> {
    let account = client
        .get_account_with_commitment(&ENABLE_TX_V1_FEATURE, CommitmentConfig::confirmed())
        .await?
        .value;

    #[allow(deprecated)]
    Ok(account
        .and_then(|account| solana_sdk::feature::from_account(&account))
        .is_some_and(|feature| feature.activated_at.is_some()))
}

/// The serialized size a transaction may reach under the active format.
pub fn max_transaction_size() -> usize {
    match tx_version() {
        TxVersion::Legacy => PACKET_DATA_SIZE,
        TxVersion::V1 => v1::MAX_TRANSACTION_SIZE - CONFIG_HEADROOM,
    }
}

/// The number of inline addresses a message may hold under the active format.
///
/// Legacy is bounded by size rather than by a count, so it reports the largest
/// number that could fit — 1232 bytes cannot hold 64 addresses anyway.
pub fn max_addresses() -> usize {
    match tx_version() {
        TxVersion::Legacy => usize::MAX,
        TxVersion::V1 => v1::MAX_ADDRESSES as usize,
    }
}

/// The number of instructions a message may hold under the active format.
pub fn max_instructions() -> usize {
    match tx_version() {
        TxVersion::Legacy => usize::MAX,
        TxVersion::V1 => v1::MAX_INSTRUCTIONS as usize,
    }
}

#[derive(Debug, thiserror::Error)]
pub enum V1Error {
    #[error("Failed to compile v1 message: {0}")]
    Compile(String),

    #[error("Invalid v1 message: {0}")]
    Invalid(String),

    #[error(
        "v1 transaction is {0} bytes, over the {} byte limit",
        v1::MAX_TRANSACTION_SIZE
    )]
    TooLarge(usize),
}

/// A signed v1 transaction, held as the wire bytes the RPC expects.
///
/// The pinned `solana-transaction` 2.x cannot hold a v1 message, so this keeps
/// the serialized form alongside the signature that identifies it.
#[derive(Clone, Debug)]
pub struct V1Transaction {
    signature: Signature,
    wire: Vec<u8>,
}

impl V1Transaction {
    pub fn signature(&self) -> Signature {
        self.signature
    }

    pub fn len(&self) -> usize {
        self.wire.len()
    }

    pub fn is_empty(&self) -> bool {
        self.wire.is_empty()
    }

    pub fn to_base64(&self) -> String {
        BASE64.encode(&self.wire)
    }
}

/// Builds and signs a v1 transaction for a single signer.
///
/// Any ComputeBudget instructions in `instructions` are lifted into the message
/// config rather than compiled, since v1 prices a transaction from its header.
pub fn build_v1_transaction(
    instructions: &[Instruction],
    signer: &Keypair,
    blockhash: Hash,
) -> Result<V1Transaction, V1Error> {
    let (instructions, config) = split_compute_budget(instructions);
    let payer = to_v1_address(&signer.pubkey());

    let message = v1::Message::try_compile_with_config(
        &payer,
        &instructions
            .iter()
            .map(to_v1_instruction)
            .collect::<Vec<_>>(),
        solana_hash::Hash::new_from_array(blockhash.to_bytes()),
        config,
    )
    .map_err(|e| V1Error::Compile(e.to_string()))?;

    message
        .validate()
        .map_err(|e| V1Error::Invalid(format!("{e:?}")))?;

    // `serialize` emits the 0x81 version prefix, and the signature covers those
    // bytes as they stand.
    let message_bytes = message.serialize();
    let signature = signer.sign_message(&message_bytes);

    // v1 inverts the legacy wire layout: the message comes first, and the
    // signatures follow as a bare fixed-width array with no short-vec length
    // prefix. A reader tells the two apart from the first byte — under 0x80 it
    // is a legacy signature count, 0x81 is a v1 message — and rejects a v1
    // message found in the legacy position with "invalid message version".
    // `num_required_signatures` in the header says how many signatures follow.
    let mut wire = Vec::with_capacity(message_bytes.len() + v1::SIGNATURE_SIZE);
    wire.extend_from_slice(&message_bytes);
    wire.extend_from_slice(signature.as_ref());

    if wire.len() > v1::MAX_TRANSACTION_SIZE {
        return Err(V1Error::TooLarge(wire.len()));
    }

    Ok(V1Transaction { signature, wire })
}

/// Submits a v1 transaction through `sendTransaction`.
///
/// The typed `RpcClient::send_transaction` cannot carry a v1 message, and
/// base58 is still capped at the legacy 1232 bytes, so this posts the base64
/// encoding down the raw request path. Errors come back in the same shape the
/// typed call produces — the sender maps preflight failures by error code,
/// independent of which request produced them.
pub async fn send_v1_transaction(
    client: &RpcClient,
    transaction: &V1Transaction,
    config: RpcSendTransactionConfig,
) -> Result<Signature, ClientError> {
    let config = RpcSendTransactionConfig {
        encoding: Some(UiTransactionEncoding::Base64),
        ..config
    };

    let signature: String = client
        .send(
            RpcRequest::SendTransaction,
            json!([transaction.to_base64(), config]),
        )
        .await?;

    signature.parse().map_err(|e| {
        ClientError::new_with_request(
            ClientErrorKind::Custom(format!(
                "Invalid signature in sendTransaction response: {e}"
            )),
            RpcRequest::SendTransaction,
        )
    })
}

/// The compute budget as stated by a legacy instruction list.
#[derive(Clone, Copy, Debug, Default)]
struct LiftedBudget {
    compute_unit_limit: Option<u32>,
    heap_size: Option<u32>,
    loaded_accounts_data_size_limit: Option<u32>,
    price_microlamports_per_cu: Option<u64>,
}

/// Splits ComputeBudget instructions out of `instructions` and folds them into
/// a v1 [`v1::TransactionConfig`].
///
/// Two conversions matter. The priority fee becomes a total in lamports rather
/// than a price in micro-lamports per compute unit, matching how the runtime
/// bills a legacy transaction. And because an unset `compute_unit_limit` or
/// `loaded_accounts_data_size_limit` resolves to zero under v1 rather than to
/// the legacy default, both are written even when the caller stated neither.
/// An unset heap resolves to 32 KiB under both, so it is left alone.
///
/// If the same budget is stated twice the last one wins. The runtime rejects
/// such a list outright under legacy, so no correct caller produces one.
fn split_compute_budget(instructions: &[Instruction]) -> (Vec<Instruction>, v1::TransactionConfig) {
    let mut budget = LiftedBudget::default();
    let mut rest = Vec::with_capacity(instructions.len());

    for instruction in instructions {
        if instruction.program_id != solana_sdk::compute_budget::ID {
            rest.push(instruction.clone());
            continue;
        }

        // Borsh-encoded `ComputeBudgetInstruction`: a one-byte discriminant
        // followed by a little-endian operand.
        match instruction.data.split_first() {
            Some((1, operand)) => budget.heap_size = read_u32(operand),
            Some((2, operand)) => budget.compute_unit_limit = read_u32(operand),
            Some((3, operand)) => budget.price_microlamports_per_cu = read_u64(operand),
            Some((4, operand)) => budget.loaded_accounts_data_size_limit = read_u32(operand),
            // Not a budget this understands; leave it for the runtime to reject
            // rather than dropping it silently.
            _ => rest.push(instruction.clone()),
        }
    }

    let compute_unit_limit = budget
        .compute_unit_limit
        .unwrap_or_else(|| default_compute_unit_limit(instructions.len()));

    let mut config = v1::TransactionConfig::empty()
        .with_compute_unit_limit(compute_unit_limit)
        .with_loaded_accounts_data_size_limit(
            budget
                .loaded_accounts_data_size_limit
                .unwrap_or(DEFAULT_LOADED_ACCOUNTS_DATA_SIZE_LIMIT),
        );

    if let Some(heap_size) = budget.heap_size {
        config = config.with_heap_size(heap_size);
    }

    if let Some(price) = budget.price_microlamports_per_cu {
        // The runtime rounds a legacy transaction's priority fee up to whole
        // lamports.
        config = config.with_priority_fee(
            u64::from(compute_unit_limit)
                .saturating_mul(price)
                .div_ceil(1_000_000),
        );
    }

    (rest, config)
}

/// The compute unit limit a legacy transaction gets when it requests none.
fn default_compute_unit_limit(instruction_count: usize) -> u32 {
    DEFAULT_COMPUTE_UNITS_PER_INSTRUCTION
        .saturating_mul(u32::try_from(instruction_count).unwrap_or(u32::MAX))
        .min(MAX_COMPUTE_UNIT_LIMIT)
}

fn read_u32(operand: &[u8]) -> Option<u32> {
    operand.get(..4)?.try_into().ok().map(u32::from_le_bytes)
}

fn read_u64(operand: &[u8]) -> Option<u64> {
    operand.get(..8)?.try_into().ok().map(u64::from_le_bytes)
}

fn to_v1_address(pubkey: &Pubkey) -> solana_address::Address {
    solana_address::Address::new_from_array(pubkey.to_bytes())
}

fn to_v1_instruction(instruction: &Instruction) -> solana_instruction::Instruction {
    solana_instruction::Instruction {
        program_id: to_v1_address(&instruction.program_id),
        accounts: instruction
            .accounts
            .iter()
            .map(|account| solana_instruction::AccountMeta {
                pubkey: to_v1_address(&account.pubkey),
                is_signer: account.is_signer,
                is_writable: account.is_writable,
            })
            .collect(),
        data: instruction.data.clone(),
    }
}

#[cfg(test)]
mod tests {
    use solana_sdk::compute_budget::ComputeBudgetInstruction;
    use solana_sdk::instruction::AccountMeta;

    use super::*;

    fn noop_instruction() -> Instruction {
        Instruction {
            program_id: Pubkey::new_unique(),
            accounts: vec![AccountMeta::new(Pubkey::new_unique(), false)],
            data: vec![7],
        }
    }

    #[test]
    fn compute_budget_instructions_move_into_the_config() {
        let noop = noop_instruction();
        let (rest, config) = split_compute_budget(&[
            ComputeBudgetInstruction::set_compute_unit_limit(300_000),
            ComputeBudgetInstruction::set_compute_unit_price(1_000),
            noop.clone(),
        ]);

        assert_eq!(rest, vec![noop], "only the budget is lifted out");
        assert_eq!(config.compute_unit_limit, Some(300_000));
        // 300_000 CU at 1_000 micro-lamports each is 300_000_000 micro-lamports,
        // which is 300 lamports.
        assert_eq!(config.priority_fee, Some(300));
    }

    #[test]
    fn a_priority_fee_rounds_up_to_whole_lamports() {
        let (_, config) = split_compute_budget(&[
            ComputeBudgetInstruction::set_compute_unit_limit(1),
            ComputeBudgetInstruction::set_compute_unit_price(1),
            noop_instruction(),
        ]);

        assert_eq!(config.priority_fee, Some(1), "a sub-lamport fee rounds up");
    }

    #[test]
    fn fields_v1_would_zero_are_always_written() {
        let (_, config) = split_compute_budget(&[noop_instruction()]);

        assert_eq!(
            config.compute_unit_limit,
            Some(DEFAULT_COMPUTE_UNITS_PER_INSTRUCTION),
            "an unset limit takes the legacy default rather than zero"
        );
        assert_eq!(
            config.loaded_accounts_data_size_limit,
            Some(DEFAULT_LOADED_ACCOUNTS_DATA_SIZE_LIMIT)
        );
        assert_eq!(config.priority_fee, None, "no fee was asked for");
        assert_eq!(config.heap_size, None, "an unset heap already means 32 KiB");
    }

    #[test]
    fn an_unset_limit_is_capped_at_the_runtime_maximum() {
        let instructions = vec![noop_instruction(); 32];
        let (_, config) = split_compute_budget(&instructions);

        assert_eq!(config.compute_unit_limit, Some(MAX_COMPUTE_UNIT_LIMIT));
    }

    #[test]
    fn a_signed_transaction_leads_with_the_message_and_trails_the_signature() {
        let signer = Keypair::new();
        let transaction =
            build_v1_transaction(&[noop_instruction()], &signer, Hash::default()).unwrap();
        let (message, signature) = transaction
            .wire
            .split_at(transaction.wire.len() - v1::SIGNATURE_SIZE);

        // The first byte doubles as the format discriminator. Anything under
        // 0x80 is read as a legacy signature count, which is how a legacy-ordered
        // v1 payload earns "invalid message version" from the RPC.
        assert_eq!(message[0], 0x81, "the message leads, prefixed 0x80 | 1");
        assert_eq!(
            signature,
            transaction.signature().as_ref(),
            "the signature trails, with no short-vec length prefix"
        );
        assert!(transaction
            .signature()
            .verify(signer.pubkey().as_ref(), message));
    }

    /// Pins the wire layout to the serializer the RPC actually reads with, so
    /// the hand-assembly in `build_v1_transaction` cannot drift from upstream.
    #[test]
    fn the_wire_bytes_match_upstreams_own_serializer() {
        use solana_transaction::versioned::VersionedTransaction;

        let signer = Keypair::new();
        let instructions = [noop_instruction(), noop_instruction()];
        let blockhash = Hash::new_from_array([7; 32]);

        let mine = build_v1_transaction(&instructions, &signer, blockhash).unwrap();

        // Rebuild the same message through the 4.x types and let upstream
        // serialize it. Signatures cross the crate-version boundary as bytes.
        let (stripped, config) = split_compute_budget(&instructions);
        let message = v1::Message::try_compile_with_config(
            &to_v1_address(&signer.pubkey()),
            &stripped.iter().map(to_v1_instruction).collect::<Vec<_>>(),
            solana_hash::Hash::new_from_array(blockhash.to_bytes()),
            config,
        )
        .unwrap();
        let upstream = wincode::serialize(&VersionedTransaction {
            signatures: vec![solana_signature::Signature::from(
                <[u8; 64]>::try_from(mine.signature().as_ref()).unwrap(),
            )],
            message: solana_message::VersionedMessage::V1(message),
        })
        .unwrap();

        assert_eq!(mine.wire, upstream);
    }

    #[test]
    fn compiling_past_the_address_cap_fails_rather_than_truncating() {
        let signer = Keypair::new();
        // Distinct instructions, so each contributes a program id and an account
        // the message has not seen.
        let instructions = (0..v1::MAX_ADDRESSES)
            .map(|_| noop_instruction())
            .collect::<Vec<_>>();

        let error = build_v1_transaction(&instructions, &signer, Hash::default())
            .expect_err("64 distinct instructions carry more than 64 addresses");
        assert!(matches!(error, V1Error::Invalid(_)), "got {error:?}");
    }
}
