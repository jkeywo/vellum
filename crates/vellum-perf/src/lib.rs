//! The fleet's cross-runtime measurement contract.
//!
//! Designed to the shape project-phoenix-v2's performance PRD wrote down
//! (its issue #868), without touching that game: a **named scenario**, the
//! **build and runtime provenance** that makes two captures comparable,
//! **metric samples** collected outside the authoritative simulation,
//! **summaries** over them, and **threshold/baseline comparison** that warns
//! before it ever gates. void-and-thunder is the first consumer — live
//! frame sampling and corpus-run measurement — and phoenix implements its
//! workflow on this crate when it picks that work up.
//!
//! The division of honesty this crate keeps, straight from the PRD:
//!
//! - **Performance values are benchmark evidence, not unit-test
//!   assertions.** What is tested here — and what a consumer should test —
//!   is the pure machinery: aggregation, comparison, classification,
//!   formatting, all with fabricated samples.
//! - **Collection stays out of the sim.** A recorder is fed by thin
//!   per-runtime adapters (a frame hook, a harness loop, a boot timer);
//!   nothing here belongs inside authoritative state, and human-readable
//!   logs are never parsed as telemetry.
//! - **Baselines are versioned, reviewable files.** A baseline moves in a
//!   diff someone reads, with tolerances that say how much drift is noise.
//!   Comparison classifies warnings-first: a consumer promotes
//!   [`Verdict::Fail`] to a CI gate only once its methodology is stable.
//!
//! Reports compose with `vellum-corpus` without a crate dependency: a
//! capture's summaries are plain serde data, so a corpus report's `summary`
//! field can carry them alongside outcome tallies.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// What a metric's numbers mean. Units are part of the contract: a baseline
/// in milliseconds compared against a capture in seconds is a category
/// error, and [`compare`] treats it as one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Unit {
    /// Wall-clock milliseconds (frame times, boot times).
    Millis,
    /// Wall-clock seconds (whole-run costs).
    Seconds,
    /// Bytes (artifact sizes, memory).
    Bytes,
    /// Plain counts (entities, draw calls, ticks).
    Count,
    /// Rates per second (simulation ticks, frames).
    PerSecond,
    /// A consumer's own unit, named so two captures can still agree on it.
    Custom(String),
}

/// One metric's samples within a capture.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Series {
    pub unit: Unit,
    pub samples: Vec<f64>,
}

/// The order statistics a series reduces to. Percentiles are
/// nearest-rank over the sorted samples — deterministic for given samples,
/// which is what lets two runs of the pure machinery agree exactly.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    pub count: u64,
    pub min: f64,
    pub max: f64,
    pub mean: f64,
    pub p50: f64,
    pub p95: f64,
    pub p99: f64,
}

/// Summarise samples. Empty input is a summary of zeros rather than a panic:
/// an instrument that crashes on a metric nobody fed is worse than one that
/// reports an empty row.
pub fn summarize(samples: &[f64]) -> Summary {
    if samples.is_empty() {
        return Summary {
            count: 0,
            min: 0.0,
            max: 0.0,
            mean: 0.0,
            p50: 0.0,
            p95: 0.0,
            p99: 0.0,
        };
    }
    let mut sorted: Vec<f64> = samples.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).expect("perf samples are finite"));
    let count = sorted.len();
    let rank = |p: f64| -> f64 {
        // Nearest-rank: ceil(p * n) clamped into the sorted vec.
        let idx = ((p * count as f64).ceil() as usize).clamp(1, count) - 1;
        sorted[idx]
    };
    Summary {
        count: count as u64,
        min: sorted[0],
        max: sorted[count - 1],
        mean: sorted.iter().sum::<f64>() / count as f64,
        p50: rank(0.50),
        p95: rank(0.95),
        p99: rank(0.99),
    }
}

/// Where a capture came from: enough to know whether two captures are
/// comparable, which is the PRD's whole reason for provenance. Free-form
/// strings on purpose — the contract is that they are *present and honest*,
/// not that every game describes a GPU the same way.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    /// Runtime: "native", "wasm-chromium", "headless-ci".
    pub runtime: String,
    /// Build flavour: "release", "dev", "release+lto".
    pub build: String,
    /// Device or host class: "github-ubuntu-runner", "dev-desktop".
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub device: String,
    /// Git rev, content fingerprint — whatever ties the capture to code.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub rev: String,
}

