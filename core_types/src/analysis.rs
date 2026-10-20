use serde::Serialize;
use serde::ser::SerializeStruct;
use std::collections::BTreeSet;

use crate::{
    BalanceChange, ChangeKind, Filter, MIN_GAMES, Patch, ProportionTest, champion_key,
    overall_kind, two_proportion_test, wilson_interval,
};

/// One champion's results within one patch (and one [`Filter`] slice of the data).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChampionStats {
    pub champion: String,
    pub games: u64,
    pub wins: u64,
    /// Matches in which the champion was banned.
    pub bans: u64,
    /// Matches in the patch, so pick and ban rates are shares of all games.
    pub total_matches: u64,
}

impl ChampionStats {
    /// A champion that was never seen: used when it appears in only one of two patches.
    fn absent(champion: &str, total_matches: u64) -> ChampionStats {
        ChampionStats {
            champion: champion.to_string(),
            games: 0,
            wins: 0,
            bans: 0,
            total_matches,
        }
    }

    /// `None` when the champion has no games.
    pub fn win_rate(&self) -> Option<f64> {
        (self.games > 0).then(|| self.wins as f64 / self.games as f64)
    }

    /// The share of matches in which the champion was played.
    pub fn pick_rate(&self) -> f64 {
        share(self.games, self.total_matches)
    }

    /// The share of matches in which the champion was banned.
    pub fn ban_rate(&self) -> f64 {
        share(self.bans, self.total_matches)
    }
}

fn share(count: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        count as f64 / total as f64
    }
}

impl Serialize for ChampionStats {
    /// Includes the derived rates, so clients never recompute them.
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let interval = wilson_interval(self.wins, self.games);
        let mut out = serializer.serialize_struct("ChampionStats", 10)?;
        out.serialize_field("champion", &self.champion)?;
        out.serialize_field("games", &self.games)?;
        out.serialize_field("wins", &self.wins)?;
        out.serialize_field("bans", &self.bans)?;
        out.serialize_field("total_matches", &self.total_matches)?;
        out.serialize_field("win_rate", &self.win_rate())?;
        out.serialize_field("win_rate_low", &interval.map(|(low, _)| low))?;
        out.serialize_field("win_rate_high", &interval.map(|(_, high)| high))?;
        out.serialize_field("pick_rate", &self.pick_rate())?;
        out.serialize_field("ban_rate", &self.ban_rate())?;
        out.end()
    }
}

/// How one champion's results moved from one patch to the next, with whatever balance changes
/// Riot made to it in between.
#[derive(Debug, Clone, Serialize)]
pub struct Impact {
    pub champion: String,
    pub before: ChampionStats,
    pub after: ChampionStats,
    /// Later win rate minus earlier one; `None` when either patch has no games.
    pub win_rate_delta: Option<f64>,
    pub pick_rate_delta: f64,
    pub ban_rate_delta: f64,
    /// `None` when either patch has too few games to test.
    pub test: Option<ProportionTest>,
    /// Empty for champions Riot did not touch, whose movement the changes cannot explain.
    pub changes: Vec<BalanceChange>,
    pub kind: Option<ChangeKind>,
    /// For a buff, whether the win rate rose; for a nerf, whether it fell. `None` unless both
    /// patches have enough games to test, so a handful of games cannot count as evidence.
    pub moved_as_intended: Option<bool>,
    /// Whether the win rate ended up closer to 50% than it started. `None` when not testable.
    pub moved_toward_even: Option<bool>,
}

impl Impact {
    fn new(before: ChampionStats, after: ChampionStats, changes: Vec<BalanceChange>) -> Impact {
        let test = two_proportion_test(before.wins, before.games, after.wins, after.games);
        let rates = before.win_rate().zip(after.win_rate());
        let win_rate_delta = rates.map(|(old, new)| new - old);
        // Direction claims need a testable sample; the raw delta is still reported without one.
        let testable_delta = win_rate_delta.filter(|_| test.is_some());
        let kind = overall_kind(changes.iter().map(|change| change.kind));
        let moved_as_intended = match (kind, testable_delta) {
            (Some(ChangeKind::Buff), Some(delta)) => Some(delta > 0.0),
            (Some(ChangeKind::Nerf), Some(delta)) => Some(delta < 0.0),
            _ => None,
        };
        Impact {
            champion: after.champion.clone(),
            win_rate_delta,
            pick_rate_delta: after.pick_rate() - before.pick_rate(),
            ban_rate_delta: after.ban_rate() - before.ban_rate(),
            test,
            kind,
            moved_as_intended,
            moved_toward_even: rates
                .filter(|_| test.is_some())
                .map(|(old, new)| (new - 0.5).abs() < (old - 0.5).abs()),
            before,
            after,
            changes,
        }
    }

