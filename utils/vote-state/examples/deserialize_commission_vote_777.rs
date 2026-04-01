use std::str::FromStr;

use anchor_lang::prelude::{AccountInfo, Pubkey};
use anyhow::{Context, Result};
use serde_json::json;
use solana_rpc_client::rpc_client::RpcClient;
use solana_rpc_client_api::request::RpcRequest;
use solana_sdk::commitment_config::CommitmentConfig;
use validator_history_vote_state::VoteStateVersions;

const DEFAULT_RPC_URL: &str = "https://api.mainnet-beta.solana.com";
const VOTE_ACCOUNT: &str = "777VtXKGPmbpN2yGDAtHuAmDt2rQ7GKLnH6K8ViVv777";

fn main() -> Result<()> {
    let rpc_url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| DEFAULT_RPC_URL.to_string());
    let vote_account = Pubkey::from_str(VOTE_ACCOUNT)?;
    let rpc_client = RpcClient::new_with_commitment(rpc_url.clone(), CommitmentConfig::confirmed());

    let account = rpc_client
        .get_account(&vote_account)
        .with_context(|| format!("failed to fetch vote account {vote_account}"))?;

    let mut lamports = account.lamports;
    let mut data = account.data;
    let owner = account.owner;
    let account_info = AccountInfo::new(
        &vote_account,
        false,
        false,
        &mut lamports,
        data.as_mut_slice(),
        &owner,
        account.executable,
        account.rent_epoch,
    );

    let deserialize_commission = VoteStateVersions::deserialize_commission(&account_info)
        .context("deserialize_commission failed")?;

    let parsed_response: serde_json::Value = rpc_client.send(
        RpcRequest::GetAccountInfo,
        json!([
            vote_account.to_string(),
            {
                "encoding": "jsonParsed",
                "commitment": "confirmed"
            }
        ]),
    )?;

    if parsed_response.get("value").is_none() {
        anyhow::bail!("vote account {vote_account} not found in jsonParsed response");
    }

    let rpc_commission = parsed_response
        .get("value")
        .and_then(|value| value.get("data"))
        .and_then(|value| value.get("parsed"))
        .and_then(|value| value.get("info"))
        .and_then(|value| value.get("commission"))
        .and_then(|value| value.as_u64())
        .context("jsonParsed response is missing value.data.parsed.info.commission")?;

    println!("rpc_url: {rpc_url}");
    println!("vote_account: {vote_account}");
    println!("deserialize_commission(): {deserialize_commission}");
    println!("rpc jsonParsed info.commission: {rpc_commission}");

    assert_eq!(
        u64::from(deserialize_commission),
        rpc_commission,
        "deserialize_commission output mismatched RPC jsonParsed info.commission"
    );

    Ok(())
}
