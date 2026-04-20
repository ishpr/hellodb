# Claude Memory Import

## What It Does

Imports existing Claude Code project memory markdown files into `hellodb` as
signed records, preserving project namespace boundaries and idempotency.

## Prerequisites

- `hellodb` installed and initialized.
- Access to `~/.claude/projects/*/memory/*.md` on the host machine.

## Steps

1. Check current status:

   ```sh
   hellodb status
   ```

2. Run a dry-run first:

   ```sh
   hellodb ingest --from-claudemd --dry-run
   ```

3. Run the actual import:

   ```sh
   hellodb ingest --from-claudemd
   ```

4. Validate imported recall:

   ```sh
   hellodb recall --top 8 --format md
   ```

## Verification

- `hellodb status` shows one or more `claude.memory.<project>` namespaces.
- Re-running import on unchanged files yields a no-op (idempotent behavior).

## Troubleshooting

- If no files are found, confirm Claude memory markdown exists under
  `~/.claude/projects`.
- If permissions fail, verify your shell user can read those directories.
