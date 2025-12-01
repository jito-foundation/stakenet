  ./epoch_schedule.sh
  ./target/debug/steward-cli --json-rpc-url http://localhost:8899 copy-directed-stake-targets --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --authority-keypair-path ~/.config/solana/id.json --vote-pubkey CcaHc2L43ZWjwCHART3oZoJvHLAe9hzT2DJNUpBzoTN1 --target-lamports 200000000000
  ./copy_directed_stake_target.sh

  # Time travel to 90% of current epoch
  EPOCH_INFO=$(curl -s http://localhost:8899 -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1,"method":"getEpochInfo"}')
  CURRENT_EPOCH=$(echo $EPOCH_INFO | jq '.result.epoch')
  SLOTS_IN_EPOCH=$(echo $EPOCH_INFO | jq '.result.slotsInEpoch')
  EPOCH_START_SLOT=$(( CURRENT_EPOCH * SLOTS_IN_EPOCH ))
  TARGET_SLOT=$(( EPOCH_START_SLOT + (SLOTS_IN_EPOCH * 90 / 100) ))
  echo "Time traveling to 90% of epoch $CURRENT_EPOCH (slot $TARGET_SLOT)"
  curl -s http://localhost:8899 -X POST -H "Content-Type: application/json" -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"surfnet_timeTravel\",\"params\":[{\"absoluteSlot\":$TARGET_SLOT}]}"

  ./target/debug/validator-history-cli --json-rpc-url http://localhost:8899 crank-copy-vote-account --keypair-path ~/.config/solana/id.json
  ./target/debug/validator-history-cli --json-rpc-url http://localhost:8899 crank-copy-cluster-info --keypair-path ~/.config/solana/id.json

  ./idle.sh
  ./compute_instant_unstake.sh
  ./rebalance.sh
  ./idle.sh

  # Time travel to next epoch
  NEXT_EPOCH=$(( CURRENT_EPOCH + 1 ))
  echo "Time traveling to epoch $NEXT_EPOCH"
  curl -s http://localhost:8899 -X POST -H "Content-Type: application/json" -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"surfnet_timeTravel\",\"params\":[{\"absoluteEpoch\":$NEXT_EPOCH}]}"

  ./update_stake_pool.sh
  ./epoch_maintenance.sh
  ./rebalance_directed.sh

  
  # For next ComputeScores commands, would need to crank validator history.


# Time travel to 90% of current epoch
EPOCH_INFO=$(curl -s http://localhost:8899 -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":1,"method":"getEpochInfo"}')
CURRENT_EPOCH=$(echo $EPOCH_INFO | jq '.result.epoch')
SLOTS_IN_EPOCH=$(echo $EPOCH_INFO | jq '.result.slotsInEpoch')
EPOCH_START_SLOT=$(( CURRENT_EPOCH * SLOTS_IN_EPOCH ))
TARGET_SLOT=$(( EPOCH_START_SLOT + (SLOTS_IN_EPOCH * 90 / 100) ))
echo "Time traveling to 90% of epoch $CURRENT_EPOCH (slot $TARGET_SLOT)"
curl -s http://localhost:8899 -X POST -H "Content-Type: application/json" -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"surfnet_timeTravel\",\"params\":[{\"absoluteSlot\":$TARGET_SLOT}]}"

./idle.sh
# Compute scores because it's epoch 889
./compute_scores.sh
./compute_delegations.sh
./idle.sh
./compute_instant_unstake.sh
./rebalance.sh
./idle.sh
