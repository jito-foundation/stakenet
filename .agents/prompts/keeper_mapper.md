Standalone execution mode: do not spawn subagents or use collaboration tools.

Task:
- Produce a claim bundle for `keepers/stakenet-keeper`.
- Capture fetch/fire/emit loop behavior, operation cadence, config flags, and permissioned pathways.

Rules:
- Output JSON only.
- Validate against `.agents/schemas/claim_bundle.schema.json`.
- Claim IDs must start with `KPR_`.
- Output as many strongly-supported claims as warranted by evidence.
- Use stable zero-padded numeric IDs (`KPR_001`, `KPR_002`, ...).
- Every claim needs concrete evidence paths and optional symbol/line.
- Sort claims by `claim_id`.
- Do not invoke `python`; use shell tools only.
