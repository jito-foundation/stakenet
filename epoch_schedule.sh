#!/usr/bin/env bash
#
# Sync EpochSchedule sysvar from mainnet to local SurfPool
#
# This script fetches the actual EpochSchedule data from Solana mainnet
# and applies it to your local SurfPool instance.
#
# Usage:
#   ./scripts/sync_epoch_schedule_from_mainnet.sh [options]
#
# Options:
#   --mainnet-rpc <URL>   Mainnet RPC endpoint (default: https://api.mainnet-beta.solana.com)
#   --local-rpc <URL>     Local SurfPool RPC endpoint (default: http://127.0.0.1:8899)
#   --dry-run             Print the payload without sending
#

set -euo pipefail

# EpochSchedule sysvar address (well-known)
EPOCH_SCHEDULE_PUBKEY="SysvarEpochSchedu1e111111111111111111111111"

# Sysvar owner program
SYSVAR_OWNER="Sysvar1111111111111111111111111111111111111"

# Default RPC endpoints
MAINNET_RPC="https://api.mainnet-beta.solana.com"
LOCAL_RPC="http://127.0.0.1:8899"
DRY_RUN=false

# Parse command line arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --mainnet-rpc)
            MAINNET_RPC="$2"
            shift 2
            ;;
        --local-rpc)
            LOCAL_RPC="$2"
            shift 2
            ;;
        --dry-run)
            DRY_RUN=true
            shift
            ;;
        --help|-h)
            head -20 "$0" | tail -n +2 | sed 's/^#//'
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

echo "=== Syncing EpochSchedule from Mainnet to SurfPool ==="
echo "Mainnet RPC: $MAINNET_RPC"
echo "Local RPC: $LOCAL_RPC"
echo ""

# Step 1: Fetch EpochSchedule from mainnet
echo "Fetching EpochSchedule from mainnet..."

MAINNET_RESPONSE=$(curl -s -X POST "$MAINNET_RPC" \
    -H "Content-Type: application/json" \
    -d "{
      \"jsonrpc\": \"2.0\",
      \"id\": 1,
      \"method\": \"getAccountInfo\",
      \"params\": [
        \"$EPOCH_SCHEDULE_PUBKEY\",
        {\"encoding\": \"base64\"}
      ]
    }")

# Check if we got a valid response
if ! echo "$MAINNET_RESPONSE" | jq -e '.result.value' > /dev/null 2>&1; then
    echo "ERROR: Failed to fetch EpochSchedule from mainnet"
    echo "$MAINNET_RESPONSE" | jq .
    exit 1
fi

# Extract account data
DATA_BASE64=$(echo "$MAINNET_RESPONSE" | jq -r '.result.value.data[0]')
OWNER=$(echo "$MAINNET_RESPONSE" | jq -r '.result.value.owner')
LAMPORTS=$(echo "$MAINNET_RESPONSE" | jq -r '.result.value.lamports')
EXECUTABLE=$(echo "$MAINNET_RESPONSE" | jq -r '.result.value.executable')

echo "Mainnet EpochSchedule account:"
echo "  Owner: $OWNER"
echo "  Lamports: $LAMPORTS"
echo "  Executable: $EXECUTABLE"
echo "  Data (base64): $DATA_BASE64"
echo ""

# Convert base64 to hex for surfnet_setAccount
DATA_HEX=$(echo "$DATA_BASE64" | base64 -d | xxd -p | tr -d '\n')

echo "Data (hex): $DATA_HEX"
echo "Data length: $((${#DATA_HEX} / 2)) bytes"
echo ""

# Function to reverse hex bytes (little-endian to big-endian) - macOS compatible
reverse_hex_bytes() {
    local hex=$1
    local result=""
    local len=${#hex}
    for ((i=len-2; i>=0; i-=2)); do
        result+="${hex:$i:2}"
    done
    echo "$result"
}

# Function to convert little-endian hex to decimal
le_hex_to_dec() {
    local le_hex=$1
    local be_hex=$(reverse_hex_bytes "$le_hex")
    # Remove leading zeros and convert
    be_hex=$(echo "$be_hex" | sed 's/^0*//')
    if [[ -z "$be_hex" ]]; then
        echo "0"
    else
        printf '%d' "0x$be_hex" 2>/dev/null || echo "0"
    fi
}

# Decode and display the EpochSchedule values
echo "Decoded EpochSchedule values:"
DECODED_BYTES=$(echo "$DATA_BASE64" | base64 -d | xxd -p | tr -d '\n')

# Extract u64 values (little-endian)
# slots_per_epoch: bytes 0-7 (chars 1-16)
SLOTS_PER_EPOCH_HEX=$(echo "$DECODED_BYTES" | cut -c1-16)
SLOTS_PER_EPOCH=$(le_hex_to_dec "$SLOTS_PER_EPOCH_HEX")

# leader_schedule_slot_offset: bytes 8-15 (chars 17-32)
LEADER_OFFSET_HEX=$(echo "$DECODED_BYTES" | cut -c17-32)
LEADER_OFFSET=$(le_hex_to_dec "$LEADER_OFFSET_HEX")

# warmup: byte 16 (chars 33-34)
WARMUP_HEX=$(echo "$DECODED_BYTES" | cut -c33-34)
if [[ "$WARMUP_HEX" == "01" ]]; then
    WARMUP="true"
else
    WARMUP="false"
fi

# first_normal_epoch: bytes 17-24 (chars 35-50)
FIRST_EPOCH_HEX=$(echo "$DECODED_BYTES" | cut -c35-50)
FIRST_EPOCH=$(le_hex_to_dec "$FIRST_EPOCH_HEX")

# first_normal_slot: bytes 25-32 (chars 51-66)
FIRST_SLOT_HEX=$(echo "$DECODED_BYTES" | cut -c51-66)
FIRST_SLOT=$(le_hex_to_dec "$FIRST_SLOT_HEX")

echo "  slots_per_epoch: $SLOTS_PER_EPOCH"
echo "  leader_schedule_slot_offset: $LEADER_OFFSET"
echo "  warmup: $WARMUP"
echo "  first_normal_epoch: $FIRST_EPOCH"
echo "  first_normal_slot: $FIRST_SLOT"
echo ""

# Build the JSON-RPC payload for surfnet_setAccount
PAYLOAD=$(cat <<EOF
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "surfnet_setAccount",
  "params": [
    "$EPOCH_SCHEDULE_PUBKEY",
    {
      "data": "$DATA_HEX",
      "owner": "$OWNER",
      "lamports": $LAMPORTS,
      "executable": $EXECUTABLE
    }
  ]
}
EOF
)

if [[ "$DRY_RUN" == "true" ]]; then
    echo "=== Dry Run - Payload ==="
    echo "$PAYLOAD" | jq .
    echo ""
    echo "To execute, run without --dry-run"
    exit 0
fi

# Step 2: Apply to local SurfPool
echo "Applying EpochSchedule to local SurfPool..."

RESPONSE=$(curl -s -X POST "$LOCAL_RPC" \
    -H "Content-Type: application/json" \
    -d "$PAYLOAD")

echo "Response: $RESPONSE"

# Check for errors
if echo "$RESPONSE" | jq -e '.error' > /dev/null 2>&1; then
    echo ""
    echo "ERROR: Failed to update EpochSchedule on SurfPool"
    echo "$RESPONSE" | jq '.error'
    exit 1
fi

echo ""
echo "=== EpochSchedule Synced Successfully ==="
echo ""
echo "Mainnet EpochSchedule has been applied to your local SurfPool."
echo "Note: You may need to advance slots for changes to take effect."
