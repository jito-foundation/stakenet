#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Compare two generated docs runs and classify true drift.

Usage:
  scripts/audit_doc_drift.sh --old RUN_DIR --new RUN_DIR [--out FILE]

Options:
  --old RUN_DIR   Older run directory (.codex-out/docs/<id>)
  --new RUN_DIR   Newer run directory (.codex-out/docs/<id>)
  --out FILE      Output JSON path (default: <new>/99_drift_report.json)
  -h, --help      Show help
USAGE
}

OLD_RUN=""
NEW_RUN=""
OUT_FILE=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --old)
      OLD_RUN="$2"
      shift 2
      ;;
    --new)
      NEW_RUN="$2"
      shift 2
      ;;
    --out)
      OUT_FILE="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage
      exit 1
      ;;
  esac
done

if [[ -z "$OLD_RUN" || -z "$NEW_RUN" ]]; then
  usage
  exit 1
fi

if ! command -v codex >/dev/null 2>&1; then
  echo "codex CLI is required but not found in PATH" >&2
  exit 1
fi

if ! command -v jq >/dev/null 2>&1; then
  echo "jq is required but not found in PATH" >&2
  exit 1
fi

OLD_CLAIMS="${OLD_RUN}/96_verified_claims.json"
OLD_DOCS="${OLD_RUN}/97_rendered_docs.json"
NEW_CLAIMS="${NEW_RUN}/96_verified_claims.json"
NEW_DOCS="${NEW_RUN}/97_rendered_docs.json"

for f in "$OLD_CLAIMS" "$OLD_DOCS" "$NEW_CLAIMS" "$NEW_DOCS"; do
  if [[ ! -f "$f" ]]; then
    echo "Missing required run artifact: $f" >&2
    exit 1
  fi
done

if [[ -z "$OUT_FILE" ]]; then
  OUT_FILE="${NEW_RUN}/99_drift_report.json"
fi

mkdir -p "$(dirname "$OUT_FILE")"

TMP_PROMPT="$(mktemp)"
cleanup() {
  rm -f "$TMP_PROMPT"
}
trap cleanup EXIT

{
  cat .agents/prompts/doc_drift_auditor.md
  echo
  echo "Context:"
  echo "- Old claims: ${OLD_CLAIMS}"
  echo "- Old docs payload: ${OLD_DOCS}"
  echo "- New claims: ${NEW_CLAIMS}"
  echo "- New docs payload: ${NEW_DOCS}"
  echo "- Classify true drift conservatively and keep deterministic ordering."
} > "$TMP_PROMPT"

codex exec \
  --ephemeral \
  --sandbox workspace-write \
  --model gpt-5.4 \
  -c 'model_reasoning_effort="high"' \
  --output-schema .agents/schemas/drift_report.schema.json \
  --output-last-message "$OUT_FILE" \
  --json \
  - \
  < "$TMP_PROMPT" \
  > "${NEW_RUN}/logs/doc_drift_auditor.manual.jsonl"

jq empty "$OUT_FILE" >/dev/null

echo "Drift report written: $OUT_FILE"
