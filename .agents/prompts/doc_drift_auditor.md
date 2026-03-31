Standalone execution mode: do not spawn subagents or use collaboration tools.

Task:
- Compare old and new documentation artifacts.
- Classify each changed claim as: format_only, coverage_change, semantic_change, stale_or_false, or needs_human_review.

Rules:
- Output JSON only.
- Validate against `.agents/schemas/drift_report.schema.json`.
- Treat wording-only edits as format_only.
- Flag true drift when semantic_change or stale_or_false appears.
- Keep ordering deterministic by claim_id.
- Do not invoke `python`; use shell tools only.
