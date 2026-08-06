//! Doom-loop detection: periodic tool-call cycles and repeated assistant text.
//!
//! # What counts as a loop
//!
//! An agent is stuck when it keeps performing the *same actions* without the
//! actions changing. The detector only ever sees `name:args_fingerprint` keys,
//! so "no progress" is defined as: **a block of k consecutive tool keys repeats
//! verbatim, back to back, enough times**.
//!
//! That definition is what separates a doom loop from ordinary iteration:
//!
//! - `edit(file, v1) -> test() -> edit(file, v2) -> test() -> ...` is a healthy
//!   edit/test cycle. The tool *names* alternate, but the edit arguments differ
//!   every round, so no block repeats verbatim and nothing fires.
//! - `read(file) -> edit(file, X) -> read(file) -> edit(file, X) -> ...` is a
//!   doom loop: byte-identical arguments every round means the agent is redoing
//!   work it already did.
//!
//! Changing arguments are the progress signal. Identical arguments are the
//! absence of one.
//!
//! # Conservatism
//!
//! Longer cycles need more observed rounds before firing, and the minimal
//! period always wins so diagnostics report the tightest pattern that explains
//! the sequence.

use std::collections::VecDeque;
use std::fmt;

/// Tool keys retained for periodicity analysis.
///
/// Sized so the longest supported cycle (period 3, 4 rounds) fits exactly.
const TOOL_WINDOW: usize = 12;
/// Assistant text blocks retained for periodicity analysis.
const TEXT_WINDOW: usize = 6;
/// Longest tool cycle length we look for.
const TOOL_MAX_PERIOD: usize = 3;
/// Longest text cycle length we look for.
const TEXT_MAX_PERIOD: usize = 2;
/// Assistant text shorter than this is boilerplate ("Ok.", "Let me retry.")
/// and repeats legitimately, so it never triggers on its own.
const TEXT_MIN_SIGNIFICANT_LEN: usize = 20;
/// Longest pattern element kept in a [`DoomLoopReason`] for display.
const PATTERN_ELEMENT_CAP: usize = 80;

/// Which observation stream produced a doom-loop verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoomLoopSignal {
    /// A cycle of tool invocations (name + argument fingerprint).
    Tool,
    /// A cycle of assistant text blocks.
    Text,
}

impl DoomLoopSignal {
    /// Stable machine-readable tag, suitable for telemetry or error payloads.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tool => "tool",
            Self::Text => "text",
        }
    }
}

impl fmt::Display for DoomLoopSignal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Tool => "tool calls",
            Self::Text => "assistant text",
        })
    }
}

/// Explains *why* the detector believes the agent is stuck.
///
/// `period == 1` is plain consecutive repetition; `period == 2` is an A/B/A/B
/// alternation, and so on. `repeats` counts complete rounds of the cycle
/// observed back to back, including the most recent one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoomLoopReason {
    /// Stream the cycle was found in.
    pub signal: DoomLoopSignal,
    /// Length of the repeating block.
    pub period: usize,
    /// Number of back-to-back rounds of that block.
    pub repeats: usize,
    /// The repeating block itself, one entry per step, truncated for display.
    pub pattern: Vec<String>,
}

impl fmt::Display for DoomLoopReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} repeated a {}-step cycle {} times without change: [{}]",
            self.signal,
            self.period,
            self.repeats,
            self.pattern.join(" -> ")
        )
    }
}

/// Tracks recent tool invocations and assistant text for loop detection.
#[derive(Debug)]
pub struct DoomLoopDetector {
    recent_tools: VecDeque<String>,
    recent_texts: VecDeque<String>,
    tool_window: usize,
    text_window: usize,
}

impl Default for DoomLoopDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl DoomLoopDetector {
    pub fn new() -> Self {
        Self {
            recent_tools: VecDeque::new(),
            recent_texts: VecDeque::new(),
            tool_window: TOOL_WINDOW,
            text_window: TEXT_WINDOW,
        }
    }

    pub fn observe_tool(&mut self, name: &str, args_fingerprint: &str) {
        let key = format!("{name}:{args_fingerprint}");
        self.recent_tools.push_back(key);
        while self.recent_tools.len() > self.tool_window {
            self.recent_tools.pop_front();
        }
    }

    pub fn observe_text(&mut self, text: &str) {
        let normalized = text.trim().chars().take(200).collect::<String>();
        if normalized.is_empty() {
            return;
        }
        self.recent_texts.push_back(normalized);
        while self.recent_texts.len() > self.text_window {
            self.recent_texts.pop_front();
        }
    }