/// A recorder collectors feed. Series are keyed by metric name in a BTreeMap
/// so every rendering of a capture lists metrics in the same order.
#[derive(Debug, Clone, Default)]
pub struct Recorder {
    series: BTreeMap<String, Series>,
}

impl Recorder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record one sample. The first sample fixes the metric's unit; feeding
    /// the same metric in two units is a collector bug worth the panic.
    pub fn sample(&mut self, metric: &str, unit: Unit, value: f64) {
        let series = self.series.entry(metric.to_owned()).or_insert(Series {
            unit: unit.clone(),
            samples: Vec::new(),
        });
        assert!(
            series.unit == unit,
            "metric '{metric}' sampled as {:?} after {:?}",
            unit,
            series.unit
        );
        series.samples.push(value);
    }

    /// Whether anything has been recorded yet.
    pub fn is_empty(&self) -> bool {
        self.series.is_empty()
    }

    /// Close the recorder into a capture for `scenario` under `profile`.
    pub fn finish(self, scenario: &str, profile: Profile) -> Capture {
        let summaries = self
            .series
            .iter()
            .map(|(name, series)| {
                (
                    name.clone(),
                    MetricSummary {
                        unit: series.unit.clone(),
                        summary: summarize(&series.samples),
                    },
                )
            })
            .collect();
        Capture {
            scenario: scenario.to_owned(),
            profile,
            series: self.series,
            summaries,
        }
    }
}

/// A summarised metric inside a capture or baseline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MetricSummary {
    pub unit: Unit,
    pub summary: Summary,
}

/// The measurement contract's artifact: one scenario, one profile, the raw
/// series (the reproducible evidence a regression is diagnosed from) and
/// their summaries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Capture {
    pub scenario: String,
    pub profile: Profile,
    pub series: BTreeMap<String, Series>,
    pub summaries: BTreeMap<String, MetricSummary>,
}

#[cfg(feature = "json")]
impl Capture {
    /// The capture as pretty JSON — the artifact CI stores.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("perf captures serialize")
    }
}

/// How much drift in a metric is noise, and how much is a finding. Ratios
/// are relative to the baseline value: `warn: 0.15` warns beyond ±15%.
/// Warnings-first is the contract — [`Verdict::Fail`] exists so a consumer
/// *can* gate once its methodology is stable, not so it must.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Tolerance {
    pub warn: f64,
    pub fail: f64,
}

impl Default for Tolerance {
    fn default() -> Self {
        // Generous by design: CI runners are noisy neighbours, and a
        // tolerance that cries wolf gets deleted rather than obeyed.
        Self {
            warn: 0.25,
            fail: 1.0,
        }
    }
}

/// One metric's expectation in a baseline: the value a capture's summary
/// statistic is held near, and how near.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Expectation {
    pub unit: Unit,
    /// Which summary statistic is compared. Mean flatters and max panics;
    /// p95 is the PRD-shaped default for time-like metrics.
    pub statistic: Statistic,
    pub expected: f64,
    #[serde(default)]
    pub tolerance: Tolerance,
}

/// The summary statistic an expectation reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Statistic {
    Mean,
    P50,
    P95,
    P99,
    Max,
    Count,
}

impl Statistic {
    pub fn read(self, summary: &Summary) -> f64 {
        match self {
            Statistic::Mean => summary.mean,
            Statistic::P50 => summary.p50,
            Statistic::P95 => summary.p95,
            Statistic::P99 => summary.p99,
            Statistic::Max => summary.max,
            Statistic::Count => summary.count as f64,
        }
    }
}

/// A versioned, reviewable set of expectations for one scenario. Stored in
/// the consumer's own format (RON in the games); moving one is a diff a
/// human reads.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Baseline {
    pub scenario: String,
    pub expectations: BTreeMap<String, Expectation>,
}

/// What comparing one metric against its expectation concluded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Verdict {
    Pass,
    /// Outside the warn tolerance: worth a look, never a red build by
    /// itself.
    Warn,
    /// Outside the fail tolerance: gate-worthy once (and only once) the
    /// consumer has decided its numbers are stable enough to gate on.
    Fail,
    /// The capture has no such metric, or the units disagree — a contract
    /// breach rather than a performance finding.
    Incomparable,
}

/// One metric's comparison outcome.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    pub metric: String,
    pub verdict: Verdict,
    pub expected: f64,
    pub got: f64,
    /// Signed drift relative to the expectation (+0.30 = 30% over).
    pub drift: f64,
}