    /// True when the win-rate change is unlikely to be noise.
    pub fn significant(&self) -> bool {
        self.test.is_some_and(|test| test.significant())
    }
}

fn total_matches(stats: &[ChampionStats]) -> u64 {
    stats.first().map_or(0, |row| row.total_matches)
}

/// Compares every champion seen in either patch, attaching the balance changes made in the later
/// one. Sorted by the size of the win-rate change, largest first; champions that cannot be
/// compared come last.
pub fn impacts(
    before: &[ChampionStats],
    after: &[ChampionStats],
    changes: &[BalanceChange],
) -> Vec<Impact> {
    let names: BTreeSet<&str> = before
        .iter()
        .chain(after)
        .map(|row| row.champion.as_str())
        .collect();
    let find = |rows: &[ChampionStats], name: &str| {
        rows.iter()
            .find(|row| row.champion == name)
            .cloned()
            .unwrap_or_else(|| ChampionStats::absent(name, total_matches(rows)))
    };
    let mut all: Vec<Impact> = names
        .into_iter()
        .map(|name| {
            let own_changes = changes
                .iter()
                .filter(|change| champion_key(&change.champion) == champion_key(name))
                .cloned()
                .collect();
            Impact::new(find(before, name), find(after, name), own_changes)
        })
        .collect();
    all.sort_by(|a, b| {
        let size = |impact: &Impact| impact.win_rate_delta.map(f64::abs);
        size(b)
            .partial_cmp(&size(a))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.champion.cmp(&b.champion))
    });
    all
}

/// How much the whole meta moved between two patches, judged on champions with enough games in
/// both to compare.
#[derive(Debug, Clone, Serialize)]
pub struct MetaShift {
    pub patch: Patch,
    pub previous: Patch,
    pub champions_compared: usize,
    /// Average size of the win-rate change across those champions.
    pub mean_abs_win_rate_delta: f64,
    /// Average size of the pick-rate change across those champions.
    pub mean_abs_pick_rate_delta: f64,
}

/// Measures how much the meta moved from `previous` to `patch`, given each patch's statistics.
pub fn meta_shift(
    patch: Patch,
    previous: Patch,
    before: &[ChampionStats],
    after: &[ChampionStats],
) -> MetaShift {
    let comparable: Vec<(f64, f64)> = impacts(before, after, &[])
        .iter()
        .filter(|impact| impact.before.games >= MIN_GAMES && impact.after.games >= MIN_GAMES)
        .filter_map(|impact| Some((impact.win_rate_delta?.abs(), impact.pick_rate_delta.abs())))
        .collect();
    let mean = |values: Vec<f64>| {
        if values.is_empty() {
            0.0
        } else {
            values.iter().sum::<f64>() / values.len() as f64
        }
    };
    MetaShift {
        patch,
        previous,
        champions_compared: comparable.len(),
        mean_abs_win_rate_delta: mean(comparable.iter().map(|(win, _)| *win).collect()),
        mean_abs_pick_rate_delta: mean(comparable.iter().map(|(_, pick)| *pick).collect()),
    }
}

/// How often buffs or nerfs moved a win rate the way they were meant to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Effectiveness {
    pub kind: ChangeKind,
    /// Champion-patches with this kind of change and enough games on both sides to test.
    pub measured: u64,
    pub in_intended_direction: u64,
    /// Of those, how many moved by more than noise would explain.
    pub significantly_in_intended_direction: u64,
    /// How many ended closer to a 50% win rate.
    pub toward_even: u64,
}

/// Summarizes buffs and nerfs across many patches' impacts. Mixed changes are left out, because
/// they have no single intended direction.
pub fn effectiveness(all: &[Impact]) -> Vec<Effectiveness> {
    [ChangeKind::Buff, ChangeKind::Nerf]
        .into_iter()
        .map(|kind| {
            let measured: Vec<&Impact> = all
                .iter()
                .filter(|impact| impact.kind == Some(kind) && impact.test.is_some())
                .collect();
            let count = |keep: &dyn Fn(&&Impact) -> bool| {
                measured.iter().filter(|impact| keep(impact)).count() as u64
            };
            Effectiveness {
                kind,
                measured: measured.len() as u64,
                in_intended_direction: count(&|i| i.moved_as_intended == Some(true)),
                significantly_in_intended_direction: count(&|i| {
                    i.moved_as_intended == Some(true) && i.significant()
                }),
                toward_even: count(&|i| i.moved_toward_even == Some(true)),
            }
        })
        .collect()
}

