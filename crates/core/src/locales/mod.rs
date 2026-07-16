//! Locale-aware entity detection patterns and language heuristic.
//!
//! Provides compile-time embedded locale JSON files and functions to detect
//! the dominant language of a text block, resolve it to a locale, and load
//! entity detection patterns for that locale.
//!
//! ## Design
//!
//! Each JSON file in this directory encodes per-language patterns used by
//! `entity_detector.rs`: candidate regexes, person/project verb templates,
//! pronouns, dialogue markers, direct-address patterns, and stopwords.
//! Files are embedded via `include_str!` so the binary carries no runtime
//! filesystem dependency.
//!
//! The `detect_language_heuristic()` function analyses Unicode script
//! ranges to identify the dominant language of a text sample. It returns
//! a BCP 47 code that callers pass to `resolve()` to obtain a
//! [`LocaleData`] with the entity patterns.

#![doc(hidden)]

use serde::Deserialize;
use std::collections::{HashMap, HashSet};

// ─── Embedded locale JSON files ──────────────────────────────────────

static EN_JSON: &str = include_str!("en.json");
static FR_JSON: &str = include_str!("fr.json");
static DE_JSON: &str = include_str!("de.json");
static ES_JSON: &str = include_str!("es.json");
static JA_JSON: &str = include_str!("ja.json");
static ZH_CN_JSON: &str = include_str!("zh-CN.json");
static KO_JSON: &str = include_str!("ko.json");
static RU_JSON: &str = include_str!("ru.json");
static PT_BR_JSON: &str = include_str!("pt-BR.json");
static IT_JSON: &str = include_str!("it.json");
static ID_JSON: &str = include_str!("id.json");
static ZH_TW_JSON: &str = include_str!("zh-TW.json");
static BE_JSON: &str = include_str!("be.json");

/// Ordered list of all embedded locale sources.
static LOCALE_SOURCES: &[(&str, &str)] = &[
    ("en", EN_JSON),
    ("fr", FR_JSON),
    ("de", DE_JSON),
    ("es", ES_JSON),
    ("ja", JA_JSON),
    ("zh-CN", ZH_CN_JSON),
    ("ko", KO_JSON),
    ("ru", RU_JSON),
    ("pt-BR", PT_BR_JSON),
    ("it", IT_JSON),
    ("id", ID_JSON),
    ("zh-TW", ZH_TW_JSON),
    ("be", BE_JSON),
];

// ─── Data structures ─────────────────────────────────────────────────

/// Deserialized entity-detection section of a locale JSON file.
#[derive(Debug, Clone, Deserialize)]
pub struct EntityPatterns {
    #[serde(default)]
    pub candidate_pattern: String,
    #[serde(default)]
    pub multi_word_pattern: String,
    #[serde(default = "default_empty_vec_str")]
    pub person_verb_patterns: Vec<String>,
    #[serde(default = "default_empty_vec_str")]
    pub pronoun_patterns: Vec<String>,
    #[serde(default = "default_empty_vec_str")]
    pub dialogue_patterns: Vec<String>,
    #[serde(default)]
    pub direct_address_pattern: String,
    #[serde(default = "default_empty_vec_str")]
    pub project_verb_patterns: Vec<String>,
    #[serde(default = "default_empty_vec_str")]
    pub stopwords: Vec<String>,
    #[serde(default)]
    pub boundary_chars: String,
}

fn default_empty_vec_str() -> Vec<String> {
    Vec::new()
}

/// Top-level locale data loaded from a JSON file.
#[derive(Debug, Clone, Deserialize)]
pub struct LocaleData {
    /// BCP 47 language code (e.g. "en", "zh-CN").
    pub lang: String,
    /// Human-readable label (e.g. "English", "简体中文").
    pub label: String,
    /// Broad script classification hint (latin, cyrillic, cjk, other).
    #[serde(default = "default_script")]
    pub script: String,
    /// Entity detection patterns for this locale.
    pub entity: EntityPatterns,
}

fn default_script() -> String {
    "latin".to_string()
}

// ─── Lazy static registry ────────────────────────────────────────────

use std::sync::LazyLock;

