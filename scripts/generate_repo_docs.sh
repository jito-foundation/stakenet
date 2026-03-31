#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Generate deterministic repository docs with Codex worker agents.

Usage:
  scripts/generate_repo_docs.sh [--run-id ID] [--compare-to RUN_DIR] [--skip-tests] [--skip-drift-audit]

Options:
  --run-id ID           Explicit run id. Default: UTC timestamp.
  --compare-to RUN_DIR  Previous run dir to classify drift against.
  --skip-tests          Skip tests mapper stage.
  --skip-drift-audit    Skip optional drift audit even when --compare-to is set.
  -h, --help            Show this help.

Outputs:
  .codex-out/docs/<RUN_ID>/
  docs/autogen/*.md
USAGE
}

RUN_ID="$(date -u +%Y%m%d_%H%M%S)"
COMPARE_TO=""
SKIP_TESTS="false"
SKIP_DRIFT_AUDIT="false"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --run-id)
      RUN_ID="$2"
      shift 2
      ;;
    --compare-to)
      COMPARE_TO="$2"
      shift 2
      ;;
    --skip-tests)
      SKIP_TESTS="true"
      shift
      ;;
    --skip-drift-audit)
      SKIP_DRIFT_AUDIT="true"
      shift
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

if ! command -v codex >/dev/null 2>&1; then
  echo "codex CLI is required but not found in PATH" >&2
  exit 1
fi

if ! command -v jq >/dev/null 2>&1; then
  echo "jq is required but not found in PATH" >&2
  exit 1
fi

ROOT_DIR="$(pwd)"
RUN_DIR=".codex-out/docs/${RUN_ID}"
LOG_DIR="${RUN_DIR}/logs"
INDEX_DIR="${RUN_DIR}/index"
WORKER_DIR="${RUN_DIR}/workers"
TMP_DIR="${RUN_DIR}/tmp"
CODEX_TIMEOUT_SECONDS="${CODEX_TIMEOUT_SECONDS:-300}"
CODEX_TIMEOUT_KILL_AFTER_SECONDS="${CODEX_TIMEOUT_KILL_AFTER_SECONDS:-20}"
TOOLS_BIN="${TMP_DIR}/bin"

mkdir -p "$RUN_DIR" "$LOG_DIR" "$INDEX_DIR" "$WORKER_DIR" "$TMP_DIR"
mkdir -p "$TOOLS_BIN"

if command -v python3 >/dev/null 2>&1; then
  ln -sf "$(command -v python3)" "${TOOLS_BIN}/python"
fi

GIT_COMMIT="$(git rev-parse HEAD 2>/dev/null || echo UNKNOWN)"

cat > "${RUN_DIR}/run_metadata.json" <<META
{
  "run_id": "${RUN_ID}",
  "git_commit": "${GIT_COMMIT}",
  "repo_root": "${ROOT_DIR}",
  "compare_to": "${COMPARE_TO}",
  "skip_tests": ${SKIP_TESTS},
  "skip_drift_audit": ${SKIP_DRIFT_AUDIT}
}
META

# Deterministic pre-index artifacts (non-LLM).
LC_ALL=C git ls-files | sort > "${INDEX_DIR}/git_files.txt"
LC_ALL=C rg --files programs | sort > "${INDEX_DIR}/program_files.txt"
LC_ALL=C rg --files keepers | sort > "${INDEX_DIR}/keeper_files.txt"
LC_ALL=C rg --files utils | sort > "${INDEX_DIR}/cli_files.txt"
LC_ALL=C rg --files sdk | sort > "${INDEX_DIR}/sdk_files.txt"
LC_ALL=C rg --files tests | sort > "${INDEX_DIR}/test_files.txt"
LC_ALL=C rg --files docs | sort > "${INDEX_DIR}/docs_files.txt" || true
LC_ALL=C rg '^pub mod ' programs/validator-history/src/instructions/mod.rs | sed 's/^pub mod //; s/;$//' | sort > "${INDEX_DIR}/validator_history_instruction_modules.txt"
LC_ALL=C rg '^pub mod ' programs/steward/src/instructions/mod.rs | sed 's/^pub mod //; s/;$//' | sort > "${INDEX_DIR}/steward_instruction_modules.txt"

run_codex_json() {
  local name="$1"
  local prompt_file="$2"
  local schema_file="$3"
  local out_file="$4"
  local model="$5"
  local reasoning_effort="$6"
  local extra_context="$7"
  local step_timeout="${8:-$CODEX_TIMEOUT_SECONDS}"

  local composed_prompt="${TMP_DIR}/${name}.prompt.md"
  {
    cat "$prompt_file"
    echo
    echo "Context:"
    echo "- Repository root: ${ROOT_DIR}"
    echo "- Run directory: ${RUN_DIR}"
    echo "- Git commit: ${GIT_COMMIT}"
    echo "- Index files are in: ${INDEX_DIR}"
    echo "- Output must be strict JSON matching schema."
    echo
    echo "Additional task context:"
    echo "$extra_context"
  } > "$composed_prompt"

  local status=0
  env PATH="${TOOLS_BIN}:${PATH}" timeout -k "${CODEX_TIMEOUT_KILL_AFTER_SECONDS}" "${step_timeout}" codex exec \
    --ephemeral \
    --sandbox workspace-write \
    --model "$model" \
    -c "model_reasoning_effort=\"${reasoning_effort}\"" \
    --output-schema "$schema_file" \
    --output-last-message "$out_file" \
    --json \
    - \
    < "$composed_prompt" \
    > "${LOG_DIR}/${name}.jsonl" || status=$?

  if [[ "$status" -ne 0 ]]; then
    if [[ "$status" -eq 124 ]]; then
      echo "Step '${name}' timed out after ${step_timeout}s (kill-after ${CODEX_TIMEOUT_KILL_AFTER_SECONDS}s)." >&2
      echo "Inspect log: ${LOG_DIR}/${name}.jsonl" >&2
    else
      echo "Step '${name}' failed with exit code ${status}. Inspect log: ${LOG_DIR}/${name}.jsonl" >&2
    fi
    return "$status"
  fi

  jq empty "$out_file" >/dev/null
}

echo "[1/8] Running repo indexer"
run_codex_json \
  "repo_indexer" \
  ".agents/prompts/repo_indexer.md" \
  ".agents/schemas/repo_index.schema.json" \
  "${RUN_DIR}/00_repo_index.json" \
  "gpt-5.4-mini" \
  "low" \
  "Use Cargo.toml workspace metadata and index files to produce the repository map." \
  "180"

echo "[2/8] Running mapper workers (sequential for reliability)"
run_codex_json \
  "validator_history_mapper" \
  ".agents/prompts/validator_history_mapper.md" \
  ".agents/schemas/claim_bundle.schema.json" \
  "${WORKER_DIR}/10_validator_history.json" \
  "gpt-5.4-mini" \
  "low" \
  "Primary scope: programs/validator-history. Secondary integration scope: keepers/stakenet-keeper operations that call validator-history instructions." \
  "300"

run_codex_json \
  "steward_mapper" \
  ".agents/prompts/steward_mapper.md" \
  ".agents/schemas/claim_bundle.schema.json" \
  "${WORKER_DIR}/20_steward.json" \
  "gpt-5.4-mini" \
  "low" \
  "Primary scope: programs/steward. Secondary integration scope: keeper/steward cranking and CLI actions/cranks that invoke steward." \
  "300"

run_codex_json \
  "keeper_mapper" \
  ".agents/prompts/keeper_mapper.md" \
  ".agents/schemas/claim_bundle.schema.json" \
  "${WORKER_DIR}/30_keeper.json" \
  "gpt-5.4-mini" \
  "low" \
  "Primary scope: keepers/stakenet-keeper. Capture fetch-fire-emit and config/flag semantics from code." \
  "300"

run_codex_json \
  "cli_surface_mapper" \
  ".agents/prompts/cli_surface_mapper.md" \
  ".agents/schemas/claim_bundle.schema.json" \
  "${WORKER_DIR}/40_cli_surface.json" \
  "gpt-5.4-mini" \
  "low" \
  "Primary scope: utils/steward-cli and utils/validator-history-cli. Produce a high-value operator-focused CLI reference claim bundle." \
  "300"

if [[ "$SKIP_TESTS" == "false" ]]; then
  run_codex_json \
    "tests_mapper" \
    ".agents/prompts/tests_mapper.md" \
    ".agents/schemas/claim_bundle.schema.json" \
    "${WORKER_DIR}/50_tests.json" \
    "gpt-5.4-mini" \
    "low" \
    "Primary scope: tests crate. Map major covered behavior and obvious gaps with conservative confidence." \
    "300"
fi

echo "[3/8] Combining claim artifacts"
LC_ALL=C ls "${WORKER_DIR}"/*.json | sort > "${RUN_DIR}/worker_bundle_files.txt"
jq -s 'sort_by(.component)' "${WORKER_DIR}"/*.json > "${RUN_DIR}/90_claim_bundles.json"
jq -s '[ .[] | .claims[] ] | sort_by(.claim_id)' "${WORKER_DIR}"/*.json > "${RUN_DIR}/90_claims_all.json"

echo "[4/8] Running claim verifier"
run_codex_json \
  "claim_verifier" \
  ".agents/prompts/claim_verifier.md" \
  ".agents/schemas/verification_report.schema.json" \
  "${RUN_DIR}/95_verification_report.json" \
  "gpt-5.4-mini" \
  "low" \
  "Use ${RUN_DIR}/90_claim_bundles.json and ${RUN_DIR}/90_claims_all.json as primary verification input." \
  "240"

echo "[5/8] Filtering unsupported/conflicting claims"
jq -n \
  --slurpfile claims "${RUN_DIR}/90_claims_all.json" \
  --slurpfile report "${RUN_DIR}/95_verification_report.json" '
    ($report[0].results
      | map(select(.status == "unsupported" or .status == "conflicting") | .claim_id)
    ) as $blocked
    | $claims[0]
    | map(select(.claim_id as $id | ($blocked | index($id) | not)))
    | sort_by(.claim_id)
  ' > "${RUN_DIR}/96_verified_claims.json"

echo "[6/8] Running docs renderer"
run_codex_json \
  "docs_renderer" \
  ".agents/prompts/docs_renderer.md" \
  ".agents/schemas/rendered_docs.schema.json" \
  "${RUN_DIR}/97_rendered_docs.json" \
  "gpt-5.4-mini" \
  "low" \
  "Primary inputs: ${RUN_DIR}/96_verified_claims.json and ${RUN_DIR}/95_verification_report.json." \
  "240"

echo "[7/8] Writing docs payload to docs/autogen"
mkdir -p docs/autogen
jq -c '.documents[]' "${RUN_DIR}/97_rendered_docs.json" | while IFS= read -r doc; do
  rel_path="$(echo "$doc" | jq -r '.path')"
  markdown="$(echo "$doc" | jq -r '.markdown')"

  if [[ "$rel_path" == /* ]]; then
    echo "Refusing absolute path in rendered docs payload: $rel_path" >&2
    exit 1
  fi

  if [[ "$rel_path" != docs/autogen/* ]]; then
    echo "Refusing out-of-scope rendered path: $rel_path" >&2
    exit 1
  fi

  mkdir -p "$(dirname "$rel_path")"
  printf '%s\n' "$markdown" > "$rel_path"
done

cp "${RUN_DIR}/97_rendered_docs.json" "docs/autogen/_rendered_docs_payload.json"
cp "${RUN_DIR}/95_verification_report.json" "docs/autogen/_verification_report.json"

echo "[8/8] Optional drift audit"
if [[ "$SKIP_DRIFT_AUDIT" == "true" ]]; then
  echo "Skipping drift audit (--skip-drift-audit)."
elif [[ -n "$COMPARE_TO" ]]; then
  OLD_CLAIMS="${COMPARE_TO}/96_verified_claims.json"
  OLD_DOCS="${COMPARE_TO}/97_rendered_docs.json"

  if [[ ! -f "$OLD_CLAIMS" || ! -f "$OLD_DOCS" ]]; then
    echo "compare-to run is missing required files: $OLD_CLAIMS or $OLD_DOCS" >&2
    exit 1
  fi

  if run_codex_json \
    "doc_drift_auditor" \
    ".agents/prompts/doc_drift_auditor.md" \
    ".agents/schemas/drift_report.schema.json" \
    "${RUN_DIR}/99_drift_report.json" \
    "gpt-5.4-mini" \
    "low" \
    "Compare old claims/docs (${OLD_CLAIMS}, ${OLD_DOCS}) against new claims/docs (${RUN_DIR}/96_verified_claims.json, ${RUN_DIR}/97_rendered_docs.json)." \
    "180"; then
    cp "${RUN_DIR}/99_drift_report.json" "docs/autogen/_drift_report.json"
  else
    echo "Drift audit failed or timed out; continuing without drift report." >&2
  fi
fi

ln -sfn "$RUN_ID" .codex-out/docs/latest

echo "Documentation generation complete"
echo "Run directory: ${RUN_DIR}"
