---
description: |
  Run the memory-consolidate plugin agent over the current main-branch facts to
  dedupe, resolve contradictions, archive stale entries, and reinforce
  actively-referenced facts. Use when the user says "consolidate memory",
  "clean up memory", or when invoked as /hellodb:consolidate-now.
  Lower-frequency than digest — only worth running when main has 20+ facts
  or when you notice drift.
---

Run the memory-consolidate pipeline:

1. **Query current facts.** Call `mcp__hellodb__hellodb_query`:
   - `namespace`: `claude.facts`
   - `schema`: `brain.fact`
   - `branch`: `claude.facts/main`
   - `limit`: 500

   Bail with `{"status": "empty"}` if 0 facts returned.

2. **Collect metadata for each fact.** For each record_id, call
   `mcp__hellodb__hellodb_get_metadata` (best-effort; skip on error — a
   missing metadata row means the fact has never been reinforced).

3. **Invoke the memory-consolidate plugin agent.** Use the `Task` tool with:
   - `subagent_type`: `memory-consolidate`
   - `description`: `consolidate facts`
   - `prompt`: The JSON payload per `plugin/agents/memory-consolidate.md`:
     ```json
     {
       "facts": [<fact objects, each with record_id, topic, statement, confidence, derived_from>],
       "metadata": { "<record_id>": {<RecordMetadata fields>} },
       "now_ms": <current unix ms>,
       "half_life_ms": 604800000
     }
     ```

4. **Parse the actions output.** Agent returns `{"actions": [...], "summary": {...}}`.
   If parsing fails, dump raw to stderr and stop.

5. **Apply each action atomically:**
   - `merge` / `supersede` → for each id in `archive[]`, call `mcp__hellodb__hellodb_archive`
   - `archive_stale` → `mcp__hellodb__hellodb_archive` on each id
   - `reinforce` → `mcp__hellodb__hellodb_reinforce` with `delta`
   - `promote_procedure` → NOT auto-applied. Print it in the report and let the user decide.

6. **Report.** Print the agent's `summary` plus any promote_procedure suggestions.

## Ground rules

- **Consolidation is destructive-ish.** Archive is reversible, but it hides facts from recall. Trust the agent's conservative posture — it's prompted to hold in ambiguous cases.
- **Don't auto-merge procedure promotions.** Creating a new procedure-typed fact requires human intent; surface the suggestion only.
- **Single pass per invocation.** Don't re-run the consolidate agent within the same skill run even if some actions were skipped.
- **Haiku, single attempt.** Same rules as digest: don't override the model, don't chase the agent for clarification.

If no actions are emitted, print `{"status": "stable", "held": N}` where N is the total fact count. That's a correct, clean outcome.
