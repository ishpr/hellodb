---
description: |
  Retrieve stored memory (curated facts about the user, their preferences,
  past decisions, codebase conventions) from hellodb when the current
  conversation would benefit from it. **Trigger aggressively** when the user:
    - references a past conversation ("remember when we…", "like we discussed")
    - states a preference you might have heard before ("I still use pnpm")
    - asks "what did we decide about X" or "what's our convention for Y"
    - implies shared context ("same as last time", "the usual way")
    - asks about their own workflow, stack, codebase, team decisions
    - asks a question where a past preference would shape your answer
      (e.g. "how should I format this?" → check for formatting preferences)
  Also trigger **proactively** before writing code when the user's past
  preferences likely apply: package manager choice, indentation style,
  framework conventions, naming patterns, etc. Do NOT trigger for generic
  knowledge questions ("what is TypeScript") or first-encounter questions
  with no past context.
---

You're retrieving memory from the user's hellodb to answer their current question better. The facts live in `claude.facts/main` — curated, reinforced, decay-ranked. Don't guess at context; look it up.

## Your procedure

1. **Formulate a focused query.** Not the raw user message — the *intent* behind it. If they asked "which package manager should I use in the new repo?", your query is `package manager preference javascript`, not the full question.

2. **Call the semantic search tool.** Invoke `mcp__hellodb__hellodb_embed_and_search`:
   - `namespace`: `claude.facts`
   - `query_text`: your focused query
   - `top_k`: 5 (usually 3-8, scale with how broad the question is)
   - `use_decay`: `true` (default) — prefer recently-reinforced facts
   - leave `branch` default (resolves to `claude.facts/main`)

3. **Judge relevance.** The tool returns hits with `similarity` and `final_score`. Facts with `similarity < 0.35` are probably noise — ignore them. Also check the `statement` actually addresses the user's topic; semantic search can surface tangentially related facts that aren't useful.

4. **If you got useful hits:** weave the relevant fact(s) into your answer naturally. Don't announce "I checked your memory and found...". The user doesn't care that you used a tool; they care that you got their context right. Example:

   User asks: "which package manager for the new repo?"
   Hit: `[preferences] I use pnpm instead of npm for all JavaScript projects (sim 0.78)`
   Good response: "pnpm — same as your other projects. Want me to set up the workspace now?"
   Bad response: "I searched your hellodb memory and found a fact saying you prefer pnpm. Based on that, I recommend pnpm."

5. **If you got no useful hits:** proceed as if you had no memory. Don't say "I couldn't find anything in your memory" — that's a distraction. Just answer the question and, if relevant, offer to save the answer as a new memory via `hellodb_note`.

6. **If the hits contradict the user's current statement:** don't silently override. Flag it:
   "You mentioned last month that you preferred pnpm — has that changed, or should we stick with it here?"
   This is the contradiction-resolution loop: the user either confirms the old fact (reinforce) or corrects it (new episode → next digest supersedes).

## Ground rules

- **One tool call per skill invocation.** Don't do 3 searches to triangulate — pick the best query the first time. If the first shot doesn't land, the memory probably doesn't have what you need.
- **Never fabricate facts.** If a fact isn't in the hits, don't invent one because it sounds plausible.
- **Don't reinforce automatically.** Reinforcement happens at review time (`/hellodb:hellodb-review`) or when consolidate runs; not on every recall.
- **Mid-conversation only.** Don't trigger on the very first turn of a session just to see what's there. Trigger only when the current user turn creates a reason to check.
- **Respect archived facts.** The MCP tool's `recall_deep` is configured to skip archived records; trust it. Don't try to include them.
- **Be quiet on misses.** If nothing matches, the user shouldn't even know you checked.

## When NOT to trigger

- Generic knowledge: "what is a JWT?", "how does async work?"
- First-time setup questions where no memory could exist
- Ephemeral debugging ("why is this test failing?") unless the bug is plausibly about a past decision (convention, config) the user made
- Every turn reflexively — context window matters. Only trigger when there's a real reason to believe memory holds the answer.