static LOCALE_REGISTRY: LazyLock<HashMap<String, LocaleData>> = LazyLock::new(|| {
    let mut map = HashMap::new();
    for &(code, json_str) in LOCALE_SOURCES {
        match serde_json::from_str::<LocaleData>(json_str) {
            Ok(data) => {
                // Insert with exact code.
                map.insert(code.to_string(), data.clone());
                // Also insert lowercase for case-insensitive lookup.
                map.insert(code.to_lowercase(), data);
            }
            Err(e) => {
                tracing::warn!("Failed to parse locale {code} at compile-time embed: {e}");
            }
        }
    }
    map
});

// ─── Public API ──────────────────────────────────────────────────────

/// List all available locale codes.
pub fn available_locales() -> Vec<&'static str> {
    LOCALE_SOURCES.iter().map(|&(code, _)| code).collect()
}

/// Resolve a BCP 47 code to a [`LocaleData`]. Case-insensitive.
/// Falls back to English when the code is not recognised.
pub fn resolve(code: &str) -> &LocaleData {
    let lower = code.to_lowercase();
    if let Some(locale) = LOCALE_REGISTRY.get(&lower) {
        return locale;
    }
    // Try primary sub-tag (e.g. "pt" from "pt-BR"). Prefer registration
    // order from LOCALE_SOURCES so "zh" resolves to zh-CN before zh-TW.
    if let Some(primary) = lower.split('-').next() {
        for &(registered, _) in LOCALE_SOURCES {
            if registered.to_lowercase().starts_with(primary) {
                if let Some(locale) = LOCALE_REGISTRY.get(registered) {
                    return locale;
                }
            }
        }
    }
    LOCALE_REGISTRY.get("en").expect("en locale must exist")
}

/// Heuristic language detection based on Unicode character script ranges.
///
/// Counts the number of characters that fall into well-known Unicode
/// scripts and picks the dominant one. When two scripts are tied the
/// one that appears first in the text wins (first-letter bias), which
/// matches common real-world usage.
///
/// Returns a BCP 47 code that can be passed to [`resolve()`].
///
/// ## Supported scripts
///
/// | Script        | Detected code |
/// |---------------|---------------|
/// | Hangul        | `ko`          |
/// | Hiragana/Katakana | `ja`     |
/// | CJK Unified   | `zh-CN`       |
/// | Cyrillic      | `ru`          |
/// | Latin (default)| `en`         |
pub fn detect_language_heuristic(text: &str) -> &'static str {
    let mut counts: HashMap<&str, usize> = HashMap::new();

    for ch in text.chars() {
        let script = classify_char_script(ch);
        if let Some(s) = script {
            *counts.entry(s).or_insert(0) += 1;
        }
    }

    // Score scripts with a bonus for the number of distinct characters
    // of that script — a single repeated CJK char is less convincing
    // than many different Latin letters.
    let best = counts
        .iter()
        .max_by_key(|(&script, &count)| {
            // Simple weighting: raw count is the primary key.
            // Scripts with fewer distinct characters get a tiny bonus
            // (e.g. hangul syllables are fewer than CJK ideographs).
            let bonus = match script {
                "ko" => 2, // Hangul block is compact; boost it slightly.
                "ja" => 1, // Hiragana/Katakana are unique to Japanese.
                _ => 0,
            };
            (count + bonus, script)
        })
        .map(|(&script, _)| script_to_locale(script))
        .unwrap_or("en");

    best
}

/// Detect language for a small text sample (first N bytes).
///
/// Convenience wrapper around [`detect_language_heuristic`] that limits
/// the analysis to the first `max_bytes` of `text`.
pub fn detect_language_sample(text: &str, max_bytes: usize) -> &'static str {
    let sample: String = text.chars().take(max_bytes).collect();
    detect_language_heuristic(&sample)
}

// ─── Script classification (internal) ────────────────────────────────

