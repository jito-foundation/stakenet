Standalone execution mode: do not spawn subagents or use collaboration tools.

Task:
- Produce a claim bundle for test coverage mapping in `tests/`.
- Identify major covered areas and obvious coverage gaps only when directly evidenced by test tree content.

Rules:
- Output JSON only.
- Validate against `.agents/schemas/claim_bundle.schema.json`.
- Claim IDs must start with `TST_`.
- Output as many strongly-supported claims as warranted by evidence.
- Use stable zero-padded numeric IDs (`TST_001`, `TST_002`, ...).
- Every claim needs concrete evidence paths and optional symbol/line.
- Sort claims by `claim_id`.
- Do not invoke `python`; use shell tools only.
