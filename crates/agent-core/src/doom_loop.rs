//! Doom-loop detection: repeated tool signatures and repeated text.

use std::collections::VecDeque;

/// Tracks recent tool invocations and assistant text for loop detection.
#[derive(Debug, Default)]
pub struct DoomLoopDetector {
    recent_tools: VecDeque<String>,
    recent_texts: VecDeque<String>,
    tool_window: usize,
    text_window: usize,
    tool_threshold: usize,
    text_threshold: usize,
}

impl DoomLoopDetector {
    pub fn new() -> Self {
        Self {
            recent_tools: VecDeque::new(),
            recent_texts: VecDeque::new(),
            tool_window: 8,
            text_window: 6,
            tool_threshold: 3,
            text_threshold: 3,
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

    /// Returns true when the same tool signature appears `tool_threshold`
    /// times in a row, or the same text block repeats.
    pub fn is_doom_loop(&self) -> bool {
        if self.recent_tools.len() >= self.tool_threshold {
            let last = self.recent_tools.back().cloned().unwrap_or_default();
            let streak = self
                .recent_tools
                .iter()
                .rev()
                .take_while(|k| **k == last)
                .count();
            if streak >= self.tool_threshold {
                return true;
            }
        }
        if self.recent_texts.len() >= self.text_threshold {
            let last = self.recent_texts.back().cloned().unwrap_or_default();
            let streak = self
                .recent_texts
                .iter()
                .rev()
                .take_while(|t| **t == last)
                .count();
            if streak >= self.text_threshold && last.len() > 20 {
                return true;
            }
        }
        false
    }
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
}