/// Compare a capture against a baseline, metric by metric, in metric order.
///
/// Every baselined metric yields exactly one finding; capture metrics with
/// no expectation yield none (new instruments appear before anyone has an
/// opinion about them, and that must not be a failure).
pub fn compare(capture: &Capture, baseline: &Baseline) -> Vec<Finding> {
    baseline
        .expectations
        .iter()
        .map(|(metric, expectation)| {
            let Some(measured) = capture.summaries.get(metric) else {
                return Finding {
                    metric: metric.clone(),
                    verdict: Verdict::Incomparable,
                    expected: expectation.expected,
                    got: f64::NAN,
                    drift: f64::NAN,
                };
            };
            if measured.unit != expectation.unit {
                return Finding {
                    metric: metric.clone(),
                    verdict: Verdict::Incomparable,
                    expected: expectation.expected,
                    got: expectation.statistic.read(&measured.summary),
                    drift: f64::NAN,
                };
            }
            let got = expectation.statistic.read(&measured.summary);
            let drift = if expectation.expected == 0.0 {
                if got == 0.0 {
                    0.0
                } else {
                    f64::INFINITY
                }
            } else {
                (got - expectation.expected) / expectation.expected
            };
            let magnitude = drift.abs();
            let verdict = if magnitude > expectation.tolerance.fail {
                Verdict::Fail
            } else if magnitude > expectation.tolerance.warn {
                Verdict::Warn
            } else {
                Verdict::Pass
            };
            Finding {
                metric: metric.clone(),
                verdict,
                expected: expectation.expected,
                got,
                drift,
            }
        })
        .collect()
}

/// The worst verdict in a set of findings — what a consumer's single
/// warn-or-gate decision reads.
pub fn worst(findings: &[Finding]) -> Verdict {
    findings
        .iter()
        .map(|finding| finding.verdict)
        .max_by_key(|verdict| match verdict {
            Verdict::Pass => 0,
            Verdict::Warn => 1,
            Verdict::Fail => 2,
            Verdict::Incomparable => 3,
        })
        .unwrap_or(Verdict::Pass)
}

