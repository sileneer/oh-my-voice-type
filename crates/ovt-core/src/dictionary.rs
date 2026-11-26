//! Personal dictionary: correct spellings of names/jargon. Terms feed ASR
//! hotword fields (through provider capability rules) and cleanup prompts.
//!
//! Ranking: higher hit count first; ties broken alphabetically so the budget
//! allocation is deterministic across runs and devices.

use serde::{Deserialize, Serialize};

/// Maximum characters of hotwords sent to prompt-style ASR fields (Whisper's
/// `prompt` caps around 224 tokens; 240 chars is conservative).
pub const HOTWORD_BUDGET_CHARS: usize = 240;

/// Maximum terms prefixed into the cleanup prompt (context is a token budget
/// too; the ranker keeps the most-used first).
pub const CLEANUP_TERM_LIMIT: usize = 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DictEntry {
    pub term: String,
    /// How many times the term was used/chose correct — higher ranks first.
    pub hits: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Dictionary {
    pub entries: Vec<DictEntry>,
}

impl Dictionary {
    pub fn add_term(&mut self, term: &str) {
        let term = term.trim();
        if term.is_empty() {
            return;
        }
        if let Some(e) = self
            .entries
            .iter_mut()
            .find(|e| e.term.eq_ignore_ascii_case(term))
        {
            e.hits += 1;
        } else {
            self.entries.push(DictEntry {
                term: term.to_string(),
                hits: 1,
            });
        }
        self.entries.sort_by_key(|e| std::cmp::Reverse(e.hits));
    }

    pub fn rename_term(&mut self, from: &str, to: &str) -> bool {
        let to = to.trim();
        if to.is_empty()
            || self
                .entries
                .iter()
                .any(|entry| entry.term.ne(from) && entry.term.eq_ignore_ascii_case(to))
        {
            return false;
        }
        let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.term.eq_ignore_ascii_case(from))
        else {
            return false;
        };
        entry.term = to.to_string();
        true
    }

    pub fn delete_term(&mut self, term: &str) -> bool {
        let before = self.entries.len();
        self.entries
            .retain(|entry| !entry.term.eq_ignore_ascii_case(term));
        before != self.entries.len()
    }

    pub fn rank(&self) -> Vec<&DictEntry> {
        let mut sorted: Vec<&DictEntry> = self.entries.iter().collect();
        sorted.sort_by(|a, b| b.hits.cmp(&a.hits).then(a.term.cmp(&b.term)));
        sorted
    }

    /// Ranked term strings (highest value first) for provider-side shaping.
    pub fn ranked_terms(&self) -> Vec<String> {
        self.rank().into_iter().map(|e| e.term.clone()).collect()
    }

    /// Terms prefixed into the cleanup prompt.
    pub fn prompt_terms(&self) -> Vec<String> {
        self.rank()
            .iter()
            .take(CLEANUP_TERM_LIMIT)
            .map(|e| e.term.clone())
            .collect()
    }

    /// Hotwords trimmed to a character budget, highest ranked first, without
    /// cutting words in half at the budget edge (clean words only).
    ///
    /// Kept for prompt-field providers (legacy call sites) and used by
    /// [`crate::providers::capabilities::HotwordRules::PromptField`] callers
    /// that only want the list itself.
    pub fn hotwords(&self, budget: usize) -> Vec<String> {
        let mut out = Vec::new();
        let mut used = 0usize;
        for term in self.ranked_terms() {
            let cost = term.chars().count() + 2; // ", "
            if used + cost > budget {
                continue;
            }
            used += cost;
            out.push(term);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranks_by_hits_then_alphabetically() {
        let mut d = Dictionary::default();
        d.add_term("beta");
        d.add_term("alpha");
        d.add_term("beta");
        let ranked = d.ranked_terms();
        assert_eq!(ranked[0], "beta");
        assert_eq!(ranked[1], "alpha");

        let mut d = Dictionary::default();
        d.add_term("bravo");
        d.add_term("alpha");
        // equal hits -> alphabetical
        assert_eq!(d.ranked_terms(), vec!["alpha", "bravo"]);
    }

    #[test]
    fn respects_budget() {
        let mut d = Dictionary::default();
        for i in 0..100 {
            d.add_term(&format!("term{i:03}"));
        }
        let hot = d.hotwords(50);
        let joined_len: usize = hot.iter().map(|h| h.len() + 2).sum();
        assert!(joined_len <= 50);
        assert!(!hot.is_empty());
    }

    #[test]
    fn dedup_case_insensitive() {
        let mut d = Dictionary::default();
        d.add_term("Foo");
        d.add_term("foo");
        assert_eq!(d.entries.len(), 1);
        assert_eq!(d.entries[0].hits, 2);
    }

    #[test]
    fn prompt_terms_capped() {
        let mut d = Dictionary::default();
        for i in 0..40 {
            d.add_term(&format!("term{i:02}"));
        }
        let terms = d.prompt_terms();
        assert_eq!(terms.len(), CLEANUP_TERM_LIMIT);
    }
}
