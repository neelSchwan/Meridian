use serde::Serialize;

/// A difference is called significant when its p-value is below this.
pub const ALPHA: f64 = 0.05;

/// Fewest games on each side for which a comparison is attempted. Below this, the normal
/// approximation behind the test is unreliable.
pub const MIN_GAMES: u64 = 30;

/// The z value that leaves 2.5% in each tail of the normal distribution, for 95% intervals.
const Z_95: f64 = 1.959964;

/// The result of comparing two win rates.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ProportionTest {
    /// Later rate minus earlier rate, as a fraction (0.02 is two percentage points).
    pub difference: f64,
    /// 95% confidence interval for `difference`.
    pub ci_low: f64,
    pub ci_high: f64,
    pub z: f64,
    /// Two-sided p-value: how often a difference this large would appear if nothing had changed.
    pub p_value: f64,
}

impl ProportionTest {
    /// True when the p-value is below [`ALPHA`].
    pub fn significant(&self) -> bool {
        self.p_value < ALPHA
    }
}

/// Tests whether a win rate changed between two samples (a two-proportion z-test).
///
/// The test statistic pools both samples because it assumes no change; the interval does not,
/// because it describes the change that was observed. Returns `None` when either side has fewer
/// than [`MIN_GAMES`] games.
pub fn two_proportion_test(
    wins_before: u64,
    games_before: u64,
    wins_after: u64,
    games_after: u64,
) -> Option<ProportionTest> {
    if games_before < MIN_GAMES || games_after < MIN_GAMES {
        return None;
    }
    let (n1, n2) = (games_before as f64, games_after as f64);
    let (p1, p2) = (wins_before as f64 / n1, wins_after as f64 / n2);
    let difference = p2 - p1;

    let pooled = (wins_before + wins_after) as f64 / (n1 + n2);
    let pooled_error = (pooled * (1.0 - pooled) * (1.0 / n1 + 1.0 / n2)).sqrt();
    let z = if pooled_error == 0.0 {
        0.0
    } else {
        difference / pooled_error
    };

    let error = (p1 * (1.0 - p1) / n1 + p2 * (1.0 - p2) / n2).sqrt();
    Some(ProportionTest {
        difference,
        ci_low: difference - Z_95 * error,
        ci_high: difference + Z_95 * error,
        z,
        p_value: erfc(z.abs() / std::f64::consts::SQRT_2),
    })
}

/// The Wilson 95% confidence interval for a win rate. It stays inside 0..1 and behaves well for
/// small samples. `None` when there are no games.
pub fn wilson_interval(wins: u64, games: u64) -> Option<(f64, f64)> {
    if games == 0 {
        return None;
    }
    let n = games as f64;
    let rate = wins as f64 / n;
    let z2 = Z_95 * Z_95;
    let scale = 1.0 + z2 / n;
    let center = (rate + z2 / (2.0 * n)) / scale;
    let half_width = Z_95 * (rate * (1.0 - rate) / n + z2 / (4.0 * n * n)).sqrt() / scale;
    Some((center - half_width, center + half_width))
}

/// Smooths a series: each output is the mean of the value and up to `window - 1` values before it.
pub fn trailing_mean(values: &[f64], window: usize) -> Vec<f64> {
    (0..values.len())
        .map(|end| {
            let slice = &values[(end + 1).saturating_sub(window)..=end];
            slice.iter().sum::<f64>() / slice.len() as f64
        })
        .collect()
}

/// The complementary error function, from Numerical Recipes (`erfcc`). Its relative error is
/// below 1.2e-7 everywhere, far finer than a p-value is ever read.
fn erfc(x: f64) -> f64 {
    let z = x.abs();
    let t = 1.0 / (1.0 + 0.5 * z);
    let polynomial = [
        0.17087277,
        -0.82215223,
        1.48851587,
        -1.13520398,
        0.27886807,
        -0.18628806,
        0.09678418,
        0.37409196,
        1.00002368,
        -1.26551223,
    ]
    .iter()
    .fold(0.0, |acc, coefficient| coefficient + t * acc);
    let positive = t * (-z * z + polynomial).exp();
    if x >= 0.0 { positive } else { 2.0 - positive }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(actual: f64, expected: f64, tolerance: f64) {
        assert!(
            (actual - expected).abs() < tolerance,
            "{actual} vs {expected}"
        );
    }

    #[test]
    fn erfc_matches_known_values() {
        close(erfc(0.0), 1.0, 1e-7);
        close(erfc(1.0), 0.157299207, 1e-7);
        close(erfc(-1.0), 1.842700793, 1e-7);
    }

    #[test]
    fn a_five_point_gain_over_a_thousand_games_each_is_significant() {
        let test = two_proportion_test(500, 1000, 550, 1000).unwrap();
        close(test.difference, 0.05, 1e-12);
        close(test.z, 2.2388, 1e-3);
        close(test.p_value, 0.0252, 1e-3);
        assert!(test.significant());
        assert!(test.ci_low > 0.0);
    }

    #[test]
    fn the_same_gain_over_fifty_games_each_is_noise() {
        let test = two_proportion_test(25, 50, 30, 50).unwrap();
        close(test.difference, 0.10, 1e-12);
        assert!(!test.significant());
        assert!(test.ci_low < 0.0 && test.ci_high > 0.0);
    }

    #[test]
    fn a_small_gain_over_many_games_is_significant() {
        let test = two_proportion_test(50_000, 100_000, 51_500, 100_000).unwrap();
        assert!(test.significant());
        assert!(test.p_value < 1e-9);
    }

    #[test]
    fn no_change_has_a_p_value_of_one() {
        let test = two_proportion_test(500, 1000, 500, 1000).unwrap();
        close(test.p_value, 1.0, 1e-6);
    }

    #[test]
    fn samples_below_the_minimum_are_not_tested() {
        assert!(two_proportion_test(10, MIN_GAMES - 1, 500, 1000).is_none());
        assert!(two_proportion_test(500, 1000, 10, MIN_GAMES - 1).is_none());
    }

    #[test]
    fn an_all_winning_sample_does_not_divide_by_zero() {
        let test = two_proportion_test(100, 100, 100, 100).unwrap();
        close(test.p_value, 1.0, 1e-6);
    }

    #[test]
    fn wilson_interval_narrows_with_more_games() {
        let (low, high) = wilson_interval(50, 100).unwrap();
        close(low, 0.4038, 1e-3);
        close(high, 0.5962, 1e-3);
        let (low, high) = wilson_interval(5_000, 10_000).unwrap();
        assert!(high - low < 0.025);
        assert!(wilson_interval(0, 0).is_none());
    }

    #[test]
    fn wilson_interval_stays_within_bounds_at_the_extremes() {
        let (low, high) = wilson_interval(10, 10).unwrap();
        assert!(low > 0.0 && high <= 1.0 + 1e-12);
    }

    #[test]
    fn trailing_mean_uses_only_earlier_values() {
        assert_eq!(
            trailing_mean(&[1.0, 3.0, 5.0, 7.0], 2),
            [1.0, 2.0, 4.0, 6.0]
        );
        assert_eq!(trailing_mean(&[4.0], 3), [4.0]);
        assert!(trailing_mean(&[], 3).is_empty());
    }
}
