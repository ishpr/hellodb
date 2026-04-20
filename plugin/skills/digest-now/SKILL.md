---
description: |
  Run the memory-digest plugin agent on recent episodes from claude.episodes and
  write the extracted facts to a draft branch in claude.facts. Use when the
  user says "digest now", "run memory digest", or when invoked as
  /hellodb:digest-now. Also fires from the Stop hook, via a background
  `claude -p` — don't run this yourself more than once per session unless
  the user explicitly asks.
---

You are running the memory-digest pipeline against hellodb. Your job:

1. **Guard against recursion.** If the environment variable `HELLODB_DIGEST_HOOK=1` is set, it means this session was spawned by the Stop hook for exactly this purpose. Run the digest once and then exit cleanly. If the env var is NOT set (normal interactive session), still fine — just proceed.

2. **Read episodes.** Call `mcp__hellodb__hellodb_tail`:
   - `namespace`: `claude.episodes`
   - `after_seq`: 0 (for first run) or the last digested cursor
   - `limit`: 200

   How do you know the last cursor? Try to read it from the brain's own state via reading `~/.hellodb/brain.state.json` using the Read tool. If it doesn't exist or you can't read it, default `after_seq` to 0 and trust `hellodb_remember` dedup (content-addressed records collapse).

3. **Read existing facts.** Call `mcp__hellodb__hellodb_query`:
   - `namespace`: `claude.facts`
   - `schema`: `brain.fact`
   - `branch`: `claude.facts/main`
   - `limit`: 100

   This gives the memory-digest agent context to detect duplicates and supersessions.

4. **Bail if nothing to do.** If `hellodb_tail` returned 0 entries, print `{"status": "no_episodes"}` and stop. Don't invoke the agent for an empty input.

5. **Invoke the memory-digest plugin agent.** Use the `Task` tool with:
   - `subagent_type`: `memory-digest`
   - `description`: `digest episodes batch`
   - `prompt`: Hand it the JSON payload described in `plugin/agents/memory-digest.md`:
     ```json
     {
       "episodes": [<episode objects from tail>],
       "existing_facts": [<fact objects from query>]
     }
     ```
     Include BOTH the topic+text of each episode and the record_id (agent needs the ids to cite in `derived_from`).

6. **Parse the agent's output.** It will return a JSON blob with `facts[]` and optional `notes[]`. If parsing fails, dump the raw output to stderr and stop — don't write partial results.

7. **Create a new digest branch.** Call `mcp__hellodb__hellodb_create_branch`:
   - `namespace`: `claude.facts`
   - `label`: `agent-digest-{current unix ms}`

8. **Write each fact to that branch.** For each fact in the agent's output, call `mcp__hellodb__hellodb_remember` with:
   - `namespace`: `claude.facts`
   - `schema`: `brain.fact`
   - `branch`: the branch id from step 7
   - `data`: the fact object (statement, topic, confidence, derived_from, rationale, optional supersedes)

9. **If the agent emitted `supersedes` anywhere, reinforce-archive the old fact.** For each superseded `record_id`, call `mcp__hellodb__hellodb_archive` with that `record_id`.

10. **Report.** Print a compact JSON summary to stdout:
    ```json
    {"status": "digested", "episodes_read": N, "facts_written": M, "branch": "claude.facts/agent-digest-…", "next_step": "run /hellodb:review to merge"}
    ```

## Ground rules

- **Don't announce each step to the user.** If this is a hook-fired session (env var set), be silent except for the final JSON. If interactive, one short paragraph summarizing results is enough.
- **Never auto-merge.** The digest branch must stay as a draft. User approves via `/hellodb:review`.
- **Haiku cost bound:** the memory-digest agent is tuned for haiku. Don't override the model. If you're tempted to give it a follow-up clarifying question, don't — refactor the input instead.
- **No recursion.** Do not invoke `/hellodb:digest-now` from within this skill. Do not re-invoke the memory-digest agent twice. One call per skill execution.

If anything fails that isn't "no episodes to digest," print a `{"status": "error", "reason": "..."}` JSON and exit non-zero so the hook can log it.
