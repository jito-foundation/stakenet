# Stakenet Auto-Docs Agent System

This repository now includes a deterministic `codex exec` multi-agent docs pipeline.

## Key Files

- `scripts/generate_repo_docs.sh`
- `scripts/audit_doc_drift.sh`
- `.codex/agents/*.toml`
- `.agents/prompts/*.md`
- `.agents/schemas/*.json`

## Generate Docs

```bash
scripts/generate_repo_docs.sh
```

Optional comparison against previous run:

```bash
scripts/generate_repo_docs.sh --compare-to .codex-out/docs/<older_run_id>
```

## Drift Audit Only

```bash
scripts/audit_doc_drift.sh --old .codex-out/docs/<older_run_id> --new .codex-out/docs/<newer_run_id>
```

## Determinism Measures

- deterministic shell pre-index stage
- strict JSON schema at every LLM stage
- stable claim IDs and sorted claim arrays
- explicit claim verification before rendering docs
- optional claim-level drift classification after rendering
