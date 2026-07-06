//! fact_checker.rs — Verify text against known facts in the palace.
//!
//! Checks AI responses, diary entries, and new content against the entity
//! registry and knowledge graph for three classes of issue:
//!
//! * `SimilarName` — text mentions a name that's one/two edits
//!   away from *another* registered name, raising the possibility of a
//!   typo or mix-up.
//! * `RelationshipMismatch` — text asserts a role between two entities
//!   (e.g. "Bob is Alice's brother") while the KG records a *different*
//!   current role for the same subject/object pair.
//! * `StaleFact` — text asserts a fact that the KG marks closed
//!   (``valid_to`` in the past).
//!
//! Purely offline. Inputs: entity_registry JSON + KG SQLite. No network.

use crate::config::Config;
use crate::knowledge_graph::{EntityQueryResult, KnowledgeGraph};
use regex::Regex;
use std::collections::HashSet;
use std::path::Path;
use tracing::{debug, info, warn};

pub use self::FactIssueType::*;

/// Fact issues detected in text.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub struct FactIssue {
    #[serde(rename = "type")]
    pub issue_type: FactIssueType,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub names: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distance: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claim: Option<Claim>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kg_fact: Option<KgFact>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub valid_to: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum FactIssueType {
    SimilarName,
    RelationshipMismatch,
    StaleFact,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub struct Claim {
    pub predicate: String,
    pub object: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub struct KgFact {
    pub predicate: String,
    pub object: String,
}

/// Structured report of all issues found in text.
#[derive(Debug, Clone, serde::Serialize)]
pub struct FactCheckReport {
    /// Detected issues (empty = no contradictions found).
    pub issues: Vec<FactIssue>,
    /// Number of claims parsed from text.
    pub claims_checked: usize,
}

// ── Public API ──────────────────────────────────────────────────────────────

/// Check text for fact contradictions against the entity registry and KG.
///
/// Loads configuration, entity registry, and knowledge graph from disk.
/// Returns a [`FactCheckReport`] with all detected issues.
pub fn check_text(text: &str) -> FactCheckReport {
    if text.is_empty() {
        return FactCheckReport {
            issues: Vec::new(),
            claims_checked: 0,
        };
    }

    let config = match Config::load() {
        Ok(c) => c,
        Err(e) => {
            warn!("Failed to load config for fact check: {}", e);
            return FactCheckReport {
                issues: Vec::new(),
                claims_checked: 0,
            };
        }
    };

    let mut issues = Vec::new();
    let entity_names = load_known_entity_names();
    issues.extend(check_entity_confusion(text, &entity_names));

    let claims_checked = claims_in_text(text);
    issues.extend(check_kg_contradictions(text, &config.palace_path));

    FactCheckReport {
        issues,
        claims_checked,
    }
}

/// Check text against a pre-loaded knowledge graph (no Config dependency).
///
/// Use this when you already have a `KnowledgeGraph` handle (e.g. inside the
/// MCP server or daemon).  Only checks KG contradictions — does NOT check
/// entity-name confusion (which requires the entity registry).
pub fn check_text_with_kg(text: &str, kg: &KnowledgeGraph) -> FactCheckReport {
    if text.is_empty() {
        return FactCheckReport {
            issues: Vec::new(),
            claims_checked: 0,
        };
    }

    let entity_names = load_known_entity_names();
    let mut issues = Vec::new();

    issues.extend(check_entity_confusion(text, &entity_names));

    let claims_checked = claims_in_text(text);
    issues.extend(check_claims_against_kg(text, kg));

    FactCheckReport {
        issues,
        claims_checked,
    }
}

// ── entity-name confusion ────────────────────────────────────────────────────

fn load_known_entity_names() -> HashSet<String> {
    let mut names = HashSet::new();
    if let Ok(registry_path) = Config::registry_file_path() {
        if let Ok(registry) = crate::entity_registry::EntityRegistry::load(&registry_path) {
            for name in registry.people().keys() {
                names.insert(name.clone());
            }
        }
    }
    names
}

fn check_entity_confusion(text: &str, all_names: &HashSet<String>) -> Vec<FactIssue> {
    if all_names.is_empty() {
        return Vec::new();
    }

    // Which names from the registry actually appear in the text?
    let mentioned: Vec<&str> = all_names
        .iter()
        .filter(|name| {
            let pattern = format!("\\b({})\\b", regex::escape(name));
            Regex::new(&pattern)
                .map(|re| re.is_match(text))
                .unwrap_or(false)
        })
        .map(|s| s.as_str())
        .collect();

    if mentioned.is_empty() {
        info!("Fact-check: no registered entity names found in text");
        return Vec::new();
    }

    debug!(
        "Fact-check: {} names mentioned in text, scanning {} registry entries",
        mentioned.len(),
        all_names.len()
    );

    let mut issues = Vec::new();
    let mut seen_pairs: HashSet<(String, String)> = HashSet::new();

    for name_a in &mentioned {
        let a_lower = name_a.to_lowercase();
        for name_b in all_names {
            if name_b == *name_a {
                continue;
            }
            // Dedupe by unordered pair
            let pair_key = (
                std::cmp::min(a_lower.as_str(), name_b.as_str()).to_string(),
                std::cmp::max(a_lower.as_str(), name_b.as_str()).to_string(),
            );
            if seen_pairs.contains(&pair_key) {
                continue;
            }
            // If name_b is also mentioned, skip (both names in text = two people)
            if mentioned.contains(&name_b.as_str()) {
                seen_pairs.insert(pair_key);
                continue;
            }

            let distance = edit_distance(&a_lower, &name_b.to_lowercase());
            if distance > 0 && distance <= 2 {
                info!(
                    "Fact-check: similar name detected: '{}' ~ '{}' (dist={})",
                    name_a, name_b, distance
                );
                issues.push(FactIssue {
                    issue_type: FactIssueType::SimilarName,
                    detail: format!(
                        "'{}' mentioned — did you mean '{}'? (edit distance {})",
                        name_a, name_b, distance
                    ),
                    names: Some(vec![name_a.to_string(), name_b.to_string()]),
                    distance: Some(distance),
                    entity: None,
                    claim: None,
                    kg_fact: None,
                    valid_to: None,
                });
                seen_pairs.insert(pair_key);
            }
        }
    }

    issues
}

// ── KG contradictions ────────────────────────────────────────────────────────

/// "Bob is Alice's brother" → subject=Bob, possessor=Alice, role=brother
/// "Alice's brother is Bob" → possessor=Alice, role=brother, subject=Bob

fn get_claim_patterns() -> Vec<Regex> {
    vec![
        Regex::new(r"\b([A-Z][\w-]+)\s+is\s+([A-Z][\w-]+)'s\s+([a-z]{3,20})\b").unwrap(),
        Regex::new(r"\b([A-Z][\w-]+)'s\s+([a-z]{3,20})\s+is\s+([A-Z][\w-]+)\b").unwrap(),
    ]
}

#[derive(Debug)]
struct ParsedClaim {
    subject: String,
    predicate: String,
    object: String,
    span: String,
}

fn extract_claims(text: &str) -> Vec<ParsedClaim> {
    let mut claims = Vec::new();
    let patterns = get_claim_patterns();
    for (i, pat) in patterns.iter().enumerate() {
        for cap in pat.captures_iter(text) {
            let (_, groups) = cap.extract::<3>();
            let (subject, possessor, role) = if i == 0 {
                (groups[0], groups[1], groups[2])
            } else {
                (groups[2], groups[0], groups[1])
            };
            claims.push(ParsedClaim {
                subject: subject.to_string(),
                predicate: role.to_lowercase(),
                object: possessor.to_string(),
                span: groups[0].to_string(),
            });
        }
    }
    claims
}

fn claims_in_text(text: &str) -> usize {
    let mut count = 0;
    for pat in get_claim_patterns() {
        count += pat.captures_iter(text).count();
    }
    count
}

fn check_kg_contradictions(text: &str, palace_path: &Path) -> Vec<FactIssue> {
    let claims = extract_claims(text);
    if claims.is_empty() {
        return Vec::new();
    }

    let kg_db_path = palace_path.join("knowledge_graph.sqlite3");
    let Ok(kg) = KnowledgeGraph::open(&kg_db_path) else {
        return Vec::new();
    };

    check_claims_against_kg(text, &kg)
}

fn check_claims_against_kg(text: &str, kg: &KnowledgeGraph) -> Vec<FactIssue> {
    let claims = extract_claims(text);
    if claims.is_empty() {
        debug!("Fact-check: no relationship claims found in text");
        return Vec::new();
    }

    debug!("Fact-check: checking {} claims against KG", claims.len());

    let now = simple_iso_date();
    let mut issues = Vec::new();

    for claim in &claims {
        let Ok(facts) = kg.query_entity(&claim.subject, None, None, "outgoing") else {
            debug!(
                "Fact-check: KG lookup failed for subject '{}'",
                claim.subject
            );
            continue;
        };
        if facts.is_empty() {
            continue;
        }

        let current_facts: Vec<&EntityQueryResult> = facts.iter().filter(|f| f.current).collect();

        // Mismatch: same (subject, object) pair but different predicate
        for fact in &current_facts {
            let kg_obj = &fact.object;
            if !objects_match(kg_obj, &claim.object) {
                continue;
            }
            let kg_pred = fact.predicate.to_lowercase();
            if !kg_pred.is_empty() && kg_pred != claim.predicate {
                warn!(
                    "Fact-check: relationship mismatch: text says '{}' but KG records {} {} {}",
                    claim.span, claim.subject, kg_pred, kg_obj
                );
                issues.push(FactIssue {
                    issue_type: FactIssueType::RelationshipMismatch,
                    detail: format!(
                        "Text says '{}' but KG records {} {} {}",
                        claim.span, claim.subject, kg_pred, kg_obj
                    ),
                    names: None,
                    distance: None,
                    entity: Some(claim.subject.clone()),
                    claim: Some(Claim {
                        predicate: claim.predicate.clone(),
                        object: claim.object.clone(),
                    }),
                    kg_fact: Some(KgFact {
                        predicate: kg_pred,
                        object: kg_obj.clone(),
                    }),
                    valid_to: None,
                });
            }
        }

        // Stale fact: exact match but valid_to is in the past
        for fact in &facts {
            if fact.current {
                continue;
            }
            let kg_pred = fact.predicate.to_lowercase();
            if kg_pred != claim.predicate {
                continue;
            }
            if !objects_match(&fact.object, &claim.object) {
                continue;
            }
            // Skip facts superseded in transaction_time (t_expired = Some means a newer
            // correction replaced this fact)
            if fact.t_expired.is_some() {
                continue;
            }
            if let Some(valid_to) = &fact.valid_to {
                if valid_to.as_str() < now.as_str() {
                    info!(
                        "Fact-check: stale fact: text says '{}' but KG marks closed on {}",
                        claim.span, valid_to
                    );
                    issues.push(FactIssue {
                        issue_type: FactIssueType::StaleFact,
                        detail: format!(
                            "Text says '{}' but KG marks this fact closed on {}",
                            claim.span, valid_to
                        ),
                        names: None,
                        distance: None,
                        entity: Some(claim.subject.clone()),
                        claim: None,
                        kg_fact: None,
                        valid_to: Some(valid_to.clone()),
                    });
                }
            }
        }
    }

    issues
}

fn objects_match(kg_obj: &str, claim_obj: &str) -> bool {
    if kg_obj.is_empty() || claim_obj.is_empty() {
        return false;
    }
    kg_obj.trim().eq_ignore_ascii_case(claim_obj.trim())
}

// ── Date helper ──────────────────────────────────────────────────────────────

fn simple_iso_date() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap();
    let secs_per_day: u64 = 86400;
    let days = now.as_secs() / secs_per_day;
    let mut y: u64 = 1970;
    let mut remaining = days;
    while remaining >= 365 {
        let is_leap = (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0);
        let days_in_y = if is_leap { 366 } else { 365 };
        if remaining >= days_in_y {
            remaining -= days_in_y;
            y += 1;
        } else {
            break;
        }
    }
    let days_per_month: [u64; 12] = if (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut month = 1usize;
    for (i, &dpm) in days_per_month.iter().enumerate() {
        if remaining < dpm {
            month = i + 1;
            break;
        }
        remaining -= dpm;
    }
    let day = remaining + 1;
    format!("{:04}-{:02}-{:02}", y, month, day)
}

// ── Levenshtein distance ─────────────────────────────────────────────────────

fn edit_distance(s1: &str, s2: &str) -> usize {
    if s1.len() < s2.len() {
        return edit_distance(s2, s1);
    }
    if s2.is_empty() {
        return s1.len();
    }
    let mut prev: Vec<usize> = (0..=s2.len()).collect();
    for (i, c1) in s1.chars().enumerate() {
        let mut curr = vec![i + 1];
        for (j, c2) in s2.chars().enumerate() {
            let cost = if c1 == c2 { 0 } else { 1 };
            curr.push(std::cmp::min(
                prev[j + 1] + 1,
                std::cmp::min(curr[j] + 1, prev[j] + cost),
            ));
        }
        prev = curr;
    }
    prev[s2.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── edit_distance ─────────────────────────────────────────────────────

    #[test]
    fn test_edit_distance_identical() {
        assert_eq!(edit_distance("hello", "hello"), 0);
    }

    #[test]
    fn test_edit_distance_one_change() {
        assert_eq!(edit_distance("hello", "hallo"), 1);
    }

    #[test]
    fn test_edit_distance_one_delete() {
        assert_eq!(edit_distance("hello", "helo"), 1);
    }

    #[test]
    fn test_edit_distance_transpose() {
        // "kitten" -> "sitten" (sub) -> "sittin" (sub) -> "sitting" (ins)
        assert_eq!(edit_distance("kitten", "sitting"), 3);
    }

    #[test]
    fn test_edit_distance_empty_s1() {
        assert_eq!(edit_distance("", "abc"), 3);
    }

    #[test]
    fn test_edit_distance_empty_s2() {
        assert_eq!(edit_distance("abc", ""), 3);
    }

    // ── claim extraction ──────────────────────────────────────────────────

    #[test]
    fn test_extract_claims_bob_is_alices_brother() {
        let claims = extract_claims("Bob is Alice's brother");
        assert_eq!(claims.len(), 1);
        assert_eq!(claims[0].subject, "Bob");
        assert_eq!(claims[0].predicate, "brother");
        assert_eq!(claims[0].object, "Alice");
    }

    #[test]
    fn test_extract_claims_alices_brother_is_bob() {
        let claims = extract_claims("Alice's brother is Bob");
        assert_eq!(claims.len(), 1);
        assert_eq!(claims[0].subject, "Bob");
        assert_eq!(claims[0].predicate, "brother");
        assert_eq!(claims[0].object, "Alice");
    }

    #[test]
    fn test_extract_claims_no_match() {
        let claims = extract_claims("Alice went to the store");
        assert!(claims.is_empty());
    }

    #[test]
    fn test_extract_claims_multiple() {
        let claims = extract_claims("Bob is Alice's brother. Charlie is Dave's cousin.");
        assert_eq!(claims.len(), 2);
        assert_eq!(claims[0].subject, "Bob");
        assert_eq!(claims[1].subject, "Charlie");
    }

    #[test]
    fn test_claims_in_text() {
        assert_eq!(claims_in_text("Bob is Alice's brother"), 1);
        assert_eq!(claims_in_text("no claims here"), 0);
        assert_eq!(claims_in_text(""), 0);
    }

    // ── objects_match ─────────────────────────────────────────────────────

    #[test]
    fn test_objects_match_identical() {
        assert!(objects_match("Alice", "Alice"));
    }

    #[test]
    fn test_objects_match_case_insensitive() {
        assert!(objects_match("Alice", "alice"));
    }

    #[test]
    fn test_objects_match_empty() {
        assert!(!objects_match("", "Alice"));
        assert!(!objects_match("Alice", ""));
    }

    #[test]
    fn test_objects_match_trimmed() {
        assert!(objects_match("  Alice  ", "alice"));
    }

    // ── simple_iso_date ───────────────────────────────────────────────────

    #[test]
    fn test_simple_iso_date_format() {
        let date = simple_iso_date();
        assert_eq!(date.len(), 10, "expected YYYY-MM-DD format, got: {}", date);
        // Ensure it's a parseable date
        assert_eq!(date.as_bytes()[4], b'-');
        assert_eq!(date.as_bytes()[7], b'-');
    }

    // ── check_text (no-config path) ───────────────────────────────────────

    #[test]
    fn test_check_text_empty() {
        let report = check_text("");
        assert!(report.issues.is_empty());
        assert_eq!(report.claims_checked, 0);
    }

    #[test]
    fn test_check_text_with_kg_empty() {
        // Can't create a real KG without a DB, but we can test the empty-text
        // short-circuit.
        let report = check_text_with_kg(
            "",
            &KnowledgeGraph::open(std::path::Path::new(":memory:")).unwrap(),
        );
        assert!(report.issues.is_empty());
        assert_eq!(report.claims_checked, 0);
    }

    // ── entity confusion (unit-tested via check_entity_confusion) ────────

    #[test]
    fn test_entity_confusion_empty_names() {
        let names = HashSet::new();
        let issues = check_entity_confusion("Alice is here", &names);
        assert!(issues.is_empty());
    }

    #[test]
    fn test_entity_confusion_no_mentioned() {
        let mut names = HashSet::new();
        names.insert("Bob".to_string());
        // "Alice" not in the set
        let issues = check_entity_confusion("Alice is here", &names);
        assert!(issues.is_empty());
    }

    #[test]
    fn test_entity_confusion_too_many_edits() {
        let mut names = HashSet::new();
        names.insert("Alexander".to_string());
        // "Alice" is > 2 edits from "Alexander"
        let issues = check_entity_confusion("Alice is here", &names);
        assert!(issues.is_empty());
    }

    // ── edit distance boundary ────────────────────────────────────────────

    #[test]
    fn test_edit_distance_one_vs_two() {
        // "Jon" -> "John" = 1 edit (insert h)
        assert_eq!(edit_distance("jon", "john"), 1);
        // "Jon" -> "Joan" = 1 edit (delete a)
        assert_eq!(edit_distance("jon", "joan"), 1);
        // "Jon" -> "Jones" = 2 edits (insert e, insert s)
        // "jon" (3 chars) -> "jones" (5 chars): insert 'e', insert 's'
        assert_eq!(edit_distance("jon", "jones"), 2);
    }

    // ── serialization ─────────────────────────────────────────────────────

    #[test]
    fn test_fact_issue_serialization() {
        let issue = FactIssue {
            issue_type: FactIssueType::SimilarName,
            detail: "test".to_string(),
            names: Some(vec!["Alice".into(), "Alicia".into()]),
            distance: Some(1),
            entity: None,
            claim: None,
            kg_fact: None,
            valid_to: None,
        };
        let json = serde_json::to_value(&issue).unwrap();
        assert_eq!(json["type"], "similar_name");
        assert_eq!(json["names"][0], "Alice");
        assert_eq!(json["distance"], 1);
    }

    #[test]
    fn test_fact_issue_type_serialization() {
        assert_eq!(
            serde_json::to_value(FactIssueType::SimilarName).unwrap(),
            "similar_name"
        );
        assert_eq!(
            serde_json::to_value(FactIssueType::RelationshipMismatch).unwrap(),
            "relationship_mismatch"
        );
        assert_eq!(
            serde_json::to_value(FactIssueType::StaleFact).unwrap(),
            "stale_fact"
        );
    }

    #[test]
    fn test_report_serialization() {
        let report = FactCheckReport {
            issues: Vec::new(),
            claims_checked: 0,
        };
        let json = serde_json::to_value(&report).unwrap();
        assert!(json["issues"].as_array().unwrap().is_empty());
        assert_eq!(json["claims_checked"], 0);
    }

    #[test]
    fn test_report_with_issues() {
        let report = FactCheckReport {
            issues: vec![FactIssue {
                issue_type: FactIssueType::StaleFact,
                detail: "old fact".to_string(),
                names: None,
                distance: None,
                entity: Some("Bob".to_string()),
                claim: None,
                kg_fact: None,
                valid_to: Some("2025-01-01".to_string()),
            }],
            claims_checked: 1,
        };
        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(json["issues"].as_array().unwrap().len(), 1);
        assert_eq!(json["issues"][0]["type"], "stale_fact");
        assert_eq!(json["issues"][0]["entity"], "Bob");
        assert_eq!(json["claims_checked"], 1);
    }
}