    /// Returns true when a repeating cycle of tool calls or assistant text has
    /// been observed. Thin wrapper over [`Self::diagnose`].
    pub fn is_doom_loop(&self) -> bool {
        self.diagnose().is_some()
    }

    /// Returns the repeating pattern behind the verdict, if any.
    ///
    /// Tool cycles are checked before text cycles because they are the stronger
    /// signal: identical arguments mean identical actions, whereas identical
    /// prose can still accompany differing work.
    pub fn diagnose(&self) -> Option<DoomLoopReason> {
        self.diagnose_tools().or_else(|| self.diagnose_texts())
    }

    fn diagnose_tools(&self) -> Option<DoomLoopReason> {
        let seq: Vec<&str> = self.recent_tools.iter().map(String::as_str).collect();
        let (period, repeats) = find_periodic_cycle(&seq, TOOL_MAX_PERIOD, required_tool_repeats)?;
        Some(DoomLoopReason {
            signal: DoomLoopSignal::Tool,
            period,
            repeats,
            pattern: truncate_pattern(&seq[seq.len() - period..]),
        })
    }

    fn diagnose_texts(&self) -> Option<DoomLoopReason> {
        let seq: Vec<&str> = self.recent_texts.iter().map(String::as_str).collect();
        let (period, repeats) = find_periodic_cycle(&seq, TEXT_MAX_PERIOD, required_text_repeats)?;
        let block = &seq[seq.len() - period..];
        // Short blocks ("Ok.", "Trying again.") repeat for benign reasons; the
        // cycle only counts if at least one step carries real content.
        if !block.iter().any(|t| t.len() > TEXT_MIN_SIGNIFICANT_LEN) {
            return None;
        }
        Some(DoomLoopReason {
            signal: DoomLoopSignal::Text,
            period,
            repeats,
            pattern: truncate_pattern(block),
        })
    }
}

/// Rounds of a `period`-length tool cycle required before we call it a loop.
///
/// Period 1 keeps the historical threshold of 3. Longer cycles demand more
/// rounds: alternation can legitimately arise from a poll/check pair, so we
/// want the evidence to be unambiguous before burning the run.
fn required_tool_repeats(period: usize) -> usize {
    match period {
        1 => 3,
        _ => 4,
    }
}

/// Rounds of a `period`-length text cycle required before we call it a loop.
fn required_text_repeats(_period: usize) -> usize {
    3
}

/// Finds the shortest block at the tail of `seq` that repeats back to back at
/// least `required(period)` times.
///
/// Returns `(period, repeats)`. Periods are scanned in ascending order so the
/// minimal (and therefore most explanatory) period wins; non-primitive blocks
/// such as `[A, A]` are skipped because their true period is smaller and was
/// already considered.
fn find_periodic_cycle(
    seq: &[&str],
    max_period: usize,
    required: fn(usize) -> usize,
) -> Option<(usize, usize)> {
    let n = seq.len();
    for period in 1..=max_period.min(n) {
        let block = &seq[n - period..];
        if period > 1 && !is_primitive(block) {
            continue;
        }
        let mut repeats = 1usize;
        while (repeats + 1) * period <= n {
            let end = n - repeats * period;
            if &seq[end - period..end] == block {
                repeats += 1;
            } else {
                break;
            }
        }
        if repeats >= required(period) {
            return Some((period, repeats));
        }
    }
    None
}

/// True when `block` is not itself a repetition of a shorter block.
fn is_primitive(block: &[&str]) -> bool {
    let k = block.len();
    (1..k).all(|d| k % d != 0 || (0..k).any(|i| block[i] != block[i % d]))
}

