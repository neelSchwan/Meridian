# Development notes

Context and history for people and agents picking up work on Meridian. Code comments explain the
code as it is; this file explains how it got here and what is planned next. Add a dated entry when
you finish a meaningful piece of work.

## Current state

- Phases 1 to 5 of the original roadmap are built: match ingestion, patch-note parsing, impact
  analysis, an HTTP API, a CLI, a web dashboard and CSV export. Phase 6 is partly built (see
  "Not built").
- Schema version is 1 (`PRAGMA user_version`).
- Tests: `core_types` (statistics and classification, unit tests), `patch_notes/tests/parse.rs`
  (parser, using an excerpt of the real 25.20 page), `engine_api/tests/engine.rs` (storage,
  reports, filters), `ingest/tests/ingest.rs` (Riot match conversion from JSON, notes import) and
  `meridian-server/tests/api.rs` (every route through `handle`).

## Verified, and not verified

Checked against the real world:
- The patch-note parser reads Riot's 25.20 and 26.1 pages (34 and 84 changes), and
  `meridian-ingest notes` was run end to end against the live site.
- The dashboard was exercised in headless Chromium against a synthetic database: all four tabs,
  filters, and click-through, with no console errors.

**Not checked against the live Riot API:** `meridian-ingest matches`. It was written without an
API key. What is tested is everything that does not need the network: converting a Riot match
(from JSON shaped like Riot's), deduplication, and storage. The calls that find players and list
matches (`ingest/src/riot.rs`) follow `riven`'s signatures and compile, but the first real run may
reveal a problem, such as a response shape or a rate-limit behavior. Run it with a small
`--players` value first.

## Design decisions

**SQLite, not PostgreSQL.** The original plan was Node.js, Express and PostgreSQL. The repository
already held Rust code using `riven`, and the sibling project memd uses SQLite, so the whole
system is one language with no database server. The queries are plain SQL (aggregates over
`participants` and `bans`); nothing relies on features only PostgreSQL has. The write-ahead log
lets the server read while an ingestion run writes. If the dataset outgrows one file, the SQL in
`engine_api/src/lib.rs` and `analysis.rs` is the part to port.

**No stored aggregates.** The roadmap mentioned materialized views. A report is a handful of
grouped queries over one patch's matches, which is fast at the sizes a developer key can collect,
and computing on demand means there is nothing to refresh or to drift out of date. Add a cache only
after measuring a slow report.

**Patches are identified by the game's version, not the notes' label.** Riot's match data reports
`16.2`; its notes call the same patch `26.2` (the notes switched to year-based numbers in 2025).
`Patch::notes_label` converts, and was checked against the 15.20/25.20 and 16.1/26.1 pages. If Riot
changes the numbering again, that function and `fetch_patch_notes` are the places to update.

**Champions are joined by a normalized key.** Match data and patch notes spell names differently
(`MonkeyKing` and Wukong, `Kai'Sa` and `Kaisa`). `champion_key` lower-cases and strips everything
but letters and digits, and maps the three champions whose names differ in substance (Wukong,
Nunu & Willump, Renata Glasc). A new champion with an unusual name would need an entry there.

**The parser reads as little markup as possible.** Riot's page has no structured data and its CSS
class names are generated, so the parser depends only on `patch-change-block`, `change-title` with
a `/champions/` link, `h4` headings and `Attribute: old ⇒ new` list items. Arena and Doom Bots
sections list champion changes too, but with different markup, and are deliberately ignored: they
are not Summoner's Rift balance. An empty result is an error, so a redesign fails loudly.

**Buff or nerf comes from comparing the first number.** `classify_change` compares the first number
of the old and new value and reads the direction from the attribute name (a higher cooldown is a
nerf). Values that begin with words, such as a rework, are adjustments. This misjudges changes where
the first number is not the point (for example a ratio that rises while a base value falls), and
tooltip-only changes without a number. A champion's overall kind is a buff or nerf only if all its
directional changes agree; mixed changes are an adjustment and are left out of effectiveness. If
effectiveness numbers matter for a decision, spot-check the classification first.

**The statistics are deliberately plain.** A win-rate change is tested with a two-proportion
z-test and a normal-approximation interval; single win rates use Wilson intervals. Below 30 games on
either side no test is run. The normal CDF uses the Numerical Recipes `erfcc` approximation
(relative error under 1.2e-7). Every champion is tested separately with alpha 0.05 and no correction
for multiple comparisons: with 170 champions, about 8 would look significant by chance alone, which
is why the report lists significant movers Riot did not change right beside the changed champions.
The comparison also ignores that the same player can appear in many matches.

**A patch is the comparison window.** The vision described two-week windows after a patch. Patches
last about two weeks, and "this patch against the previous one" needs no release dates (which the
match API does not give). The two-week persistence check, rolling averages by day, and day-level
charts are not built; the history view smooths across patches instead.

**Regions compare case-insensitively.** Ingestion stores lower-case platform names (`na1`), and the
SQL lower-cases the filter, so every frontend accepts `NA1` too.

**Bans ignore the role filter.** A ban is not tied to a role, so a role-filtered ban rate is the
champion's overall ban rate.

**Tier is the discovery tier.** Riot's match data has no ranks, so a match carries the tier of the
player it was found through. A Diamond player's lobby can include lower and higher ranks. This is
documented in the README as a sampling bias.

**Transactions are IMMEDIATE.** Writes read before they write (is this match stored? which id does
this champion have?), and a deferred transaction in WAL mode fails with "database is locked" when
another connection commits in between. `two_connections_to_one_file_can_both_write` covers it.

**The server is synchronous, read-only and local.** Like `memd-server`, it uses `tiny_http`, a
`match` on path segments instead of a router, and a `handle` function the tests call directly. It
checks the `Host` header to stop DNS rebinding. It has no write routes, so it needs no
content-type checks or body limit.

**The dashboard is one static file** (`meridian-server/ui/index.html`), plain HTML, CSS and
JavaScript with no build step or CDN, embedded with `include_str!`. All API data is rendered with
`textContent`. Charts are hand-built SVG, because two small charts did not justify a library.

**The old `seeding` crate was replaced.** It was a 45-line experiment that printed player names
from one match. `ingest` supersedes it; git has the history.

## Not built

- **Discord bot.** The JSON API already gives a bot everything it needs; the bot itself would be a
  small separate crate.
- **Item analysis.** The parser skips item sections. Items would need their own table and parser
  rules; the `patch-change-block` structure is the same.
- **Rank and matchup segmentation beyond filters.** Region, tier and role filters exist. Matchups
  (champion against champion) are not stored; `participants` would need a team column.
- **Multi-season comparisons and forecasting.** Nothing prevents loading several seasons, but
  nothing was tested over more than a handful of patches. Forecasting patch impact from history is
  the longer-term idea in the original vision and needs far more data than a development key
  collects.
- **A per-day time series** (see "A patch is the comparison window").
- **Deployment.** There is no Dockerfile or service unit; cron plus the two binaries is the
  documented setup.
- **Attribution beyond association.** There is no adjustment for other simultaneous changes. A
  difference-in-differences style control (the average movement of unchanged champions) is the
  natural next step, with the caveat that win rates sum to 50% so everything is correlated.
