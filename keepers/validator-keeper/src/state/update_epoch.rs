use std::{error::Error, sync::Arc};

use solana_client::nonblocking::rpc_client::RpcClient;

use super::keeper_state::KeeperState;
use crate::operations::keeper_operations::KeeperOperations;

pub async fn update_epoch(
    client: &Arc<RpcClient>,
    loop_state: &mut KeeperState,
) -> Result<(), Box<dyn Error>> {
    let current_epoch = client.get_epoch_info().await?;

    if current_epoch.epoch != loop_state.epoch_info.epoch {
        loop_state.runs_for_epoch = [0; KeeperOperations::LEN];
        loop_state.errors_for_epoch = [0; KeeperOperations::LEN];
        loop_state.epoch_info = current_epoch.clone();
    }

    Ok(())
}
