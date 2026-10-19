use core_types::{
    ChampionStats, Effectiveness, Filter, HistoryPoint, MIN_GAMES, MetaShift, Patch, PatchReport,
    champion_key, effectiveness, impacts, meta_shift, trailing_mean,
};

use crate::{
    Connection, EngineError, Result, balance_changes, champion_stats, find_champion, stats_query,
};

/// How many patches the smoothed win rate in a champion's history averages over.
const SMOOTHING_WINDOW: usize = 3;

/// Patches that have at least one match in the filtered slice, oldest first. Comparisons are
/// always between neighbours in this list, so a slice with a gap in its data is compared across
/// the gap rather than against an empty patch.
fn comparable_patches(conn: &Connection, filter: &Filter) -> Result<Vec<Patch>> {
    let mut stmt = conn.prepare(
        "SELECT p.major, p.minor FROM patches p
         WHERE EXISTS (
             SELECT 1 FROM matches m
             WHERE m.patch_id = p.id
               AND (:region IS NULL OR m.region = lower(:region))
               AND (:tier IS NULL OR m.tier = :tier))
         ORDER BY p.major, p.minor",
    )?;
    let rows = stmt.query_map(
        rusqlite::named_params! {
            ":region": filter.region,
            ":tier": filter.tier.map(|tier| tier.as_str()),
        },
        |row| {
            Ok(Patch {
                major: row.get(0)?,
                minor: row.get(1)?,
            })
        },
    )?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Compares a patch with the one before it: how each champion's win, pick and ban rates moved,
/// whether the win-rate change is more than noise, and which balance changes Riot made.
pub fn patch_report(conn: &Connection, patch: Patch, filter: &Filter) -> Result<PatchReport> {
    let patches = comparable_patches(conn, filter)?;
    let position = patches
        .iter()
        .position(|&candidate| candidate == patch)
        .ok_or(EngineError::PatchNotFound(patch))?;
    let previous_index = position
        .checked_sub(1)
        .ok_or(EngineError::NoPreviousPatch(patch))?;
    let previous = patches[previous_index];
    let after = champion_stats(conn, patch, filter)?;
    let before = champion_stats(conn, previous, filter)?;
    let changes = balance_changes(conn, patch)?;
    Ok(PatchReport::new(
        patch,
        previous,
        filter.clone(),
        &before,
        &after,
        &changes,
    ))
}

/// A champion's results in every patch where it was played, oldest first, with a smoothed win
/// rate and the balance changes made to it in each patch.
pub fn champion_history(
    conn: &Connection,
    champion: &str,
    filter: &Filter,
) -> Result<Vec<HistoryPoint>> {
    let champion_id = find_champion(conn, champion)?;
    let mut points = Vec::new();
    for patch in comparable_patches(conn, filter)? {
        let played = stats_query(conn, patch, filter, Some(champion_id))?
            .into_iter()
            .find(|stats| stats.games > 0);
        if let Some(stats) = played {
            let changes = balance_changes(conn, patch)?
                .into_iter()
                .filter(|change| champion_key(&change.champion) == champion_key(champion))
                .collect();
            points.push(HistoryPoint {
                patch,
                stats,
                smoothed_win_rate: None,
                changes,
            });
        }
    }
    let win_rates: Vec<f64> = points
        .iter()
        .filter_map(|point| point.stats.win_rate())
        .collect();
    for (point, smoothed) in points
        .iter_mut()
        .zip(trailing_mean(&win_rates, SMOOTHING_WINDOW))
    {
        point.smoothed_win_rate = Some(smoothed);
    }
    Ok(points)
}

/// Champions in a patch ordered by win rate, best first. Champions with fewer than
/// [`MIN_GAMES`] games are left out because their win rate says little.
pub fn rankings(conn: &Connection, patch: Patch, filter: &Filter) -> Result<Vec<ChampionStats>> {
    let mut ranked: Vec<ChampionStats> = champion_stats(conn, patch, filter)?
        .into_iter()
        .filter(|stats| stats.games >= MIN_GAMES)
        .collect();
    ranked.sort_by(|a, b| {
        b.win_rate()
            .partial_cmp(&a.win_rate())
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.champion.cmp(&b.champion))
    });
    Ok(ranked)
}

/// How far the meta moved between each pair of neighbouring patches, oldest first.
pub fn meta_shifts(conn: &Connection, filter: &Filter) -> Result<Vec<MetaShift>> {
    let mut shifts = Vec::new();
    each_comparison(conn, filter, |patch, previous, before, after| {
        shifts.push(meta_shift(patch, previous, before, after));
        Ok(())
    })?;
    Ok(shifts)
}

/// How often buffs and nerfs moved win rates the way they were meant to, over every patch that
/// has a patch before it to compare with.
pub fn balance_effectiveness(conn: &Connection, filter: &Filter) -> Result<Vec<Effectiveness>> {
    let mut changed = Vec::new();
    each_comparison(conn, filter, |patch, _, before, after| {
        let changes = balance_changes(conn, patch)?;
        changed.extend(
            impacts(before, after, &changes)
                .into_iter()
                .filter(|impact| !impact.changes.is_empty()),
        );
        Ok(())
    })?;
    Ok(effectiveness(&changed))
}

/// Calls `visit(patch, previous, stats before, stats after)` for each pair of neighbouring
/// patches, loading every patch's statistics once.
fn each_comparison(
    conn: &Connection,
    filter: &Filter,
    mut visit: impl FnMut(Patch, Patch, &[ChampionStats], &[ChampionStats]) -> Result<()>,
) -> Result<()> {
    let mut previous: Option<(Patch, Vec<ChampionStats>)> = None;
    for patch in comparable_patches(conn, filter)? {
        let stats = champion_stats(conn, patch, filter)?;
        if let Some((previous_patch, previous_stats)) = &previous {
            visit(patch, *previous_patch, previous_stats, &stats)?;
        }
        previous = Some((patch, stats));
    }
    Ok(())
}
