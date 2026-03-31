Standalone execution mode: do not spawn subagents or use collaboration tools.

Task:
- Produce a claim bundle for `utils/steward-cli` and `utils/validator-history-cli`.
- Focus on command surface, high-value operational commands, permissions/signer model, and key argument groups.

Rules:
- Output JSON only.
- Validate against `.agents/schemas/claim_bundle.schema.json`.
- Claim IDs must start with `CLI_`.
- Output as many strongly-supported claims as warranted by evidence.
- Prefer stable semantic IDs (for example `CLI_STEWARD_SIGNER_MODEL`, `CLI_VH_COMMAND_SURFACE`) instead of fixed numeric quotas.
- Every claim needs concrete evidence paths and optional symbol/line.
- Sort claims by `claim_id`.
- Do not invoke `python`; use shell tools only.