fn truncate_pattern(block: &[&str]) -> Vec<String> {
    block
        .iter()
        .map(|entry| {
            if entry.chars().count() > PATTERN_ELEMENT_CAP {
                let head: String = entry.chars().take(PATTERN_ELEMENT_CAP).collect();
                format!("{head}…")
            } else {
                (*entry).to_string()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_repeated_tool_signature() {
        let mut d = DoomLoopDetector::new();
        d.observe_tool("read_file", "path=/a");
        d.observe_tool("read_file", "path=/a");
        assert!(!d.is_doom_loop());
        d.observe_tool("read_file", "path=/a");
        assert!(d.is_doom_loop());
    }

    #[test]
    fn detects_repeated_long_text() {
        let mut d = DoomLoopDetector::new();
        let text = "this is a long enough repeated answer block";
        d.observe_text(text);
        d.observe_text(text);
        assert!(!d.is_doom_loop());
        d.observe_text(text);
        assert!(d.is_doom_loop());
    }

    // ---------- positive: periodic cycles ----------

    #[test]
    fn detects_period_two_tool_cycle() {
        let mut d = DoomLoopDetector::new();
        for round in 0..3 {
            d.observe_tool("read_file", "path=/a");
            d.observe_tool("write_file", "path=/a,body=X");
            assert!(!d.is_doom_loop(), "fired too early at round {round}");
        }
        // Fourth round completes the required repeats.
        d.observe_tool("read_file", "path=/a");
        d.observe_tool("write_file", "path=/a,body=X");
        let reason = d.diagnose().expect("period-2 cycle should be detected");
        assert_eq!(reason.signal, DoomLoopSignal::Tool);
        assert_eq!(reason.period, 2);
        assert_eq!(reason.repeats, 4);
        assert_eq!(
            reason.pattern,
            vec!["read_file:path=/a", "write_file:path=/a,body=X"]
        );
    }

    #[test]
    fn detects_period_three_tool_cycle() {
        let mut d = DoomLoopDetector::new();
        for _ in 0..3 {
            d.observe_tool("read_file", "path=/a");
            d.observe_tool("run_tests", "suite=unit");
            d.observe_tool("write_file", "path=/a,body=X");
        }
        assert!(!d.is_doom_loop(), "three rounds must not be enough");
        d.observe_tool("read_file", "path=/a");
        d.observe_tool("run_tests", "suite=unit");
        d.observe_tool("write_file", "path=/a,body=X");
        let reason = d.diagnose().expect("period-3 cycle should be detected");
        assert_eq!(reason.period, 3);
        assert_eq!(reason.repeats, 4);
    }

    #[test]
    fn detects_alternating_long_text() {
        let mut d = DoomLoopDetector::new();
        let a = "I will now inspect the failing module once more";
        let b = "The compiler still reports the same borrow checker error";
        for _ in 0..2 {
            d.observe_text(a);
            d.observe_text(b);
        }
        assert!(!d.is_doom_loop());
        d.observe_text(a);
        d.observe_text(b);
        let reason = d
            .diagnose()
            .expect("period-2 text cycle should be detected");
        assert_eq!(reason.signal, DoomLoopSignal::Text);
        assert_eq!(reason.period, 2);
        assert_eq!(reason.repeats, 3);
    }

    // ---------- negative: legitimate repetition ----------

    #[test]
    fn productive_edit_test_iteration_is_not_a_loop() {
        // The archetypal false positive: edit -> test -> edit -> test.
        // Tool names alternate, but each edit carries different arguments,
        // so no block repeats verbatim.
        let mut d = DoomLoopDetector::new();
        for round in 0..6 {
            d.observe_tool(
                "edit_file",
                &format!("path=/a,old=v{round},new=v{}", round + 1),
            );
            d.observe_tool("run_tests", "suite=unit");
            assert!(
                !d.is_doom_loop(),
                "productive iteration flagged at round {round}: {:?}",
                d.diagnose()
            );
        }
    }

    #[test]
    fn reading_many_different_files_is_not_a_loop() {
        let mut d = DoomLoopDetector::new();
        for i in 0..10 {
            d.observe_tool("read_file", &format!("path=/src/file_{i}.rs"));
            assert!(!d.is_doom_loop(), "distinct reads flagged at file {i}");
        }
    }

    #[test]
    fn three_phase_pipeline_over_distinct_inputs_is_not_a_loop() {
        // read -> transform -> write, repeated across different files: same
        // shape, different arguments. Must survive well past period-3 length.
        let mut d = DoomLoopDetector::new();
        for i in 0..5 {
            d.observe_tool("read_file", &format!("path=/src/{i}.rs"));
            d.observe_tool("format_code", &format!("path=/src/{i}.rs"));
            d.observe_tool("write_file", &format!("path=/src/{i}.rs"));
            assert!(!d.is_doom_loop(), "pipeline flagged at iteration {i}");
        }
    }

    #[test]
    fn short_repeated_text_is_not_a_loop() {
        let mut d = DoomLoopDetector::new();
        for _ in 0..6 {
            d.observe_text("Ok.");
        }
        assert!(!d.is_doom_loop());
        assert!(d.diagnose().is_none());
    }

    #[test]
    fn alternating_short_text_is_not_a_loop() {
        let mut d = DoomLoopDetector::new();
        for _ in 0..3 {
            d.observe_text("Ok.");
            d.observe_text("Retrying.");
        }
        assert!(!d.is_doom_loop());
    }

    #[test]
    fn period_two_below_threshold_does_not_fire() {
        let mut d = DoomLoopDetector::new();
        for _ in 0..3 {
            d.observe_tool("read_file", "path=/a");
            d.observe_tool("write_file", "path=/a");
        }
        assert!(
            !d.is_doom_loop(),
            "3 rounds must stay under the period-2 bar"
        );
    }

    #[test]
    fn broken_cycle_resets_the_verdict() {
        let mut d = DoomLoopDetector::new();
        for _ in 0..3 {
            d.observe_tool("read_file", "path=/a");
            d.observe_tool("write_file", "path=/a");
        }
        // A genuinely new action interrupts the pattern.
        d.observe_tool("run_tests", "suite=unit");
        d.observe_tool("read_file", "path=/a");
        d.observe_tool("write_file", "path=/a");
        assert!(!d.is_doom_loop());
    }

    // ---------- diagnostics ----------

    #[test]
    fn diagnose_prefers_the_minimal_period() {
        // A A A A A is both period-1 (5 repeats) and period-2-ish; the
        // minimal, most explanatory period must be reported.
        let mut d = DoomLoopDetector::new();
        for _ in 0..5 {
            d.observe_tool("read_file", "path=/a");
        }
        let reason = d.diagnose().expect("consecutive repetition detected");
        assert_eq!(reason.period, 1);
        assert_eq!(reason.repeats, 5);
        assert_eq!(reason.pattern, vec!["read_file:path=/a"]);
    }

    #[test]
    fn diagnose_is_none_when_healthy() {
        let mut d = DoomLoopDetector::new();
        d.observe_tool("read_file", "path=/a");
        d.observe_text("a perfectly reasonable single explanation block");
        assert!(d.diagnose().is_none());
        assert!(!d.is_doom_loop());
    }

    #[test]
    fn is_doom_loop_agrees_with_diagnose() {
        let mut d = DoomLoopDetector::new();
        for _ in 0..4 {
            d.observe_tool("read_file", "path=/a");
            d.observe_tool("write_file", "path=/a");
            assert_eq!(d.is_doom_loop(), d.diagnose().is_some());
        }
        assert!(d.is_doom_loop());
    }

    #[test]
    fn reason_renders_a_human_explanation() {
        let mut d = DoomLoopDetector::new();
        for _ in 0..4 {
            d.observe_tool("read_file", "path=/a");
            d.observe_tool("write_file", "path=/a");
        }
        let text = d.diagnose().expect("cycle detected").to_string();
        assert!(text.contains("tool calls"), "unexpected: {text}");
        assert!(text.contains("2-step cycle"), "unexpected: {text}");
        assert!(text.contains("read_file:path=/a"), "unexpected: {text}");
    }

    #[test]
    fn tool_signal_wins_over_text_signal() {
        let mut d = DoomLoopDetector::new();
        let text = "this is a long enough repeated answer block";
        for _ in 0..3 {
            d.observe_text(text);
            d.observe_tool("read_file", "path=/a");
        }
        let reason = d.diagnose().expect("both streams loop");
        assert_eq!(reason.signal, DoomLoopSignal::Tool);
    }

    #[test]
    fn default_matches_new() {
        let d = DoomLoopDetector::default();
        assert_eq!(d.tool_window, TOOL_WINDOW);
        assert_eq!(d.text_window, TEXT_WINDOW);
    }

    // ---------- unit-level helpers ----------

    #[test]
    fn is_primitive_rejects_repeated_blocks() {
        assert!(is_primitive(&["a"]));
        assert!(is_primitive(&["a", "b"]));
        assert!(!is_primitive(&["a", "a"]));
        assert!(is_primitive(&["a", "b", "c"]));
        assert!(!is_primitive(&["a", "a", "a"]));
        assert!(is_primitive(&["a", "b", "a"]));
    }

    #[test]
    fn find_periodic_cycle_counts_rounds() {
        let seq = ["a", "b", "a", "b", "a", "b"];
        assert_eq!(find_periodic_cycle(&seq, 3, |_| 3), Some((2, 3)));
        assert_eq!(find_periodic_cycle(&seq, 3, |_| 4), None);
        // A trailing intruder breaks the tail alignment entirely.
        let broken = ["a", "b", "a", "b", "a", "b", "c"];
        assert_eq!(find_periodic_cycle(&broken, 3, |_| 2), None);
    }
}
