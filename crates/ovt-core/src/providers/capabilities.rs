//! Provider capability descriptions used by the engine to shape ASR requests
//! (hotword budgeting, context composition, language codes) without knowing
//! which wire protocol it is talking to.

/// How a provider accepts vocabulary priming.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotwordRules {
    /// No priming support: terms are dropped (they still inform cleanup).
    Unsupported,
    /// Words joined into a free-text prompt field, bounded by total chars.
    PromptField { max_chars: usize },
    /// Discrete `hotwords` array: item count cap + per-item char cap.
    List {
        max_items: usize,
        max_item_chars: usize,
    },
}

impl HotwordRules {
    /// Split ranked terms into (`hotwords`, `overflow`).
    ///
    /// `overflow` terms still inform cleanup prompts and (where the provider
    /// has a context field) ride along as context text.
    pub fn apply(&self, terms: Vec<String>) -> (Vec<String>, Vec<String>) {
        match *self {
            HotwordRules::Unsupported => (Vec::new(), terms),
            HotwordRules::PromptField { max_chars } => {
                let mut kept = Vec::new();
                let mut used = 0usize;
                let mut overflow = Vec::new();
                for term in terms {
                    let cost = term.chars().count() + 2; // ", "
                    if used + cost <= max_chars {
                        used += cost;
                        kept.push(term);
                    } else {
                        overflow.push(term);
                    }
                }
                (kept, overflow)
            }
            HotwordRules::List {
                max_items,
                max_item_chars,
            } => {
                let mut hotwords = Vec::new();
                let mut overflow = Vec::new();
                for term in terms {
                    if hotwords.len() < max_items && term.chars().count() <= max_item_chars {
                        hotwords.push(term);
                    } else {
                        overflow.push(term);
                    }
                }
                (hotwords, overflow)
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct AsrCapabilities {
    pub hotwords: HotwordRules,
    /// Character budget of a free-text context field, `None` when the
    /// provider has no such field.
    pub max_context_chars: Option<usize>,
    /// Accepted provider-native language codes with English labels.
    /// Empty = accepts ISO-639-1 primary subtags.
    pub languages: &'static [(&'static str, &'static str)],
}

impl AsrCapabilities {
    /// Map a user language setting ("en", "en-US", "auto", …) to the
    /// provider's wire format. `None` = let the provider auto-detect.
    pub fn resolve_language(&self, setting: Option<&str>) -> Option<String> {
        let raw = setting.map(str::trim).filter(|s| !s.is_empty())?;
        if raw.eq_ignore_ascii_case("auto") {
            return None;
        }
        if self.languages.is_empty() {
            // ISO-639-1 style providers: send the primary subtag ("en-US" -> "en").
            let primary = raw.split(['-', '_']).next().unwrap_or(raw);
            return Some(primary.to_ascii_lowercase());
        }
        if let Some((code, _)) = self
            .languages
            .iter()
            .find(|(code, _)| code.eq_ignore_ascii_case(raw))
        {
            return Some((*code).to_string());
        }
        let primary = raw
            .split(['-', '_'])
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        self.languages
            .iter()
            .find(|(code, _)| {
                code.split('-')
                    .next()
                    .unwrap_or("")
                    .eq_ignore_ascii_case(&primary)
            })
            .map(|(code, _)| (*code).to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const U2_LIKE: AsrCapabilities = AsrCapabilities {
        hotwords: HotwordRules::List {
            max_items: 3,
            max_item_chars: 5,
        },
        max_context_chars: Some(500),
        languages: &[
            ("en-US", "English"),
            ("zh-CN", "Chinese"),
            ("ja-JP", "Japanese"),
        ],
    };

    #[test]
    fn list_rules_split_short_and_long_terms() {
        // `apply` consumes already-ranked terms; mimicking the engine's
        // deterministic order (by hits, then alphabetically).
        let terms = vec![
            "Claude".to_string(),
            "Codestral-CLI".to_string(),
            "K8s".to_string(),
            "Kubernetes".to_string(),
            "Redis".to_string(),
        ];
        let (hot, overflow) = U2_LIKE.hotwords.apply(terms);
        // ≤5 chars AND within the item cap: K8s (3), Redis (5).
        // "Claude" (6) and both long terms exceed the char cap → context.
        assert_eq!(hot, vec!["K8s", "Redis"]);
        assert_eq!(overflow, vec!["Claude", "Codestral-CLI", "Kubernetes"]);
    }

    #[test]
    fn prompt_field_rules_respect_char_budget() {
        let rules = HotwordRules::PromptField { max_chars: 20 };
        let (kept, overflow) = rules.apply(vec!["alpha".into(), "beta".into(), "gamma".into()]);
        // 5+2 + 4+2 = 13 fits, 5+2 more = 20 fits exactly; none overflow here
        assert_eq!(kept.len(), 3);
        assert!(overflow.is_empty());

        let (kept, overflow) =
            HotwordRules::PromptField { max_chars: 10 }.apply(vec!["alpha".into(), "beta".into()]);
        assert_eq!(kept, vec!["alpha"]);
        assert_eq!(overflow, vec!["beta"]);
    }

    #[test]
    fn unsupported_moves_all_terms_to_overflow() {
        let (hot, overflow) = HotwordRules::Unsupported.apply(vec!["a".into(), "b".into()]);
        assert!(hot.is_empty());
        assert_eq!(overflow, vec!["a", "b"]);
    }

    #[test]
    fn resolution_prefers_exact_code_then_primary_subtag() {
        assert_eq!(
            U2_LIKE.resolve_language(Some("en-US")),
            Some("en-US".to_string())
        );
        assert_eq!(
            U2_LIKE.resolve_language(Some("zh")),
            Some("zh-CN".to_string())
        );
        assert_eq!(
            U2_LIKE.resolve_language(Some("JA-jp")),
            Some("ja-JP".to_string())
        );
        assert_eq!(U2_LIKE.resolve_language(Some("de")), None); // not offered
        assert_eq!(U2_LIKE.resolve_language(None), None);
        assert_eq!(U2_LIKE.resolve_language(Some("auto")), None);
    }

    #[test]
    fn bare_subtag_providers_normalize_to_iso_639_1() {
        let caps = AsrCapabilities {
            hotwords: HotwordRules::PromptField { max_chars: 800 },
            max_context_chars: None,
            languages: &[],
        };
        assert_eq!(caps.resolve_language(Some("en-US")), Some("en".to_string()));
        assert_eq!(caps.resolve_language(Some("zh-CN")), Some("zh".to_string()));
        assert_eq!(caps.resolve_language(Some("en")), Some("en".to_string()));
    }
}
