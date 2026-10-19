# Meridian: Contributor Guidelines

These rules apply to every change in this repository, whether written by a person or an AI agent.
Claude Code loads this file automatically at the start of every session. They are the same rules
as the sibling project memd, adapted to Meridian's architecture.

## 1. Simplicity comes first

Simplicity is the main goal. When two designs both work, pick the one with less code, fewer moving
parts, and fewer concepts a reader has to keep in mind.

- Solve the problem in front of you. Don't add configuration, abstraction layers, or extension
  points "for later".
- Prefer plain functions and data over traits, generics, and macros. Add a trait only when there
  are two real implementations.
- Keep functions short and doing one thing. If a function needs a comment to explain its sections,
  split it into smaller, well-named functions instead.
- Delete dead code; don't comment it out. Git keeps the history.
- If a change makes the code harder to read, it needs a strong reason.

## 2. Dependencies

Don't add a crate unless it is clearly necessary.

- First check whether the standard library or an existing dependency already does the job.
- A new dependency must be justified in the PR description: what it does, and why writing it
  ourselves would be worse.
- Remove dependencies that are no longer used. CI runs clippy, but it does not catch unused crates,
  so check `Cargo.toml` when you remove code.
- Current dependencies, for reference: `serde` (domain types); `rusqlite` (storage); `scraper`
  (reading patch-note HTML); `clap` (CLIs); `riven`, `tokio`, `reqwest`, `dotenv` (talking to Riot
  and loading the API key); `tiny_http`, `serde_json` (HTTP server and JSON output).

## 3. Comments

Comments are for the reviewer reading the code next year, not a log of what you did today.

**Do comment:**
- Every public function, with a short `///` doc comment saying what it does and anything surprising
  about its behavior (for example: "returns `false`, changing nothing, when the match was already
  stored").
- Anything that isn't obvious from the code itself: why a decision was made, an invariant that must
  hold, a non-obvious SQL constraint, a statistical assumption, an edge case handled on purpose.
- Workarounds, with the reason they exist.

**Don't comment:**
- What a line plainly does (`// increment counter`). Good names make these unnecessary.
- Every line or every block. Most code should have no comments.
- History, session notes, TODO lists for agents, or "changed X to Y" remarks. Those belong in
  [`docs/DEVELOPMENT_NOTES.md`](docs/DEVELOPMENT_NOTES.md) or in the commit message, never in code.

Write comments in plain, complete sentences that a person can review without extra context.

## 4. Naming

Follow standard Rust naming (`snake_case` functions and variables, `CamelCase` types,
`SCREAMING_SNAKE_CASE` constants), plus these project conventions:

- Names describe what a thing is, not how it's stored: `win_rate`, not `wr` or `ratio_val`.
- No single-letter names except short closures and loop indexes.
- Booleans read as questions: `won`, `significant`, `is_apex`.
- Domain words are used consistently: **patch**, **match**, **participant**, **champion**,
  **balance change**, **impact**, **report**, **filter**, **tier**, **role**. Don't introduce
  synonyms like "version" (for a patch), "record" (for a match) or "buff list" in code.
  `match` is a Rust keyword, so a local variable holding Riot's match may be called `game`; the
  type is `MatchRecord`.
- A rate is a fraction between 0 and 1 everywhere inside the code (`0.512`); only the text and
  dashboard output turn it into a percentage. A change in a rate is a difference of fractions,
  shown as percentage points ("pp").
- SQL row structs are named `<Thing>Row` and convert with `from_row` / `into_<thing>`.

## 5. Architecture rules

```
core_types/      Domain types, validation and statistics. No I/O, no database.
patch_notes/     Patch-note HTML parser. Pure: text in, balance changes out.
engine_api/      SQLite storage and analysis. All logic lives here. Also the `meridian` CLI binary.
ingest/          Fetches matches from the Riot API and notes from Riot's site. `meridian-ingest`.
meridian-server/ HTTP API and dashboard. A thin layer that maps routes to engine_api calls.
```

- **Matches are the source of truth.** `matches`, `participants` and `bans` are written once and
  never updated or deleted. Every statistic is computed from them on demand; don't store
  aggregates that could drift out of date. `balance_changes` is the one derived table, replaced
  per patch when notes are imported again.
- Writes that touch more than one row run in a transaction.
- Frontends (`meridian-server`, the CLI, `ingest`) contain no business logic. If they need
  something new, add it to `engine_api` or `core_types` first.
- Keep network code thin. Anything that can be decided without the network (converting a Riot
  match, parsing a page, choosing what to import) is a plain function with a test.
- Schema changes need a new step in `engine_api/src/schema.rs` and a bump of `SCHEMA_VERSION`, plus
  a migration test. Existing user databases must keep working.
- **Statistical claims need their uncertainty.** A new number that compares two patches reports the
  sample size behind it, and anything called significant goes through a test. Win-rate changes are
  associations: don't word output or docs as if a balance change was proven to cause one.

## 6. Errors

- Return `engine_api::Result`. Don't `unwrap()` or `expect()` outside tests, except for a lock
  that can only fail if another thread already panicked, or a constant that is checked by tests
  (such as a CSS selector).
- Add an `EngineError` variant when callers need to tell a case apart. Error messages are shown to
  users, so make them specific: "no matches found for patch 16.3", not "query failed".
- Failing loudly beats importing nothing quietly: a patch-notes page that yields no champion
  changes is an error, because it means Riot changed the page.

## 7. Tests and checks

Every change must pass these before it is pushed:

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test
```

- New engine behavior gets an integration test in `engine_api/tests/`.
- Bug fixes get a test that fails without the fix.
- Tests use an in-memory database and read like a short story: set up, act, assert. Tests never
  call the network; real Riot HTML lives in `patch_notes/tests/fixtures/`.

## 8. Pull requests

- One topic per PR. Keep diffs small enough to review in one sitting.
- The description says what changed, why, and how it was tested.
- Commit messages explain why, not just what.
- Before merging, review the diff against this file. The `pr-reviewer` agent
  (`.claude/agents/pr-reviewer.md`) does exactly that.