/// Findings as fixed-width text, one line per metric — what a CI log or a
/// job summary shows a human.
pub fn render(findings: &[Finding]) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    for finding in findings {
        let _ = writeln!(
            out,
            "{:<12} {:<32} expected {:>12.3}  got {:>12.3}  drift {:>+7.1}%",
            format!("{:?}", finding.verdict).to_lowercase(),
            finding.metric,
            finding.expected,
            finding.got,
            finding.drift * 100.0
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fabricated() -> Vec<f64> {
        // 1..=100, shuffled enough to prove sorting happens.
        let mut v: Vec<f64> = (1..=100).map(f64::from).collect();
        v.reverse();
        v
    }

    #[test]
    fn summaries_are_exact_order_statistics() {
        let summary = summarize(&fabricated());
        assert_eq!(summary.count, 100);
        assert_eq!(summary.min, 1.0);
        assert_eq!(summary.max, 100.0);
        assert_eq!(summary.mean, 50.5);
        assert_eq!(summary.p50, 50.0, "nearest-rank, not interpolated");
        assert_eq!(summary.p95, 95.0);
        assert_eq!(summary.p99, 99.0);
    }

    #[test]
    fn an_empty_series_summarises_to_zeros_not_a_panic() {
        let summary = summarize(&[]);
        assert_eq!(summary.count, 0);
        assert_eq!(summary.max, 0.0);
    }

    #[test]
    fn recorders_keep_metric_order_deterministic() {
        let mut recorder = Recorder::new();
        recorder.sample("zeta", Unit::Millis, 1.0);
        recorder.sample("alpha", Unit::Count, 2.0);
        let capture = recorder.finish("fabricated", Profile::default());
        let names: Vec<&String> = capture.summaries.keys().collect();
        assert_eq!(names, ["alpha", "zeta"], "BTreeMap order, not feed order");
    }

    #[test]
    #[should_panic(expected = "sampled as")]
    fn a_unit_change_mid_metric_is_a_collector_bug() {
        let mut recorder = Recorder::new();
        recorder.sample("frame", Unit::Millis, 16.0);
        recorder.sample("frame", Unit::Seconds, 0.016);
    }

    fn capture_with(metric: &str, unit: Unit, samples: &[f64]) -> Capture {
        let mut recorder = Recorder::new();
        for &s in samples {
            recorder.sample(metric, unit.clone(), s);
        }
        recorder.finish("fabricated", Profile::default())
    }

    fn expectation(expected: f64, warn: f64, fail: f64) -> Expectation {
        Expectation {
            unit: Unit::Millis,
            statistic: Statistic::P95,
            expected,
            tolerance: Tolerance { warn, fail },
        }
    }

    #[test]
    fn comparison_classifies_warnings_first() {
        let capture = capture_with("frame", Unit::Millis, &fabricated());
        let mut baseline = Baseline {
            scenario: "fabricated".into(),
            expectations: BTreeMap::new(),
        };
        // p95 of the fabricated series is 95.
        baseline
            .expectations
            .insert("frame".into(), expectation(100.0, 0.10, 0.50));
        let findings = compare(&capture, &baseline);
        assert_eq!(findings[0].verdict, Verdict::Pass, "{findings:?}");

        baseline
            .expectations
            .insert("frame".into(), expectation(80.0, 0.10, 0.50));
        assert_eq!(compare(&capture, &baseline)[0].verdict, Verdict::Warn);

        baseline
            .expectations
            .insert("frame".into(), expectation(40.0, 0.10, 0.50));
        let findings = compare(&capture, &baseline);
        assert_eq!(findings[0].verdict, Verdict::Fail);
        assert!(findings[0].drift > 1.0, "95 vs 40 is +137%");
    }

    #[test]
    fn improvements_beyond_tolerance_also_surface() {
        // A metric that got *better* than the tolerance still warns: a large
        // unexplained improvement is a methodology question (did the work
        // stop happening?), and silently absorbing it would let the baseline
        // rot.
        let capture = capture_with("frame", Unit::Millis, &fabricated());
        let baseline = Baseline {
            scenario: "fabricated".into(),
            expectations: [("frame".to_string(), expectation(200.0, 0.10, 2.0))]
                .into_iter()
                .collect(),
        };
        assert_eq!(compare(&capture, &baseline)[0].verdict, Verdict::Warn);
    }

    #[test]
    fn missing_metrics_and_unit_mismatches_are_incomparable() {
        let capture = capture_with("frame", Unit::Millis, &[16.0]);
        let baseline = Baseline {
            scenario: "fabricated".into(),
            expectations: [
                ("absent".to_string(), expectation(1.0, 0.1, 0.5)),
                (
                    "frame".to_string(),
                    Expectation {
                        unit: Unit::Seconds,
                        statistic: Statistic::P95,
                        expected: 0.016,
                        tolerance: Tolerance::default(),
                    },
                ),
            ]
            .into_iter()
            .collect(),
        };
        let findings = compare(&capture, &baseline);
        assert!(findings.iter().all(|f| f.verdict == Verdict::Incomparable));
        assert_eq!(worst(&findings), Verdict::Incomparable);
    }

    #[test]
    fn unbaselined_capture_metrics_are_not_findings() {
        // New instruments appear before anyone has an opinion about them.
        let capture = capture_with("brand-new", Unit::Count, &[1.0]);
        let findings = compare(&capture, &Baseline::default());
        assert!(findings.is_empty());
        assert_eq!(worst(&findings), Verdict::Pass);
    }

    #[test]
    fn baselines_round_trip_through_ron() {
        // Baselines are versioned files in the consumer's own format; RON is
        // what the first consumer authors.
        let baseline = Baseline {
            scenario: "skirmish".into(),
            expectations: [("sim-step".to_string(), expectation(0.5, 0.25, 1.0))]
                .into_iter()
                .collect(),
        };
        let text = ron::ser::to_string_pretty(&baseline, Default::default()).unwrap();
        let back: Baseline = ron::from_str(&text).unwrap();
        assert_eq!(back, baseline);
    }

    #[test]
    fn rendering_is_one_line_per_finding() {
        let capture = capture_with("frame", Unit::Millis, &fabricated());
        let baseline = Baseline {
            scenario: "fabricated".into(),
            expectations: [("frame".to_string(), expectation(95.0, 0.1, 0.5))]
                .into_iter()
                .collect(),
        };
        let text = render(&compare(&capture, &baseline));
        assert_eq!(text.lines().count(), 1);
        assert!(text.contains("pass"));
        assert!(text.contains("frame"));
    }
}