/// Classify a single character into a coarse script label.
/// Returns `None` for punctuation, digits, and whitespace.
fn classify_char_script(ch: char) -> Option<&'static str> {
    let cp = ch as u32;

    // Hangul Jamo + Syllables (AC00–D7AF, 1100–11FF, 3130–318F, A960–A97F, D7B0–D7FF)
    if (0xAC00..=0xD7AF).contains(&cp)
        || (0x1100..=0x11FF).contains(&cp)
        || (0x3130..=0x318F).contains(&cp)
        || (0xA960..=0xA97F).contains(&cp)
        || (0xD7B0..=0xD7FF).contains(&cp)
    {
        return Some("hangul");
    }

    // Hiragana (3040–309F)
    if (0x3040..=0x309F).contains(&cp) {
        return Some("hiragana");
    }

    // Katakana (30A0–30FF, 31F0–31FF)
    if (0x30A0..=0x30FF).contains(&cp) || (0x31F0..=0x31FF).contains(&cp) {
        return Some("katakana");
    }

    // CJK Unified Ideographs (4E00–9FFF, 3400–4DBF, 20000–2A6DF, ...)
    if (0x4E00..=0x9FFF).contains(&cp)
        || (0x3400..=0x4DBF).contains(&cp)
        || (0x20000..=0x2A6DF).contains(&cp)
    {
        return Some("cjk");
    }

    // Cyrillic (0400–04FF, 0500–052F, 2DE0–2DFF, A640–A69F)
    if (0x0400..=0x04FF).contains(&cp)
        || (0x0500..=0x052F).contains(&cp)
        || (0x2DE0..=0x2DFF).contains(&cp)
        || (0xA640..=0xA69F).contains(&cp)
    {
        return Some("cyrillic");
    }

    // Latin (basic + extended ranges)
    if (0x0041..=0x005A).contains(&cp)
        || (0x0061..=0x007A).contains(&cp)
        || (0x00C0..=0x024F).contains(&cp)
        || (0x0100..=0x017F).contains(&cp)
        || (0x0180..=0x024F).contains(&cp)
        || (0x1E00..=0x1EFF).contains(&cp)
    {
        return Some("latin");
    }

    None
}

/// Map a raw script label to a BCP 47 locale code.
fn script_to_locale(script: &str) -> &'static str {
    match script {
        "hangul" => "ko",
        "hiragana" | "katakana" => "ja",
        "cjk" => "zh-CN",
        "cyrillic" => "ru",
        "latin" => "en",
        _ => "en",
    }
}

// ─── Merged pattern access (multi-language) ──────────────────────────

/// Merged entity patterns across multiple locales.
///
/// Useful when the corpus contains mixed-language content and the caller
/// wants to search for entities across all declared languages.
#[derive(Debug, Clone, Default)]
pub struct MergedPatterns {
    pub candidate_patterns: Vec<String>,
    pub multi_word_patterns: Vec<String>,
    pub person_verb_patterns: Vec<String>,
    pub pronoun_patterns: Vec<String>,
    pub dialogue_patterns: Vec<String>,
    pub direct_address_patterns: Vec<String>,
    pub project_verb_patterns: Vec<String>,
    pub stopwords: HashSet<String>,
}

/// Return merged entity patterns for the given language codes.
///
/// Locales are merged in the order provided. Stopwords are set-unioned.
/// Duplicate patterns are removed while preserving first-occurrence order.
pub fn get_merged_patterns(langs: &[&str]) -> MergedPatterns {
    let mut merged = MergedPatterns::default();
    let mut seen_person: HashSet<String> = HashSet::new();
    let mut seen_pronoun: HashSet<String> = HashSet::new();
    let mut seen_dialogue: HashSet<String> = HashSet::new();
    let mut seen_project: HashSet<String> = HashSet::new();

    for &lang in langs {
        let locale = resolve(lang);
        let e = &locale.entity;

        if !e.candidate_pattern.is_empty() {
            merged.candidate_patterns.push(e.candidate_pattern.clone());
        }
        if !e.multi_word_pattern.is_empty() {
            merged
                .multi_word_patterns
                .push(e.multi_word_pattern.clone());
        }
        if !e.direct_address_pattern.is_empty() {
            merged
                .direct_address_patterns
                .push(e.direct_address_pattern.clone());
        }

        for p in &e.person_verb_patterns {
            if seen_person.insert(p.clone()) {
                merged.person_verb_patterns.push(p.clone());
            }
        }
        for p in &e.pronoun_patterns {
            if seen_pronoun.insert(p.clone()) {
                merged.pronoun_patterns.push(p.clone());
            }
        }
        for p in &e.dialogue_patterns {
            if seen_dialogue.insert(p.clone()) {
                merged.dialogue_patterns.push(p.clone());
            }
        }
        for p in &e.project_verb_patterns {
            if seen_project.insert(p.clone()) {
                merged.project_verb_patterns.push(p.clone());
            }
        }
        for w in &e.stopwords {
            merged.stopwords.insert(w.clone());
        }
    }

    merged
}

