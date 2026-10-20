# Meridian

Measures how League of Legends balance patches actually change champion performance.

Statistics sites show a champion's current win rate. Meridian answers a different question: when
Riot buffs or nerfs a champion, **how much does it move, and is that more than noise?** It collects
ranked matches and Riot's patch notes into one database, compares each patch with the one before
it, and reports win, pick and ban rates with confidence intervals and significance tests next to
the balance changes that were made.

```
Patch 16.2 impact report

| Champion | Changes  | Win rate        | Change  | 95% CI           | p     | Result            |
| Jinx     | buff (2) | 49.0% to 55.0%  | +6.0pp  | +1.9pp to +10.1pp| 0.004 | moved as intended |
| Ahri     | nerf (1) | 52.3% to 51.9%  | -0.4pp  | -3.5pp to +2.7pp | 0.801 | within noise      |
```

(Illustrative numbers. Run it on your own data.)

## Architecture

```
Meridian/
├── core_types/      Domain types, validation and statistics (no I/O)
├── patch_notes/     Patch-note HTML parser
├── engine_api/      SQLite storage and analysis, plus the `meridian` CLI
├── ingest/          `meridian-ingest`: Riot matches and patch notes into the database
└── meridian-server/ HTTP API and web dashboard
```

Ingestion, storage, analysis and presentation are separate: `ingest` only writes, `engine_api`
holds every calculation, and the CLI, server and dashboard only display what it returns.

### Data model

Matches are the source of truth and are never changed after they are stored. Each match records its
patch, region, tier, start time, ten participants (champion, role, win or loss) and bans. Every
statistic is a query over those rows, so a report always reflects exactly the matches in the
database. Balance changes parsed from patch notes are stored per patch (champion, ability,
attribute, old value, new value, and whether it is a buff or nerf).

The schema is versioned with `PRAGMA user_version`; databases are migrated automatically.

### What is measured

| Measure                    | How                                                                                         |
| -------------------------- | ------------------------------------------------------------------------------------------- |
| Win, pick, ban rate        | Counted from stored matches per champion, patch and filter (region, tier, role)             |
| Patch impact               | Win-rate, pick-rate and ban-rate difference between a patch and the previous one            |
| Statistical confidence     | 95% interval on the difference, and a two-proportion z-test (p < 0.05 is "significant")     |
| Small samples              | No test below 30 games on either side; Wilson intervals for single win rates                |
| Trends                     | Win rate per patch with a three-patch trailing average                                      |
| Balance effectiveness      | How often buffs raised and nerfs lowered a win rate, how often that was significant, and how often it moved toward 50% |
| Meta shift                 | Average size of win-rate and pick-rate changes across all champions, per patch              |
| Champions Riot did *not* change | Significant movers with no balance change, as context: something else moved them     |

Win-rate changes are **associations**, not proof of cause. Other champions, items and the player
population change between patches too, which is why the report shows unexplained movers beside the
changes.

## Getting started

```bash
cargo build --release
export MERIDIAN_DB=meridian.db          # optional; this is the default

# 1. Balance changes from Riot's patch notes (no API key needed).
#    Patches use the game's version numbers; 16.2 is the patch Riot's notes call 26.2.
./target/release/meridian-ingest notes 16.1
./target/release/meridian-ingest notes 16.2

# 2. Matches from the Riot API. Get a key at https://developer.riotgames.com
export RGAPI_KEY=RGAPI-...              # or put it in a .env file
./target/release/meridian-ingest matches --region na1 --tier DIAMOND --players 100

# 3. Look at the results.
./target/release/meridian report 16.2
./target/release/meridian history Jinx
./target/release/meridian rankings 16.2 --role support
./target/release/meridian shifts
./target/release/meridian effectiveness

# 4. Or browse them at http://127.0.0.1:7979
./target/release/meridian-server
```

Every `meridian` command takes `--json`, and `report` also takes `--format csv`. All of them accept
`--region`, `--tier` and `--role` to look at one slice of the data.

To compare two patches you need matches from both. Run `meridian-ingest matches` during each patch
and it only ever fetches matches it has not stored. The Riot client waits for the rate limit
itself (a development key allows about 100 requests every two minutes), so a large run is slow but
safe to interrupt and repeat.

### Keeping it up to date

Both ingestion commands are idempotent, so scheduling them is plain cron:

```cron
# every six hours: new matches for two tiers, and the notes of the current patch
0 */6 * * *  meridian-ingest matches --region na1 --tier DIAMOND >> ingest.log 2>&1
5 */6 * * *  meridian-ingest matches --region na1 --tier PLATINUM >> ingest.log 2>&1
0 12 * * *   meridian-ingest notes 16.2 >> ingest.log 2>&1
```

### HTTP API

`meridian-server` is read-only, listens on `127.0.0.1:7979` (set `MERIDIAN_ADDR`) and only answers
requests addressed to localhost, plus any host names listed in `MERIDIAN_HOSTS` (comma-separated,
for example `meridian.example.com,100.64.0.1`) when it is served on a private network or behind a
tunnel. Routes are documented at the top of
[`meridian-server/src/lib.rs`](meridian-server/src/lib.rs); for example
`GET /api/patches/16.2/report?tier=DIAMOND` and `GET /api/champions/Jinx/history`.

## Sampling and its limits

Riot has no feed of all matches and no table of champion win rates, so Meridian builds its own
sample, and the sample has biases you should know about:

- Matches are found through players. A run takes the players of **one tier in one region**, then
  each player's recent ranked solo games. A match is labelled with the tier of the player it was
  found through, so the label approximates the lobby's skill but is not exact.
- Only ranked solo queue games on Summoner's Rift are counted. Remakes and early surrenders are
  dropped.
- Frequent players appear more often in the sample than occasional ones.
- Comparing patches only makes sense when both samples were collected the same way, so keep the
  region, tier and sampling settings the same across runs.

See [`docs/DEVELOPMENT_NOTES.md`](docs/DEVELOPMENT_NOTES.md) for design decisions and what is not
built yet.

## Development

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Contributor rules are in [`CLAUDE.md`](CLAUDE.md). The tests never use the network.
