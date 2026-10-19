use core_types::{
    ChampionStats, Effectiveness, Filter, HistoryPoint, Impact, MetaShift, PatchReport,
    PatchSummary,
};
use std::fmt::Write;

/// How many champions Riot did not change to list as unexplained movers.
const UNEXPLAINED_LIMIT: usize = 10;

/// Renders a patch report as Markdown: a summary, a table of the champions Riot changed with
/// the evidence for each, and the significant movers that no balance change explains.
pub fn render_markdown(report: &PatchReport) -> String {
    let mut out = String::new();
    // Writing to a String cannot fail.
    let _ = write_markdown(&mut out, report);
    out
}

fn write_markdown(out: &mut String, report: &PatchReport) -> std::fmt::Result {
    writeln!(out, "# Patch {} impact report\n", report.patch)?;
    writeln!(
        out,
        "Compared with patch {} ({} matches before, {} after). Data: {}.\n",
        report.previous,
        report.matches_before,
        report.matches_after,
        describe_filter(&report.filter)
    )?;
    writeln!(out, "- Champions changed: {}", report.champions_changed)?;
    if let Some(mover) = &report.largest_gain {
        writeln!(
            out,
            "- Largest gain: {} {}",
            mover.champion,
            points(Some(mover.win_rate_delta))
        )?;
    }
    if let Some(mover) = &report.largest_drop {
        writeln!(
            out,
            "- Largest drop: {} {}",
            mover.champion,
            points(Some(mover.win_rate_delta))
        )?;
    }
    let shift = &report.meta_shift;
    writeln!(
        out,
        "- Meta shift: win rates moved {} and pick rates {} on average, across {} champions\n",
        points_unsigned(shift.mean_abs_win_rate_delta),
        points_unsigned(shift.mean_abs_pick_rate_delta),
        shift.champions_compared
    )?;

    let changed: Vec<&Impact> = report
        .impacts
        .iter()
        .filter(|i| !i.changes.is_empty())
        .collect();
    writeln!(out, "## Champions Riot changed\n")?;
    if changed.is_empty() {
        writeln!(out, "No balance changes are stored for this patch.\n")?;
    } else {
        write_table(out, &changed, true)?;
    }

    let unexplained: Vec<&Impact> = report
        .impacts
        .iter()
        .filter(|impact| impact.changes.is_empty() && impact.significant())
        .take(UNEXPLAINED_LIMIT)
        .collect();
    if !unexplained.is_empty() {
        writeln!(out, "## Significant movers Riot did not change\n")?;
        write_table(out, &unexplained, false)?;
    }

    writeln!(
        out,
        "Win-rate changes are associations, not proof that a change caused them: other champions, \
         items and the player population also shift between patches. \"Within noise\" means the \
         difference is no larger than chance alone would often produce (p >= 0.05)."
    )
}

fn write_table(out: &mut String, rows: &[&Impact], with_changes: bool) -> std::fmt::Result {
    let changes_header = if with_changes { " Changes |" } else { "" };
    let changes_rule = if with_changes { " --- |" } else { "" };
    writeln!(
        out,
        "| Champion |{changes_header} Win rate | Change | 95% CI | p | Pick rate | Result |"
    )?;
    writeln!(
        out,
        "| --- |{changes_rule} --- | --- | --- | --- | --- | --- |"
    )?;
    for impact in rows {
        let changes_cell = if with_changes {
            format!(" {} |", describe_changes(impact))
        } else {
            String::new()
        };
        writeln!(
            out,
            "| {} |{changes_cell} {} to {} | {} | {} | {} | {} to {} | {} |",
            impact.champion,
            percent(impact.before.win_rate()),
            percent(impact.after.win_rate()),
            points(impact.win_rate_delta),
            interval(impact),
            p_value(impact),
            percent(Some(impact.before.pick_rate())),
            percent(Some(impact.after.pick_rate())),
            verdict(impact),
        )?;
    }
    writeln!(out)
}

fn describe_filter(filter: &Filter) -> String {
    let region = filter.region.as_deref().unwrap_or("all regions");
    let tier = filter
        .tier
        .map_or("all tiers".to_string(), |tier| tier.to_string());
    let role = filter
        .role
        .map_or("all roles".to_string(), |role| role.to_string());
    format!("{region}, {tier}, {role}")
}

fn describe_changes(impact: &Impact) -> String {
    let kind = impact.kind.map_or("", |kind| kind.as_str());
    format!("{kind} ({})", impact.changes.len())
}

/// What the evidence says about one champion, in a few words.
fn verdict(impact: &Impact) -> &'static str {
    match (impact.test, impact.moved_as_intended) {
        (None, _) if impact.win_rate_delta.is_none() => "not played in both patches",
        (None, _) => "too few games to test",
        (Some(test), _) if !test.significant() => "within noise",
        (Some(_), Some(true)) => "moved as intended",
        (Some(_), Some(false)) => "moved against intent",
        (Some(_), None) => "significant change",
    }
}

fn percent(rate: Option<f64>) -> String {
    rate.map_or("n/a".to_string(), |rate| format!("{:.1}%", rate * 100.0))
}

/// A signed change in percentage points, such as `+2.4pp`.
fn points(delta: Option<f64>) -> String {
    delta.map_or("n/a".to_string(), |delta| {
        format!("{:+.1}pp", delta * 100.0)
    })
}

fn points_unsigned(delta: f64) -> String {
    format!("{:.1}pp", delta * 100.0)
}