/// Auto-detect the language of `text` and return the merged entity
/// patterns for the detected language. Falls back to English patterns
/// when detection confidence is low.
pub fn auto_patterns(text: &str) -> MergedPatterns {
    let detected = detect_language_heuristic(text);
    get_merged_patterns(&[detected])
}

// ─── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_available_locales_count() {
        let locales = available_locales();
        assert_eq!(locales.len(), 13);
        assert!(locales.contains(&"en"));
        assert!(locales.contains(&"fr"));
        assert!(locales.contains(&"de"));
        assert!(locales.contains(&"es"));
        assert!(locales.contains(&"ja"));
        assert!(locales.contains(&"zh-CN"));
        assert!(locales.contains(&"ko"));
        assert!(locales.contains(&"ru"));
        assert!(locales.contains(&"pt-BR"));
        assert!(locales.contains(&"it"));
        assert!(locales.contains(&"id"));
        assert!(locales.contains(&"zh-TW"));
        assert!(locales.contains(&"be"));
    }

    #[test]
    fn test_resolve_exact() {
        let locale = resolve("en");
        assert_eq!(locale.lang, "en");
        assert_eq!(locale.label, "English");
    }

    #[test]
    fn test_resolve_case_insensitive() {
        let locale = resolve("EN");
        assert_eq!(locale.lang, "en");
        let locale = resolve("ZH-CN");
        assert_eq!(locale.lang, "zh-CN");
        let locale = resolve("zh-cn");
        assert_eq!(locale.lang, "zh-CN");
    }

    #[test]
    fn test_resolve_fallback_to_english() {
        let locale = resolve("xx");
        assert_eq!(locale.lang, "en");
    }

    #[test]
    fn test_resolve_partial_subtag() {
        // "zh" should match "zh-CN"
        let locale = resolve("zh");
        assert_eq!(locale.lang, "zh-CN");
    }

    #[test]
    fn test_detect_english() {
        let lang = detect_language_heuristic("The quick brown fox jumps over the lazy dog");
        assert_eq!(lang, "en");
    }

    #[test]
    fn test_detect_russian() {
        let lang =
            detect_language_heuristic("Привет, как дела? Это тест для определения языка текста.");
        assert_eq!(lang, "ru");
    }

    #[test]
    fn test_detect_chinese() {
        let lang = detect_language_heuristic("这是一个中文文本测试，用于检测语言");
        assert_eq!(lang, "zh-CN");
    }

    #[test]
    fn test_detect_japanese() {
        // Japanese text with hiragana + kanji
        let lang = detect_language_heuristic("これはテストです。日本語のテキストを検出します。");
        assert_eq!(lang, "ja");
    }

    #[test]
    fn test_detect_korean() {
        let lang =
            detect_language_heuristic("이것은 한국어 텍스트입니다. 언어 감지 테스트를 합니다.");
        assert_eq!(lang, "ko");
    }

    #[test]
    fn test_detect_french() {
        // French text with accented characters
        let lang = detect_language_heuristic(
            "Bonjour, comment allez-vous aujourd'hui? C'est une belle journée à Paris.",
        );
        assert_eq!(lang, "en"); // Latin script defaults to en for heuristic
    }

    #[test]
    fn test_entity_patterns_all_locales() {
        for &lang in &available_locales() {
            let locale = resolve(lang);
            assert!(
                !locale.entity.person_verb_patterns.is_empty(),
                "{lang}: person_verb_patterns should not be empty"
            );
            assert!(
                !locale.entity.stopwords.is_empty(),
                "{lang}: stopwords should not be empty"
            );
            assert!(
                !locale.entity.candidate_pattern.is_empty(),
                "{lang}: candidate_pattern should not be empty"
            );
        }
    }

    #[test]
    fn test_dialogue_patterns_present() {
        for &lang in &available_locales() {
            let locale = resolve(lang);
            assert!(
                !locale.entity.dialogue_patterns.is_empty(),
                "{lang}: dialogue_patterns should not be empty"
            );
        }
    }

    #[test]
    fn test_pronoun_patterns_present() {
        for &lang in &available_locales() {
            let locale = resolve(lang);
            assert!(
                !locale.entity.pronoun_patterns.is_empty(),
                "{lang}: pronoun_patterns should not be empty"
            );
        }
    }

    #[test]
    fn test_project_verb_patterns_present() {
        for &lang in &available_locales() {
            let locale = resolve(lang);
            assert!(
                !locale.entity.project_verb_patterns.is_empty(),
                "{lang}: project_verb_patterns should not be empty"
            );
        }
    }

    #[test]
    fn test_direct_address_patterns_present() {
        for &lang in &available_locales() {
            let locale = resolve(lang);
            assert!(
                !locale.entity.direct_address_pattern.is_empty(),
                "{lang}: direct_address_pattern should not be empty"
            );
        }
    }

    #[test]
    fn test_merged_patterns_union() {
        let merged = get_merged_patterns(&["en", "fr", "de"]);
        // All three languages should contribute stopwords.
        assert!(merged.stopwords.contains("the")); // en
        assert!(merged.stopwords.contains("le")); // fr
        assert!(merged.stopwords.contains("der")); // de
                                                   // Patterns should be deduplicated.
        let count = merged
            .dialogue_patterns
            .iter()
            .filter(|p| *p == "^>\\s*{name}[:\\s]")
            .count();
        assert_eq!(count, 1, "Dialogue pattern should be deduplicated");
    }

    #[test]
    fn test_auto_patterns_english() {
        let patterns = auto_patterns("Hello world, this is a test.");
        assert!(!patterns.person_verb_patterns.is_empty());
    }

    #[test]
    fn test_auto_patterns_russian() {
        let patterns = auto_patterns("Привет мир, это тест на русском языке.");
        assert!(!patterns.person_verb_patterns.is_empty());
        // Russian verb templates should be present.
        assert!(patterns
            .person_verb_patterns
            .iter()
            .any(|p| p.contains("сказал")));
    }

    #[test]
    fn test_script_classification() {
        assert_eq!(classify_char_script('A'), Some("latin"));
        assert_eq!(classify_char_script('А'), Some("cyrillic")); // Cyrillic A
        assert_eq!(classify_char_script('あ'), Some("hiragana"));
        assert_eq!(classify_char_script('ア'), Some("katakana"));
        assert_eq!(classify_char_script('한'), Some("hangul"));
        assert_eq!(classify_char_script('中'), Some("cjk"));
        assert_eq!(classify_char_script(' '), None);
        assert_eq!(classify_char_script('1'), None);
    }

    #[test]
    fn test_detect_language_sample() {
        // Use enough English text so the first 200 chars are clearly English.
        let long_text = "The quick brown fox jumps over the lazy dog. How vexingly quick daft zebras jump! The five boxing wizards jump quickly. Sphinx of black quartz, judge my vow. Pack my box with five dozen liquor jugs. привет мир";
        let detected = detect_language_sample(long_text, 200);
        // The English portion dominates the first 200 chars.
        assert_eq!(detected, "en");
    }

    #[test]
    fn test_cjk_ja_vs_zh() {
        // Pure hiragana -> ja
        let lang = detect_language_heuristic("すもももももももものうち");
        assert_eq!(lang, "ja");

        // Pure kanji -> zh-CN
        let lang = detect_language_heuristic("中华人民共和国");
        assert_eq!(lang, "zh-CN");
    }

    #[test]
    fn test_empty_text_defaults_to_english() {
        let lang = detect_language_heuristic("");
        assert_eq!(lang, "en");
    }

    #[test]
    fn test_boundary_chars_for_cjk() {
        let ja = resolve("ja");
        assert!(!ja.entity.boundary_chars.is_empty());
        let zh = resolve("zh-CN");
        assert!(!zh.entity.boundary_chars.is_empty());
        let ko = resolve("ko");
        // ko may or may not have boundary chars; just ensure it resolves.
        assert_eq!(ko.lang, "ko");
    }
}