/// One champion's results in one patch, in a series across patches.
#[derive(Debug, Clone, Serialize)]
pub struct HistoryPoint {
    pub patch: Patch,
    pub stats: ChampionStats,
    /// The mean win rate over this patch and the two before it, to show the trend through noise.
    pub smoothed_win_rate: Option<f64>,
    /// What Riot changed about the champion in this patch.
    pub changes: Vec<BalanceChange>,
}

/// A champion and how far its win rate moved.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Mover {
    pub champion: String,
    pub win_rate_delta: f64,
}

/// The comparison of one patch with the one before it.
#[derive(Debug, Clone, Serialize)]
pub struct PatchReport {
    pub patch: Patch,
    pub previous: Patch,
    pub filter: Filter,
    pub matches_before: u64,
    pub matches_after: u64,
    pub champions_changed: usize,
    /// Among champions Riot changed.
    pub largest_gain: Option<Mover>,
    pub largest_drop: Option<Mover>,
    pub meta_shift: MetaShift,
    pub impacts: Vec<Impact>,
}

impl PatchReport {
    /// Builds the report from both patches' statistics and the balance changes of `patch`.
    pub fn new(
        patch: Patch,
        previous: Patch,
        filter: Filter,
        before: &[ChampionStats],
        after: &[ChampionStats],
        changes: &[BalanceChange],
    ) -> PatchReport {
        let impacts = impacts(before, after, changes);
        let changed: Vec<&Impact> = impacts.iter().filter(|i| !i.changes.is_empty()).collect();
        let movers: Vec<Mover> = changed
            .iter()
            .filter_map(|impact| {
                Some(Mover {
                    champion: impact.champion.clone(),
                    win_rate_delta: impact.win_rate_delta?,
                })
            })
            .collect();
        let by_delta = |a: &&Mover, b: &&Mover| a.win_rate_delta.total_cmp(&b.win_rate_delta);
        PatchReport {
            patch,
            previous,
            filter,
            matches_before: total_matches(before),
            matches_after: total_matches(after),
            champions_changed: changed.len(),
            largest_gain: movers
                .iter()
                .max_by(by_delta)
                .filter(|m| m.win_rate_delta > 0.0)
                .cloned(),
            largest_drop: movers
                .iter()
                .min_by(by_delta)
                .filter(|m| m.win_rate_delta < 0.0)
                .cloned(),
            meta_shift: meta_shift(patch, previous, before, after),
            impacts,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats(champion: &str, games: u64, wins: u64) -> ChampionStats {
        ChampionStats {
            champion: champion.to_string(),
            games,
            wins,
            bans: 0,
            total_matches: 1000,
        }
    }

    fn change(champion: &str, kind: ChangeKind) -> BalanceChange {
        BalanceChange {
            champion: champion.to_string(),
            ability: "Q".to_string(),
            attribute: "Damage".to_string(),
            old_value: "80".to_string(),
            new_value: "75".to_string(),
            kind,
        }
    }

    fn patch(text: &str) -> Patch {
        text.parse().unwrap()
    }

    #[test]
    fn rates_are_shares_of_games_and_matches() {
        let row = ChampionStats {
            bans: 250,
            ..stats("Ahri", 100, 55)
        };
        assert_eq!(row.win_rate(), Some(0.55));
        assert_eq!(row.pick_rate(), 0.1);
        assert_eq!(row.ban_rate(), 0.25);
        assert_eq!(stats("Ahri", 0, 0).win_rate(), None);
    }

    #[test]
    fn an_impact_carries_the_delta_the_test_and_the_changes() {
        let before = [stats("Jinx", 1000, 490)];
        let after = [stats("Jinx", 1000, 550)];
        let all = impacts(&before, &after, &[change("Jinx", ChangeKind::Buff)]);

        let jinx = &all[0];
        assert!((jinx.win_rate_delta.unwrap() - 0.06).abs() < 1e-12);
        assert_eq!(jinx.kind, Some(ChangeKind::Buff));
        assert_eq!(jinx.moved_as_intended, Some(true));
        assert!(jinx.significant());
    }

    #[test]
    fn a_nerf_that_raises_the_win_rate_did_not_move_as_intended() {
        let before = [stats("Ahri", 500, 250)];
        let after = [stats("Ahri", 500, 260)];
        let all = impacts(&before, &after, &[change("Ahri", ChangeKind::Nerf)]);
        assert_eq!(all[0].moved_as_intended, Some(false));
    }

    #[test]
    fn changes_are_matched_to_champions_by_key_not_spelling() {
        let before = [stats("MonkeyKing", 500, 250)];
        let after = [stats("MonkeyKing", 500, 250)];
        let all = impacts(&before, &after, &[change("Wukong", ChangeKind::Nerf)]);
        assert_eq!(all[0].changes.len(), 1);
    }

    #[test]
    fn a_champion_missing_from_one_patch_has_no_delta() {
        let before = [stats("Ahri", 500, 250)];
        let after = [stats("Ahri", 500, 250), stats("Ambessa", 400, 220)];
        let all = impacts(&before, &after, &[]);
        let ambessa = all.iter().find(|i| i.champion == "Ambessa").unwrap();
        assert_eq!(ambessa.win_rate_delta, None);
        assert!(ambessa.test.is_none());
        assert!(ambessa.pick_rate_delta > 0.0);
    }

    #[test]
    fn impacts_are_sorted_by_the_size_of_the_change() {
        let before = [
            stats("A", 500, 250),
            stats("B", 500, 250),
            stats("C", 500, 250),
        ];
        let after = [
            stats("A", 500, 255),
            stats("B", 500, 200),
            stats("C", 500, 280),
        ];
        let names: Vec<_> = impacts(&before, &after, &[])
            .into_iter()
            .map(|i| i.champion)
            .collect();
        assert_eq!(names, ["B", "C", "A"]);
    }

    #[test]
    fn a_small_sample_gets_a_delta_but_no_test() {
        let all = impacts(&[stats("Ahri", 20, 10)], &[stats("Ahri", 20, 16)], &[]);
        assert!(all[0].win_rate_delta.is_some());
        assert!(all[0].test.is_none());
        assert!(!all[0].significant());
    }

    #[test]
    fn a_buff_seen_in_a_handful_of_games_is_not_evidence() {
        let before = [stats("Rare", 1, 0)];
        let after = [stats("Rare", 3, 3)];
        let all = impacts(&before, &after, &[change("Rare", ChangeKind::Buff)]);

        assert_eq!(all[0].moved_as_intended, None);
        assert_eq!(effectiveness(&all)[0].measured, 0);
    }

    #[test]
    fn effectiveness_counts_buffs_and_nerfs_separately() {
        let before = [
            stats("A", 1000, 500),
            stats("B", 1000, 540),
            stats("C", 1000, 500),
        ];
        let after = [
            stats("A", 1000, 560),
            stats("B", 1000, 500),
            stats("C", 1000, 510),
        ];
        let changes = [
            change("A", ChangeKind::Buff),
            change("B", ChangeKind::Nerf),
            change("C", ChangeKind::Nerf),
        ];
        let result = effectiveness(&impacts(&before, &after, &changes));

        let buffs = &result[0];
        assert_eq!(
            (buffs.kind, buffs.measured, buffs.in_intended_direction),
            (ChangeKind::Buff, 1, 1)
        );
        assert_eq!(buffs.significantly_in_intended_direction, 1);
        let nerfs = &result[1];
        assert_eq!(
            (nerfs.kind, nerfs.measured, nerfs.in_intended_direction),
            (ChangeKind::Nerf, 2, 1)
        );
        assert_eq!(nerfs.toward_even, 1);
    }

    #[test]
    fn meta_shift_averages_over_champions_with_enough_games() {
        let before = [
            stats("A", 500, 250),
            stats("B", 500, 250),
            stats("Rare", 5, 1),
        ];
        let after = [
            stats("A", 500, 260),
            stats("B", 500, 240),
            stats("Rare", 5, 5),
        ];
        let shift = meta_shift(patch("16.2"), patch("16.1"), &before, &after);
        assert_eq!(shift.champions_compared, 2);
        assert!((shift.mean_abs_win_rate_delta - 0.02).abs() < 1e-12);
    }

    #[test]
    fn the_report_summarizes_only_changed_champions() {
        let before = [
            stats("Jinx", 500, 245),
            stats("Ahri", 500, 260),
            stats("Lux", 500, 250),
        ];
        let after = [
            stats("Jinx", 500, 257),
            stats("Ahri", 500, 251),
            stats("Lux", 500, 300),
        ];
        let changes = [
            change("Jinx", ChangeKind::Buff),
            change("Ahri", ChangeKind::Nerf),
        ];
        let report = PatchReport::new(
            patch("16.2"),
            patch("16.1"),
            Filter::default(),
            &before,
            &after,
            &changes,
        );

        assert_eq!(report.champions_changed, 2);
        assert_eq!(report.largest_gain.unwrap().champion, "Jinx");
        assert_eq!(report.largest_drop.unwrap().champion, "Ahri");
        assert_eq!(report.impacts[0].champion, "Lux");
    }
}