fn interval(impact: &Impact) -> String {
    impact.test.map_or("n/a".to_string(), |test| {
        format!(
            "{} to {}",
            points(Some(test.ci_low)),
            points(Some(test.ci_high))
        )
    })
}

fn p_value(impact: &Impact) -> String {
    impact.test.map_or("n/a".to_string(), |test| {
        if test.p_value < 0.001 {
            "<0.001".to_string()
        } else {
            format!("{:.3}", test.p_value)
        }
    })
}

/// Renders every champion in a patch report as CSV, one row each, for spreadsheets. Rates are
/// fractions (0.512 is 51.2%). Empty cells mean the value could not be computed.
pub fn render_csv(report: &PatchReport) -> String {
    let mut out = String::from(
        "champion,changed,kind,games_before,games_after,win_rate_before,win_rate_after,\
         win_rate_delta,ci_low,ci_high,p_value,significant,pick_rate_before,pick_rate_after,\
         ban_rate_before,ban_rate_after\n",
    );
    for impact in &report.impacts {
        let cells = [
            csv_text(&impact.champion),
            (!impact.changes.is_empty()).to_string(),
            impact.kind.map_or(String::new(), |kind| kind.to_string()),
            impact.before.games.to_string(),
            impact.after.games.to_string(),
            csv_number(impact.before.win_rate()),
            csv_number(impact.after.win_rate()),
            csv_number(impact.win_rate_delta),
            csv_number(impact.test.map(|test| test.ci_low)),
            csv_number(impact.test.map(|test| test.ci_high)),
            csv_number(impact.test.map(|test| test.p_value)),
            impact.significant().to_string(),
            csv_number(Some(impact.before.pick_rate())),
            csv_number(Some(impact.after.pick_rate())),
            csv_number(Some(impact.before.ban_rate())),
            csv_number(Some(impact.after.ban_rate())),
        ];
        out.push_str(&cells.join(","));
        out.push('\n');
    }
    out
}

fn csv_number(value: Option<f64>) -> String {
    value.map_or(String::new(), |value| format!("{value:.6}"))
}

/// Quotes a text cell when it contains a comma, quote or line break.
fn csv_text(text: &str) -> String {
    if text.contains([',', '"', '\n']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text.to_string()
    }
}

/// Renders champion statistics (for rankings) as aligned text columns.
pub fn render_stats_table(rows: &[ChampionStats]) -> String {
    let mut out = String::from("rank  champion         games  win rate  pick rate  ban rate\n");
    for (index, row) in rows.iter().enumerate() {
        let _ = writeln!(
            out,
            "{:>4}  {:<14} {:>6}  {:>8}  {:>9}  {:>8}",
            index + 1,
            row.champion,
            row.games,
            percent(row.win_rate()),
            percent(Some(row.pick_rate())),
            percent(Some(row.ban_rate())),
        );
    }
    out
}

/// Renders a champion's history as aligned text columns, marking patches that changed it.
pub fn render_history(points: &[HistoryPoint]) -> String {
    let mut out = String::from(
        "patch   games  win rate  95% CI           trend  pick rate  ban rate  changes\n",
    );
    for point in points {
        let stats = &point.stats;
        let range = stats
            .win_rate()
            .zip(core_types::wilson_interval(stats.wins, stats.games))
            .map_or("n/a".to_string(), |(_, (low, high))| {
                format!("{} to {}", percent(Some(low)), percent(Some(high)))
            });
        let _ = writeln!(
            out,
            "{:<6} {:>6}  {:>8}  {:<15} {:>6}  {:>9}  {:>8}  {}",
            point.patch,
            stats.games,
            percent(stats.win_rate()),
            range,
            percent(point.smoothed_win_rate),
            percent(Some(stats.pick_rate())),
            percent(Some(stats.ban_rate())),
            if point.changes.is_empty() {
                String::new()
            } else {
                format!("{} changes", point.changes.len())
            },
        );
    }
    out
}

/// Renders how much the meta moved at each patch.
pub fn render_shifts(shifts: &[MetaShift]) -> String {
    let mut out = String::from("patch   vs      champions  win-rate move  pick-rate move\n");
    for shift in shifts {
        let _ = writeln!(
            out,
            "{:<6}  {:<6}  {:>9}  {:>13}  {:>14}",
            shift.patch,
            shift.previous,
            shift.champions_compared,
            points_unsigned(shift.mean_abs_win_rate_delta),
            points_unsigned(shift.mean_abs_pick_rate_delta),
        );
    }
    out
}

/// Renders how often buffs and nerfs moved win rates as intended.
pub fn render_effectiveness(rows: &[Effectiveness]) -> String {
    let mut out = String::from("kind  measured  as intended  significantly  toward 50%\n");
    for row in rows {
        let _ = writeln!(
            out,
            "{:<4}  {:>8}  {:>11}  {:>14}  {:>10}",
            row.kind,
            row.measured,
            row.in_intended_direction,
            row.significantly_in_intended_direction,
            row.toward_even,
        );
    }
    out
}

/// Renders the stored patches with how much data each has.
pub fn render_patches(patches: &[PatchSummary]) -> String {
    let mut out = String::from("patch   matches  balance changes\n");
    for summary in patches {
        let _ = writeln!(
            out,
            "{:<6} {:>8}  {:>15}",
            summary.patch, summary.matches, summary.balance_changes
        );
    }
    out
}
