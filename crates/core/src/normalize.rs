use chrono::Utc;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::collections::HashSet;
use std::io::BufRead;

use crate::types::{HookType, RawObservation};

const SLACK_PROVENANCE_FOOTER: &str =
    "\n[source: slack-export | multi-party chat — speaker roles are positional, not verified]";

/// Maximum file size for transcript normalization: 500 MiB.
const MAX_FILE_SIZE_BYTES: u64 = 500 * 1024 * 1024;

/// Structured transcript message returned by individual parsers.
/// Captures a single user or assistant turn with provenance metadata.
#[derive(Debug, Clone)]
pub struct TranscriptMessage {
    /// "user" or "assistant"
    pub role: String,
    /// The message text content
    pub text: String,
    /// Source format identifier (e.g. "claude_code_jsonl", "gemini_ai_studio")
    pub source_format: String,
    /// Optional timestamp extracted from the source data
    pub timestamp: Option<chrono::DateTime<chrono::Utc>>,
    /// Optional sequence index within the source file
    pub index: Option<usize>,
}

/// Generate a deterministic observation ID from source content.
///
/// The ID is a SHA-256 hash of (source_format + separator + message_index +
/// separator + content) truncated to 16 hex chars, prefixed with "obs-".
pub fn deterministic_obs_id(source_format: &str, index: usize, content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(source_format.as_bytes());
    hasher.update(b"\0");
    hasher.update(index.to_string().as_bytes());
    hasher.update(b"\0");
    hasher.update(content.as_bytes());
    let hex = format!("{:x}", hasher.finalize());
    format!("obs-{}", &hex[..16])
}

/// Returns a prefix of `s` truncated to at most `max_bytes` bytes,
/// ensuring the result ends on a valid UTF-8 character boundary.
/// If `max_bytes` is >= `s.len()`, returns `s` unchanged.
pub fn safe_truncate(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

fn strip_noise(text: &str) -> String {
    let mut result = text.to_string();

    let noise_tags = [
        "system-reminder",
        "command-message",
        "command-name",
        "task-notification",
        "user-prompt-submit-hook",
        "hook_output",
    ];
    for tag in &noise_tags {
        let pattern = format!(r"(?m)^[\s]*{}[\s]*$", regex::escape(tag));
        if let Ok(re) = regex::Regex::new(&pattern) {
            result = re.replace_all(&result, "").to_string();
        }
    }

    if let Ok(re) = regex::Regex::new(
        r"(?m)^(?:> )?Ran \d+ (?:Stop|PreCompact|PreToolUse|PostToolUse|UserPromptSubmit|Notification|SessionStart|SessionEnd) hook[s]?.*$",
    ) {
        result = re.replace_all(&result, "").to_string();
    }

    let noise_prefixes = [
        "CURRENT TIME:",
        "VERIFIED FACTS",
        "AGENT SPECIALIZATION:",
        "Checking verified facts...",
        "Injecting timestamp...",
        "Starting background pipeline...",
        "Checking emotional weights...",
        "Auto-save reminder...",
        "Checking pipeline...",
        "MemPalace auto-save checkpoint.",
    ];
    for prefix in &noise_prefixes {
        let pattern = format!(r"(?m)^[\s]*{}.*$", regex::escape(prefix));
        if let Ok(re) = regex::Regex::new(&pattern) {
            result = re.replace_all(&result, "").to_string();
        }
    }

    if let Ok(re) = regex::Regex::new(r"(?m)^\s*… \+\d+ lines\s*$") {
        result = re.replace_all(&result, "").to_string();
    }

    if let Ok(re) = regex::Regex::new(r"\s*\[(\d+)\s+tokens?\]\s+\(ctrl\+o to (?:open|expand)\)") {
        result = re.replace_all(&result, "").to_string();
    }

    if let Ok(re) = regex::Regex::new(r"(?m)^\s*hook_output\s*$") {
        result = re.replace_all(&result, "").to_string();
    }

    if let Ok(re) = regex::Regex::new(r"\n{3,}") {
        result = re.replace_all(&result, "\n\n").to_string();
    }

    result.trim().to_string()
}

fn format_tool_use(content: &Value) -> String {
    let obj = match content.as_object() {
        Some(o) => o,
        None => return String::new(),
    };

    let tool_name = obj
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown");
    let args = obj
        .get("arguments")
        .or_else(|| obj.get("input"))
        .and_then(|v| v.as_object());

    match tool_name {
        "Bash" => {
            let cmd = args
                .and_then(|a| a.get("command").or_else(|| a.get("cmd")))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if cmd.len() > 200 {
                format!("[Bash] {}...", safe_truncate(cmd, 200))
            } else {
                format!("[Bash] {}", cmd)
            }
        }
        "Read" => {
            let path = args
                .and_then(|a| a.get("file_path").or_else(|| a.get("path")))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let offset = args.and_then(|a| a.get("offset")).and_then(|v| v.as_u64());
            let limit = args.and_then(|a| a.get("limit")).and_then(|v| v.as_u64());
            if let (Some(off), Some(lim)) = (offset, limit) {
                format!("[Read {}:{}-{}]", path, off, off.saturating_add(lim))
            } else {
                format!("[Read {}]", path)
            }
        }
        "Grep" | "Glob" => {
            let pattern = args
                .and_then(|a| a.get("pattern"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let target = args
                .and_then(|a| a.get("target").or_else(|| a.get("file_path")))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            format!("[{}] {} in {}", tool_name, pattern, target)
        }
        "Edit" | "Write" => {
            let path = args
                .and_then(|a| a.get("file_path").or_else(|| a.get("path")))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            format!("[{}] {}", tool_name, path)
        }
        _ => {
            let args_str = args
                .map(|a| serde_json::to_string(a).unwrap_or_default())
                .unwrap_or_default();
            let summary = if args_str.len() > 200 {
                format!("{}...", safe_truncate(&args_str, 200))
            } else {
                args_str
            };
            format!("[{}] {}", tool_name, summary)
        }
    }
}

fn format_tool_result(content: &Value, tool_name: Option<&str>) -> String {
    let obj = match content.as_object() {
        Some(o) => o,
        None => return String::new(),
    };

    let text = obj.get("content").and_then(|v| v.as_str()).unwrap_or("");

    match tool_name {
        Some("Read") | Some("Edit") | Some("Write") => String::new(),
        Some("Bash") => {
            let lines: Vec<&str> = text.lines().collect();
            if lines.len() <= 20 {
                if text.is_empty() {
                    String::new()
                } else {
                    format!("→ {}", text)
                }
            } else if lines.len() <= 40 {
                format!("→ {}", text)
            } else {
                let head: String = lines[..20].join("\n");
                let tail: String = lines[lines.len() - 20..].join("\n");
                format!(
                    "→ {}\n… [{} lines truncated] …\n{}",
                    head,
                    lines.len() - 40,
                    tail
                )
            }
        }
        Some("Grep") | Some("Glob") => {
            let lines: Vec<&str> = text.lines().collect();
            if lines.len() <= 20 {
                if text.is_empty() {
                    String::new()
                } else {
                    format!("→ {}", text)
                }
            } else {
                let kept: String = lines[..20].join("\n");
                format!("→ {}\n… [{} matches truncated] …", kept, lines.len() - 20)
            }
        }
        _ => {
            if text.len() > 2048 {
                format!("→ {}", safe_truncate(text, 2048))
            } else if text.is_empty() {
                String::new()
            } else {
                format!("→ {}", text)
            }
        }
    }
}

fn load_known_names() -> HashSet<String> {
    let Ok(registry_path) = crate::Config::registry_file_path() else {
        return HashSet::new();
    };
    let Ok(registry) = crate::entity_registry::EntityRegistry::load(&registry_path) else {
        return HashSet::new();
    };

    let mut names = HashSet::new();
    for (canonical, entry) in registry.people() {
        names.insert(canonical.to_lowercase());
        if let Some(canonical_name) = &entry.canonical {
            names.insert(canonical_name.to_lowercase());
        }
        for alias in &entry.aliases {
            names.insert(alias.to_lowercase());
        }
    }
    names
}

#[cfg(feature = "spellcheck")]
fn spellcheck_transcript_preserving_known_names(content: &str) -> String {
    let known_names = load_known_names();
    content
        .lines()
        .map(|line| {
            let stripped = line.trim_start();
            if !stripped.starts_with('>') {
                return line.to_string();
            }

            let prefix_len = line.len() - stripped.len() + 2;
            if prefix_len > line.len() {
                return line.to_string();
            }

            let message = &line[prefix_len..];
            if message.trim().is_empty() {
                return line.to_string();
            }

            let corrected = crate::spellcheck::correct_spelling(message, &known_names);
            format!("{}> {}", &line[..prefix_len - 2], corrected)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(not(feature = "spellcheck"))]
fn spellcheck_transcript_preserving_known_names(content: &str) -> String {
    // Spellcheck not enabled — return content unmodified.
    content.to_string()
}

pub fn normalize(file_path: &std::path::Path, content: &str) -> anyhow::Result<String> {
    use std::fs;

    if content.trim().is_empty() {
        return Ok(content.to_string());
    }

    if let Ok(metadata) = fs::metadata(file_path) {
        if metadata.len() > MAX_FILE_SIZE_BYTES {
            anyhow::bail!(
                "Content too large ({} bytes) to normalize: {} (cap: {} bytes)",
                metadata.len(),
                file_path.display(),
                MAX_FILE_SIZE_BYTES
            );
        }
    }

    let lines: Vec<&str> = content.split('\n').collect();
    let quote_count = lines.iter().filter(|l| l.trim().starts_with('>')).count();
    if quote_count >= 3 {
        return Ok(content.to_string());
    }

    let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if ext.eq_ignore_ascii_case("db") || ext.eq_ignore_ascii_case("sqlite") {
        // mr-kqrs: pass None for the session store here. The
        // content-only `normalize` entry point doesn't carry a
        // session store; callers with a store should use
        // `normalize_opencode_db` directly.
        if let Some(normalized) = normalize_opencode_db(file_path, None) {
            return Ok(normalized);
        }
    }
    if ext.eq_ignore_ascii_case("json")
        || ext.eq_ignore_ascii_case("jsonl")
        || content.trim().starts_with('{')
        || content.trim().starts_with('[')
    {
        if let Some(normalized) = try_normalize_json(content) {
            return Ok(normalized);
        }
    }

    Ok(content.to_string())
}

/// Parse a transcript file into structured `Vec<RawObservation>`.
///
/// Each parsed message (user or assistant) becomes a `RawObservation` with a
/// deterministic ID, provenance metadata, and the message text in the
/// appropriate field (`user_prompt` or `assistant_response`).
///
/// Returns `Ok(vec)` on success (possibly empty if the file had no
/// recognisable messages) or `Err` for I/O or size-limit violations.
pub fn normalize_to_observations(
    file_path: &std::path::Path,
    content: &str,
    session_id: &str,
) -> anyhow::Result<Vec<RawObservation>> {
    use std::fs;

    if content.trim().is_empty() {
        return Ok(Vec::new());
    }

    if let Ok(metadata) = fs::metadata(file_path) {
        if metadata.len() > MAX_FILE_SIZE_BYTES {
            anyhow::bail!(
                "Content too large ({} bytes) to parse observations: {} (cap: {} bytes)",
                metadata.len(),
                file_path.display(),
                MAX_FILE_SIZE_BYTES
            );
        }
    }

    let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let source_format = detect_format(content).unwrap_or_else(|| "plain_text".to_string());

    // For JSONL files, try streaming parse first for large files
    if ext.eq_ignore_ascii_case("jsonl") || content.len() > 1024 * 1024 {
        if let Some(obs) = parse_jsonl_to_observations(content, session_id, &source_format) {
            return Ok(obs);
        }
    }

    // For JSON files or content starting with {/[, try object-based parsing
    if ext.eq_ignore_ascii_case("json")
        || content.trim().starts_with('{')
        || content.trim().starts_with('[')
    {
        if let Some(transcript) = try_normalize_json(content) {
            if let Some(messages) = parse_transcript_to_messages(&transcript) {
                return Ok(messages_to_observations(
                    &messages,
                    session_id,
                    &source_format,
                ));
            }
        }
    }

    // Fall back to in-memory parsing
    let messages = extract_messages(content, &source_format);
    Ok(messages_to_observations(
        &messages,
        session_id,
        &source_format,
    ))
}

/// Streaming JSONL parser that processes lines one at a time.
///
/// This avoids loading the entire file into memory for large JSONL files.
/// Returns `None` if the content doesn't look like JSONL.
fn parse_jsonl_to_observations(
    content: &str,
    session_id: &str,
    source_format: &str,
) -> Option<Vec<RawObservation>> {
    let reader = std::io::Cursor::new(content.as_bytes());
    let buf_reader = std::io::BufReader::new(reader);

    let mut observations = Vec::new();
    let mut index: usize = 0;

    for line_result in buf_reader.lines() {
        let line = match line_result {
            Ok(l) => l,
            Err(_) => continue,
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let entry = match serde_json::from_str::<Value>(trimmed) {
            Ok(v) => v,
            Err(_) => continue,
        };

        if let Some((role, text)) = extract_message_from_entry(&entry, source_format) {
            if text.is_empty() {
                index += 1;
                continue;
            }
            let obs = make_observation(&role, &text, session_id, source_format, index, &entry);
            observations.push(obs);
            index += 1;
        }
    }

    if observations.is_empty() {
        None
    } else {
        Some(observations)
    }
}

/// Extract a (role, text) pair from a single JSONL entry based on source format.
fn extract_message_from_entry(entry: &Value, source_format: &str) -> Option<(String, String)> {
    let obj = entry.as_object()?;

    match source_format {
        "claude_code_jsonl" => {
            let msg_type = obj.get("type")?.as_str()?;
            let message = obj.get("message")?.as_object()?;
            let role = match msg_type {
                "human" | "user" => "user",
                "assistant" => "assistant",
                _ => return None,
            };
            let text = extract_content_to_string(message.get("content")?);
            Some((role.to_string(), text))
        }
        "codex_jsonl" => {
            let t = obj.get("type")?.as_str()?;
            let (role, text) = match t {
                "event_msg/user_message" => {
                    let text = obj.get("text")?.as_str()?;
                    ("user", text.to_string())
                }
                "event_msg/agent_message" => {
                    let text = obj.get("text")?.as_str()?;
                    ("assistant", text.to_string())
                }
                _ => return None,
            };
            Some((role.to_string(), text))
        }
        "gemini_cli_jsonl" => {
            let role_raw = obj
                .get("role")
                .or_else(|| obj.get("type"))
                .and_then(|r| r.as_str())?;
            let text = if let Some(c) = obj.get("content").and_then(|c| c.as_str()) {
                c.to_string()
            } else if let Some(parts) = obj.get("parts").and_then(|p| p.as_array()) {
                let buf: Vec<String> = parts
                    .iter()
                    .filter_map(|part| {
                        let p = part.as_object()?;
                        p.get("text").and_then(|t| t.as_str()).map(String::from)
                    })
                    .collect();
                buf.join("\n")
            } else {
                return None;
            };
            let role = match role_raw {
                "model" | "assistant" | "ai" => "assistant",
                "user" | "human" => "user",
                _ => return None,
            };
            Some((role.to_string(), text))
        }
        "pi_jsonl" => {
            let role_raw = obj
                .get("type")
                .or_else(|| obj.get("kind"))
                .and_then(|r| r.as_str())?;
            let text = obj
                .get("text")
                .or_else(|| obj.get("message"))
                .or_else(|| obj.get("content"))
                .and_then(|c| c.as_str())?
                .trim()
                .to_string();
            let role = match role_raw {
                "user" | "human" => "user",
                "pi" | "assistant" | "ai" => "assistant",
                _ => return None,
            };
            Some((role.to_string(), text))
        }
        "soulforge_jsonl" => {
            let role_raw = obj.get("role").and_then(|r| r.as_str())?;
            let text = if let Some(msg) = obj.get("message").and_then(|m| m.as_object()) {
                msg.get("text")
                    .or_else(|| {
                        msg.get("segments")
                            .and_then(|s| s.as_array())
                            .and_then(|arr| arr.first())
                            .and_then(|s| s.as_object())
                            .and_then(|s| s.get("text"))
                    })
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
            } else {
                obj.get("text").and_then(|t| t.as_str()).unwrap_or("")
            };
            let role = match role_raw {
                "user" | "human" => "user",
                "assistant" | "ai" | "agent" => "assistant",
                _ => return None,
            };
            Some((role.to_string(), text.to_string()))
        }
        _ => None,
    }
}

/// Build a `RawObservation` from parsed message data with a deterministic ID.
fn make_observation(
    role: &str,
    text: &str,
    session_id: &str,
    source_format: &str,
    index: usize,
    raw_entry: &Value,
) -> RawObservation {
    let id = deterministic_obs_id(source_format, index, text);
    let hook_type = match role {
        "user" => HookType::UserPromptSubmit,
        "assistant" => HookType::PostToolUse,
        _ => HookType::Notification,
    };

    RawObservation {
        id,
        session_id: session_id.to_string(),
        timestamp: Utc::now(),
        hook_type,
        tool_name: Some(source_format.to_string()),
        tool_input: None,
        tool_output: None,
        user_prompt: if role == "user" {
            Some(text.to_string())
        } else {
            None
        },
        assistant_response: if role == "assistant" {
            Some(text.to_string())
        } else {
            None
        },
        raw: Some(raw_entry.to_string()),
        modality: "text".to_string(),
        image_data: None,
        agent_id: Some(source_format.to_string()),
    }
}

/// Extract messages as `(role, text)` pairs from content based on format.
fn extract_messages(content: &str, source_format: &str) -> Vec<(String, String)> {
    match source_format {
        "pi_jsonl" => {
            // Already handled by try_normalize_json chain; extract inline
            let lines: Vec<&str> = content
                .trim()
                .split('\n')
                .filter(|l| !l.trim().is_empty())
                .collect();
            let mut messages = Vec::new();
            for line in lines {
                let Ok(v) = serde_json::from_str::<Value>(line) else {
                    continue;
                };
                if let Some((role, text)) = extract_message_from_entry(&v, source_format) {
                    if !text.is_empty() {
                        messages.push((role, text));
                    }
                }
            }
            messages
        }
        "claude_code_jsonl" | "codex_jsonl" | "gemini_cli_jsonl" | "soulforge_jsonl" => {
            let lines: Vec<&str> = content
                .trim()
                .split('\n')
                .filter(|l| !l.trim().is_empty())
                .collect();
            let mut messages = Vec::new();
            for line in lines {
                let Ok(v) = serde_json::from_str::<Value>(line) else {
                    continue;
                };
                if let Some((role, text)) = extract_message_from_entry(&v, source_format) {
                    if !text.is_empty() {
                        messages.push((role, text));
                    }
                }
            }
            messages
        }
        _ => {
            // For JSON object formats, try each parser
            if let Ok(data) = serde_json::from_str::<Value>(content) {
                if let Some(messages) = try_extract_messages_from_json(&data, source_format) {
                    return messages;
                }
            }
            Vec::new()
        }
    }
}

/// Try to extract messages from a parsed JSON value for object-based formats.
fn try_extract_messages_from_json(
    data: &Value,
    source_format: &str,
) -> Option<Vec<(String, String)>> {
    match source_format {
        "claude_ai_json"
        | "chatgpt_json"
        | "slack_json"
        | "gemini_ai_studio_json"
        | "continue_dev_json" => {
            // Use the existing parsers' logic to get messages
            let transcript = try_normalize_json(&data.to_string())?;
            // Parse back from transcript format
            parse_transcript_to_messages(&transcript)
        }
        _ => None,
    }
}

/// Parse a `>` prefixed transcript back into `(role, text)` pairs.
fn parse_transcript_to_messages(transcript: &str) -> Option<Vec<(String, String)>> {
    let mut messages = Vec::new();
    let mut current_user = String::new();
    let mut in_user = false;

    for line in transcript.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if in_user && !current_user.is_empty() {
                messages.push(("user".to_string(), current_user.trim().to_string()));
                current_user.clear();
                in_user = false;
            }
            continue;
        }

        if let Some(user_text) = trimmed.strip_prefix("> ") {
            if in_user && !current_user.is_empty() {
                messages.push(("user".to_string(), current_user.trim().to_string()));
                current_user.clear();
            }
            in_user = true;
            current_user = user_text.to_string();
        } else if in_user {
            // This is an assistant line following a user turn
            if !current_user.is_empty() {
                messages.push(("user".to_string(), current_user.trim().to_string()));
                current_user.clear();
            }
            in_user = false;
            messages.push(("assistant".to_string(), trimmed.to_string()));
        } else {
            // Continuation of assistant text
            if let Some(last) = messages.last_mut() {
                if last.0 == "assistant" {
                    last.1.push('\n');
                    last.1.push_str(trimmed);
                }
            }
        }
    }

    // Flush remaining user message
    if in_user && !current_user.is_empty() {
        messages.push(("user".to_string(), current_user.trim().to_string()));
    }

    if messages.len() >= 2 {
        Some(messages)
    } else {
        None
    }
}

/// Convert `(role, text)` pairs into `Vec<RawObservation>` with deterministic IDs.
fn messages_to_observations(
    messages: &[(String, String)],
    session_id: &str,
    source_format: &str,
) -> Vec<RawObservation> {
    messages
        .iter()
        .enumerate()
        .map(|(index, (role, text))| {
            make_observation(role, text, session_id, source_format, index, &Value::Null)
        })
        .collect()
}

fn try_normalize_json(content: &str) -> Option<String> {
    if let Some(normalized) = try_claude_code_jsonl(content) {
        return Some(normalized);
    }
    if let Some(normalized) = try_gemini_cli_jsonl(content) {
        return Some(normalized);
    }
    if let Some(normalized) = try_codex_jsonl(content) {
        return Some(normalized);
    }
    if let Some(normalized) = try_soulforge_jsonl(content) {
        return Some(normalized);
    }
    if let Some(normalized) = try_pi_jsonl(content) {
        return Some(normalized);
    }
    if let Some(normalized) = try_aider_md(content) {
        return Some(normalized);
    }

    let Ok(data) = serde_json::from_str::<Value>(content) else {
        return None;
    };

    for parser in [
        try_claude_ai_json,
        try_chatgpt_json,
        try_gemini_ai_studio_json,
        try_continue_dev_json,
        try_slack_json,
    ] {
        if let Some(normalized) = parser(&data) {
            return Some(normalized);
        }
    }

    None
}

/// Try to parse Claude Code JSONL session format.
///
/// Each line is a JSON object with `type` ("human"/"user"/"assistant") and
/// `message` containing `content` (string or array of content blocks).
///
/// Hardened against:
/// - Malformed lines (skipped silently)
/// - Missing fields (skipped, never panics)
/// - Large files (streamed line-by-line, not loaded entirely)
/// - `thinking` / `caching` content blocks (ignored gracefully)
/// - Empty assistant turns (collapsed with adjacent user turns)
fn try_claude_code_jsonl(content: &str) -> Option<String> {
    let lines: Vec<&str> = content
        .trim()
        .split('\n')
        .filter(|l| !l.trim().is_empty())
        .collect();
    let mut messages: Vec<(String, String)> = Vec::new();
    let mut tool_use_map: HashMap<String, String> = HashMap::new();

    for line in lines {
        let Ok(entry) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let Some(entry_obj) = entry.as_object() else {
            continue;
        };
        let msg_type = match entry_obj.get("type").and_then(|v| v.as_str()) {
            Some(t) => t,
            None => continue,
        };
        let message = match entry_obj.get("message").and_then(|v| v.as_object()) {
            Some(m) => m,
            None => continue,
        };

        match msg_type {
            "assistant" => {
                let Some(content_val) = message.get("content") else {
                    continue;
                };

                // Collect tool_use IDs from this assistant block
                if let Some(arr) = content_val.as_array() {
                    for block in arr {
                        let obj = match block.as_object() {
                            Some(o) => o,
                            None => continue,
                        };
                        let block_type = obj.get("type").and_then(|v| v.as_str());
                        if block_type != Some("tool_use") {
                            continue;
                        }
                        let tool_id = obj.get("id").and_then(|v| v.as_str()).unwrap_or("");
                        let tool_name = obj
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("Unknown");
                        tool_use_map.insert(tool_id.to_string(), tool_name.to_string());
                    }
                }

                let text = extract_claude_code_assistant_text(content_val, &tool_use_map);
                let text = strip_noise(&text);
                if text.is_empty() {
                    continue;
                }
                messages.push(("assistant".to_string(), text));
            }
            "human" | "user" => {
                let Some(content_val) = message.get("content") else {
                    continue;
                };
                let (user_text, is_tool_only) =
                    extract_claude_code_user_text(content_val, &tool_use_map);

                // Tool-only results get folded into the preceding assistant turn
                if is_tool_only {
                    if let Some(prev) = messages.last_mut() {
                        if prev.0 == "assistant" {
                            if !user_text.is_empty() {
                                prev.1.push('\n');
                                prev.1.push_str(&user_text);
                            }
                            continue;
                        }
                    }
                }

                if user_text.is_empty() {
                    continue;
                }
                messages.push(("user".to_string(), user_text));
            }
            // "summary", "system", "meta" etc. — skip
            _ => continue,
        }
    }

    if messages.len() >= 2 {
        return Some(messages_to_transcript(&messages));
    }
    None
}

fn try_claude_ai_json(data: &Value) -> Option<String> {
    let messages_data = if data.is_object() {
        data.get("messages")
            .or_else(|| data.get("chat_messages"))
            .unwrap_or(data)
    } else {
        data
    };

    let list = messages_data.as_array()?;
    let mut messages: Vec<(String, String)> = Vec::new();

    for item in list {
        let obj = item.as_object()?;
        let role = obj.get("role")?.as_str()?;
        let text = extract_content_to_string(obj.get("content")?);

        if text.is_empty() {
            continue;
        }

        if role == "user" || role == "human" {
            messages.push(("user".to_string(), text));
        } else if role == "assistant" || role == "ai" {
            messages.push(("assistant".to_string(), text));
        }
    }

    if messages.len() >= 2 {
        return Some(messages_to_transcript(&messages));
    }
    None
}

fn try_chatgpt_json(data: &Value) -> Option<String> {
    let mapping = data.get("mapping")?.as_object()?;

    let mut root_id: Option<&str> = None;
    let mut fallback_root: Option<&str> = None;

    for (node_id, node) in mapping {
        let node = node.as_object()?;
        let parent = node.get("parent");
        if parent.is_none() || parent?.is_null() {
            let msg = node.get("message");
            if msg.is_none() || msg?.is_null() {
                root_id = Some(node_id);
                break;
            } else if fallback_root.is_none() {
                fallback_root = Some(node_id);
            }
        }
    }

    let root_id = root_id.or(fallback_root)?;
    let mut messages: Vec<(String, String)> = Vec::new();
    let mut visited = std::collections::HashSet::new();
    let mut current_id: &str = root_id;

    while !current_id.is_empty() && !visited.contains(current_id) {
        visited.insert(current_id);
        let node = mapping.get(current_id)?.as_object()?;
        if let Some(msg_val) = node.get("message") {
            let msg = msg_val.as_object()?;
            let role = msg.get("author")?.as_object()?.get("role")?.as_str()?;
            let content_val = msg.get("content")?;

            let parts: Vec<String> = if content_val.is_array() {
                content_val
                    .as_array()?
                    .iter()
                    .filter_map(|p| p.as_str().map(String::from))
                    .collect()
            } else {
                Vec::new()
            };

            let text: String = parts.join(" ").trim().to_string();

            if text.is_empty() {
                let children = node.get("children")?.as_array()?;
                current_id = children.first()?.as_str().unwrap_or("");
                continue;
            }

            if role == "user" {
                messages.push(("user".to_string(), text));
            } else if role == "assistant" {
                messages.push(("assistant".to_string(), text));
            }
        }

        let children = node.get("children")?.as_array()?;
        current_id = children.first()?.as_str().unwrap_or("");
    }

    if messages.len() >= 2 {
        return Some(messages_to_transcript(&messages));
    }
    None
}

fn try_slack_json(data: &Value) -> Option<String> {
    let list = data.as_array()?;
    let mut messages: Vec<(String, String)> = Vec::new();
    let mut seen_users: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();
    let mut last_role: Option<&str> = None;

    for item in list {
        let obj = item.as_object()?;
        if obj.get("type")?.as_str() != Some("message") {
            continue;
        }

        let user_id = obj
            .get("user")
            .or_else(|| obj.get("username"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let text = obj.get("text")?.as_str().unwrap_or("").trim().to_string();

        if text.is_empty() || user_id.is_empty() {
            continue;
        }

        let role = if !seen_users.contains_key(user_id) {
            if seen_users.is_empty() {
                seen_users.insert(user_id, "user");
                "user"
            } else if last_role == Some("user") {
                seen_users.insert(user_id, "assistant");
                "assistant"
            } else {
                seen_users.insert(user_id, "user");
                "user"
            }
        } else {
            *seen_users.get(user_id).unwrap()
        };

        last_role = Some(role);
        messages.push((role.to_string(), text));
    }

    if messages.len() >= 2 {
        return Some(messages_to_transcript(&messages) + SLACK_PROVENANCE_FOOTER);
    }
    None
}

fn try_codex_jsonl(content: &str) -> Option<String> {
    let lines: Vec<&str> = content
        .trim()
        .split('\n')
        .filter(|l| !l.trim().is_empty())
        .collect();

    // Detect Codex format via session_meta presence
    let has_session_meta = lines.iter().any(|l| {
        if let Ok(v) = serde_json::from_str::<Value>(l) {
            if let Some(obj) = v.as_object() {
                if let Some(t) = obj.get("type").and_then(|v| v.as_str()) {
                    return t == "session_meta";
                }
            }
        }
        false
    });

    if !has_session_meta {
        return None;
    }

    let mut messages: Vec<(String, String)> = Vec::new();

    for line in lines {
        let Ok(entry) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let entry = entry.as_object()?;

        // Try flat format first: {"type": "event_msg/user_message", "text": "..."}
        let msg_type = entry.get("type").and_then(|v| v.as_str());

        let (text, role) = if let Some(t) = msg_type {
            if t == "event_msg/user_message" || t == "event_msg/agent_message" {
                let text = entry
                    .get("text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim();
                if text.is_empty() {
                    continue;
                }
                let role = if t == "event_msg/user_message" {
                    "user"
                } else {
                    "assistant"
                };
                (text.to_string(), role)
            } else if t == "event_msg" {
                // Try nested format: {"type": "event_msg", "payload": {"type": "user_message", "message": "..."}}
                let payload = entry.get("payload")?.as_object()?;
                let nested_type = payload.get("type")?.as_str()?;
                if nested_type != "user_message" && nested_type != "agent_message" {
                    continue;
                }
                let msg_content = payload.get("message")?;
                let text = extract_content_to_string(msg_content);
                if text.is_empty() {
                    continue;
                }
                let role = if nested_type == "user_message" {
                    "user"
                } else {
                    "assistant"
                };
                (text, role)
            } else {
                continue;
            }
        } else {
            continue;
        };

        messages.push((role.to_string(), text));
    }

    if messages.len() >= 2 {
        return Some(messages_to_transcript(&messages));
    }
    None
}

/// Try to parse Gemini CLI session JSONL format (mr-uzlo).
///
/// Gemini CLI persists sessions as JSONL where each line is a JSON object
/// describing one event. Two shapes are supported:
///
/// * `{ "role": "user" | "model", "content": "..." }` — simple string content.
/// * `{ "type": "user" | "model", "parts": [{ "text": "..." }, ...] }` — the
///   newer Gemini SDK shape where content is split into typed parts.
///
/// Detection: at least one line must carry `role`/`type` in
/// `{user, model, system}` AND a `content`/`parts` field. We require ≥2
/// messages so we don't transcribe lone ping/pong handshakes.
fn try_gemini_cli_jsonl(content: &str) -> Option<String> {
    let lines: Vec<&str> = content
        .trim()
        .split('\n')
        .filter(|l| !l.trim().is_empty())
        .collect();
    if lines.is_empty() {
        return None;
    }

    let mut has_gemini_marker = false;
    for line in &lines {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let Some(obj) = v.as_object() else {
            continue;
        };
        let role = obj
            .get("role")
            .and_then(|r| r.as_str())
            .or_else(|| obj.get("type").and_then(|r| r.as_str()));
        let is_known_role = matches!(role, Some("user") | Some("model") | Some("system"));
        let has_payload = obj.contains_key("content") || obj.contains_key("parts");
        if is_known_role && has_payload {
            has_gemini_marker = true;
            break;
        }
    }
    if !has_gemini_marker {
        return None;
    }

    let mut messages: Vec<(String, String)> = Vec::new();
    for line in &lines {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let Some(obj) = v.as_object() else {
            continue;
        };

        // Extract text from either `content` (string) or `parts[]` (text parts).
        let text: Option<String> = if let Some(c) = obj.get("content").and_then(|c| c.as_str()) {
            Some(c.to_string())
        } else if let Some(parts) = obj.get("parts").and_then(|p| p.as_array()) {
            let buf: Vec<String> = parts
                .iter()
                .filter_map(|part| {
                    let p = part.as_object()?;
                    p.get("text").and_then(|t| t.as_str()).map(String::from)
                })
                .collect();
            if buf.is_empty() {
                None
            } else {
                Some(buf.join("\n"))
            }
        } else {
            None
        };

        let Some(text) = text else {
            continue;
        };
        let text = text.trim().to_string();
        if text.is_empty() {
            continue;
        }

        let role_raw = obj
            .get("role")
            .and_then(|r| r.as_str())
            .or_else(|| obj.get("type").and_then(|r| r.as_str()))
            .unwrap_or("user");
        let role = match role_raw {
            "model" | "assistant" | "ai" => "assistant",
            "user" | "human" => "user",
            // Drop system/metadata rows; they belong in session config, not
            // the conversation transcript.
            "system" => continue,
            _ => "user",
        };
        messages.push((role.to_string(), text));
    }

    if messages.len() >= 2 {
        return Some(messages_to_transcript(&messages));
    }
    None
}

fn try_soulforge_jsonl(content: &str) -> Option<String> {
    let lines: Vec<&str> = content
        .trim()
        .split('\n')
        .filter(|l| !l.trim().is_empty())
        .collect();

    // Detect SoulForge via unique fields: segments, toolCalls, durationMs
    let has_soulforge_marker = lines.iter().any(|l| {
        if let Ok(v) = serde_json::from_str::<Value>(l) {
            if let Some(obj) = v.as_object() {
                // Check for SoulForge-specific fields
                if obj.contains_key("segments")
                    || obj.contains_key("toolCalls")
                    || obj.contains_key("durationMs")
                {
                    return true;
                }
                // Also check message content for segments array or toolCalls
                if let Some(msg) = obj.get("message").and_then(|m| m.as_object()) {
                    if msg.contains_key("segments")
                        || msg.contains_key("toolCalls")
                        || msg.contains_key("durationMs")
                    {
                        return true;
                    }
                }
            }
        }
        false
    });

    if !has_soulforge_marker {
        return None;
    }

    let mut messages: Vec<(String, String)> = Vec::new();

    for line in lines {
        let Ok(entry) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let entry = entry.as_object()?;

        // Get message content - could be in message.segments or directly in message.text
        let text = if let Some(msg) = entry.get("message").and_then(|m| m.as_object()) {
            if let Some(segments) = msg.get("segments").and_then(|s| s.as_array()) {
                // Extract text from segments array
                let parts: Vec<String> = segments
                    .iter()
                    .filter_map(|seg| seg.as_object()?.get("text")?.as_str().map(String::from))
                    .collect();
                let text = parts.join(" ");
                if !text.is_empty() {
                    Some(text)
                } else {
                    msg.get("text")?.as_str().map(String::from)
                }
            } else {
                msg.get("text")?.as_str().map(String::from)
            }
        } else {
            entry.get("text")?.as_str().map(String::from)
        };

        let Some(text) = text else {
            continue;
        };
        let text = text.trim().to_string();
        if text.is_empty() {
            continue;
        }

        // Determine role - SoulForge has user/assistant markers
        let role = entry
            .get("role")
            .and_then(|r| r.as_str())
            .or_else(|| entry.get("type").and_then(|t| t.as_str()))
            .unwrap_or("");

        // Summarize tool calls if present (inside message object)
        let final_text = if role == "assistant" || role == "agent" {
            if let Some(msg) = entry.get("message").and_then(|m| m.as_object()) {
                if let Some(tool_calls) = msg.get("toolCalls").and_then(|tc| tc.as_array()) {
                    if !tool_calls.is_empty() {
                        let tool_names: Vec<String> = tool_calls
                            .iter()
                            .filter_map(|tc| {
                                tc.as_object()?.get("name")?.as_str().map(String::from)
                            })
                            .collect();
                        if !tool_names.is_empty() {
                            format!("{} [tools: {}]", text, tool_names.join(", "))
                        } else {
                            text
                        }
                    } else {
                        text
                    }
                } else {
                    text
                }
            } else {
                text
            }
        } else {
            text
        };

        match role {
            "user" | "human" => messages.push(("user".to_string(), final_text)),
            "assistant" | "ai" | "agent" => messages.push(("assistant".to_string(), final_text)),
            // Skip system messages
            "system" => continue,
            _ => {
                // If role is unknown, alternate based on position
                if messages.is_empty()
                    || messages.last().map(|m| m.0 == "assistant").unwrap_or(false)
                {
                    messages.push(("user".to_string(), final_text));
                } else {
                    messages.push(("assistant".to_string(), final_text));
                }
            }
        }
    }

    if messages.len() >= 2 {
        return Some(messages_to_transcript(&messages));
    }
    None
}

/// Try to parse Google AI Studio (Gemini AI Studio) JSON export format.
///
/// AI Studio exports conversations as a JSON array where each element is a
/// conversation with a `messages` or `contents` array. Each message has
/// `role` and either `parts` (array of `{text}` objects) or `content` (string).
///
/// Detection: JSON array where first element has `messages` or `contents` key
/// and at least one message has `role` in {user, model, assistant}.
fn try_gemini_ai_studio_json(data: &Value) -> Option<String> {
    // AI Studio can export as a single conversation object or array of conversations
    // Normalize to a list of conversation values to iterate
    let conv_list: Vec<&Value> = if let Some(arr) = data.as_array() {
        arr.iter().collect()
    } else {
        // Single conversation object
        vec![data]
    };

    // Find the first conversation that has recognizable messages
    for conv in conv_list {
        let messages_data = conv
            .get("messages")
            .or_else(|| conv.get("contents"))
            .or_else(|| conv.get("conversation"))
            .or_else(|| conv.get("turns"));

        let list = match messages_data.and_then(|v| v.as_array()) {
            Some(l) => l,
            None => continue,
        };

        let mut messages: Vec<(String, String)> = Vec::new();

        for item in list {
            let obj = match item.as_object() {
                Some(o) => o,
                None => continue,
            };

            let role = obj.get("role").and_then(|v| v.as_str()).unwrap_or("");

            let text = if let Some(parts) = obj.get("parts").and_then(|p| p.as_array()) {
                let buf: Vec<String> = parts
                    .iter()
                    .filter_map(|part| {
                        let p = part.as_object()?;
                        p.get("text").and_then(|t| t.as_str()).map(String::from)
                    })
                    .collect();
                buf.join("\n")
            } else {
                extract_content_to_string(obj.get("content")?)
            };

            if text.is_empty() {
                continue;
            }

            match role {
                "user" | "human" => messages.push(("user".to_string(), text)),
                "model" | "assistant" | "ai" => messages.push(("assistant".to_string(), text)),
                "system" => continue,
                _ => continue,
            }
        }

        if messages.len() >= 2 {
            return Some(messages_to_transcript(&messages));
        }
    }

    None
}

/// Try to parse Pi AI (Inflection) JSONL session format.
///
/// Pi JSONL files have one JSON object per line with fields:
/// - `type` or `kind`: "user" | "pi" | "assistant"
/// - `text` or `message` or `content`: the message body
/// - `createdAt` or `timestamp`: optional ISO-8601 timestamp
///
/// Detection: at least one line has `type`/`kind` in {user, pi, assistant}
/// AND a `text`/`message`/`content` field.
fn try_pi_jsonl(content: &str) -> Option<String> {
    let lines: Vec<&str> = content
        .trim()
        .split('\n')
        .filter(|l| !l.trim().is_empty())
        .collect();
    if lines.is_empty() {
        return None;
    }

    // Quick detection: scan first few lines for Pi markers
    let mut has_pi_marker = false;
    for line in lines.iter().take(20) {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let Some(obj) = v.as_object() else {
            continue;
        };
        let role = obj
            .get("type")
            .or_else(|| obj.get("kind"))
            .and_then(|r| r.as_str());
        let is_pi_role = matches!(role, Some("user") | Some("pi") | Some("assistant"));
        let has_payload =
            obj.contains_key("text") || obj.contains_key("message") || obj.contains_key("content");
        if is_pi_role && has_payload {
            has_pi_marker = true;
            break;
        }
    }
    if !has_pi_marker {
        return None;
    }

    let mut messages: Vec<(String, String)> = Vec::new();

    for line in lines {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let Some(obj) = v.as_object() else {
            continue;
        };

        let role_raw = obj
            .get("type")
            .or_else(|| obj.get("kind"))
            .and_then(|r| r.as_str())
            .unwrap_or("user");

        let text = obj
            .get("text")
            .or_else(|| obj.get("message"))
            .or_else(|| obj.get("content"))
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .trim()
            .to_string();

        if text.is_empty() {
            continue;
        }

        match role_raw {
            "user" | "human" => messages.push(("user".to_string(), text)),
            "pi" | "assistant" | "ai" => messages.push(("assistant".to_string(), text)),
            _ => continue,
        }
    }

    if messages.len() >= 2 {
        Some(messages_to_transcript(&messages))
    } else {
        None
    }
}

/// Try to parse Continue.dev conversation JSON format.
///
/// Continue.dev stores conversations as JSON with a top-level object containing
/// either:
/// - `history`: array of `{ role, content }` or `{ role, parts }` messages
/// - `messages`: same structure
/// - A nested `conversation` or `chat` object with a messages array
///
/// Each message may have `content` as string or array of content blocks.
///
/// Detection: JSON object with `history` or `messages` key containing objects
/// with `role` in {user, assistant, system} and recognizable content.
fn try_continue_dev_json(data: &Value) -> Option<String> {
    let obj = data.as_object()?;

    // Continue.dev uses "history" or "messages" at top level, or nested
    let messages_data = obj
        .get("history")
        .or_else(|| obj.get("messages"))
        .or_else(|| obj.get("chatHistory"))
        .or_else(|| obj.get("conversation"));

    // Also check for nested conversation object
    let messages_data = if messages_data.is_some() {
        messages_data
    } else {
        // Try nested: obj.conversation.messages or obj.chat.messages
        let nested = obj
            .get("conversation")
            .or_else(|| obj.get("chat"))
            .and_then(|n| n.as_object());
        if let Some(nested_obj) = nested {
            nested_obj
                .get("messages")
                .or_else(|| nested_obj.get("history"))
        } else {
            None
        }
    };

    let list = messages_data?.as_array()?;

    // Check for Continue.dev markers: role field with content/parts
    let has_continue_marker = list.iter().any(|item| {
        if let Some(obj) = item.as_object() {
            let role = obj.get("role").and_then(|r| r.as_str());
            let has_content = obj.contains_key("content") || obj.contains_key("parts");
            matches!(role, Some("user") | Some("assistant") | Some("system")) && has_content
        } else {
            false
        }
    });

    if !has_continue_marker {
        return None;
    }

    let mut messages: Vec<(String, String)> = Vec::new();

    for item in list {
        let obj = match item.as_object() {
            Some(o) => o,
            None => continue,
        };

        let role = obj.get("role").and_then(|v| v.as_str()).unwrap_or("");

        let text = if let Some(parts) = obj.get("parts").and_then(|p| p.as_array()) {
            let buf: Vec<String> = parts
                .iter()
                .filter_map(|part| {
                    if let Some(s) = part.as_str() {
                        return Some(s.to_string());
                    }
                    let p = part.as_object()?;
                    p.get("text").and_then(|t| t.as_str()).map(String::from)
                })
                .collect();
            buf.join("\n")
        } else {
            extract_content_to_string(obj.get("content")?)
        };

        if text.is_empty() {
            continue;
        }

        match role {
            "user" | "human" => messages.push(("user".to_string(), text)),
            "assistant" | "ai" => messages.push(("assistant".to_string(), text)),
            "system" => continue,
            _ => continue,
        }
    }

    if messages.len() >= 2 {
        Some(messages_to_transcript(&messages))
    } else {
        None
    }
}

/// Try to parse Aider .aider.chat.history.md format.
/// Format: Lines starting with "> " are user turns, other lines are assistant responses.
/// Detected by: presence of "# Aider Chat History" header or "> " quoted lines.
/// Try to parse OpenCode SQLite database format.
/// Reads sessions from an OpenCode session SQLite database file.
/// Detected by: file extension is .db or .sqlite and contains OpenCode schema.
/// mr-kqrs (B15): try_opencode_sqlite — detect if a buffer looks like
/// an OpenCode SQLite database file (by magic header) and return a
/// short tag. Real parsing happens in `normalize_opencode_db` against
/// a path; this entry point is the content-sniffing variant.
///
/// The optional `sessions` argument, when supplied, is asked to
/// `ensure_session` for every session id we discover, so that later
/// observation inserts (in `add_observation`) don't trip the
/// observations.session_id foreign key.
pub fn try_opencode_sqlite(
    content: &str,
    sessions: Option<&crate::session::SessionStore>,
) -> Option<String> {
    // OpenCode SQLite databases start with the SQLite header
    // ("SQLite format 3\0"). Sniff the first 16 bytes.
    let head = &content.as_bytes()[..content.len().min(16)];
    if head.starts_with(b"SQLite format 3") {
        if let Some(store) = sessions {
            // The path is unknowable from a buffer alone, but a
            // content-only caller is the rare path. We do best-effort
            // session provisioning by hashing a synthetic id from the
            // buffer; real callers (with a db_path) use
            // `normalize_opencode_db` instead, which DOES know the id.
            let _ = store; // content-only path: skip
        }
        return Some("opencode-sqlite".to_string());
    }
    None
}

/// Try to parse OpenCode SQLite database from file path.
/// Returns transcript format for sessions found.
///
/// mr-kqrs (B15): when an optional `SessionStore` is supplied, we
/// `ensure_session` for every discovered OpenCode session id, so
/// downstream observation inserts (via `add_observation`) never trip
/// the FK.
pub fn normalize_opencode_db(
    db_path: &std::path::Path,
    sessions: Option<&crate::session::SessionStore>,
) -> Option<String> {
    let conn = rusqlite::Connection::open(db_path).ok()?;

    // Query the session table to get conversation history
    let mut stmt = conn
        .prepare("SELECT id, dir, created_at, updated_at FROM sessions ORDER BY created_at")
        .ok()?;

    let sessions_oc: Vec<(i64, String, String, String)> = stmt
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .ok()?
        .filter_map(|r| r.ok())
        .collect();

    if sessions_oc.is_empty() {
        return None;
    }

    // mr-kqrs: ensure_session on every OpenCode session id.
    if let Some(store) = sessions {
        for (session_id, dir, _created, _updated) in &sessions_oc {
            // Project name falls back to the parent dir name.
            let project = std::path::Path::new(dir)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("opencode")
                .to_string();
            let sid = format!("opencode:{}", session_id);
            let _ = store.ensure_session(&sid, &project, dir);
        }
    }

    let mut messages: Vec<(String, String)> = Vec::new();

    for (session_id, _dir, _created, _updated) in sessions_oc {
        // Try to get messages for this session
        if let Ok(mut msg_stmt) =
            conn.prepare("SELECT role, content FROM messages WHERE session_id = ? ORDER BY id")
        {
            let rows = msg_stmt
                .query_map([session_id], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .ok()?;

            for row in rows.flatten() {
                let (role, content) = row;
                if content.trim().is_empty() {
                    continue;
                }
                match role.as_str() {
                    "user" | "human" => {
                        messages.push(("user".to_string(), content.trim().to_string()));
                    }
                    "assistant" | "ai" | "bot" => {
                        messages.push(("assistant".to_string(), content.trim().to_string()));
                    }
                    _ => {}
                }
            }
        }
    }

    if messages.len() >= 2 {
        Some(messages_to_transcript(&messages))
    } else {
        None
    }
}

fn try_aider_md(content: &str) -> Option<String> {
    let trimmed = content.trim();

    // Check for Aider format markers
    let has_header =
        trimmed.contains("Aider Chat History") || trimmed.contains("aider.chat.history");
    let has_quoted_lines = trimmed
        .lines()
        .filter(|l| l.trim().starts_with("> "))
        .count()
        >= 2;

    if !has_header && !has_quoted_lines {
        return None;
    }

    let mut messages: Vec<(String, String)> = Vec::new();
    let mut current_assistant = String::new();

    for line in content.lines() {
        let trimmed_line = line.trim();
        if trimmed_line.is_empty() {
            continue;
        }

        if trimmed_line.starts_with("> ") {
            // Save previous assistant message if any
            if !current_assistant.is_empty() {
                messages.push((
                    "assistant".to_string(),
                    current_assistant.trim().to_string(),
                ));
                current_assistant.clear();
            }

            // User message (strip the "> " prefix)
            let user_text = trimmed_line
                .strip_prefix("> ")
                .unwrap_or(trimmed_line)
                .trim()
                .to_string();
            if !user_text.is_empty() {
                messages.push(("user".to_string(), user_text));
            }
        } else if trimmed_line.starts_with("#") {
            // Skip markdown headers
            continue;
        } else if trimmed_line.starts_with("```") {
            // Skip code blocks markers
            continue;
        } else {
            // Accumulate as assistant response
            if !current_assistant.is_empty() {
                current_assistant.push('\n');
            }
            current_assistant.push_str(trimmed_line);
        }
    }

    // Don't forget the last assistant message
    if !current_assistant.is_empty() {
        messages.push((
            "assistant".to_string(),
            current_assistant.trim().to_string(),
        ));
    }

    if messages.len() >= 2 {
        Some(messages_to_transcript(&messages))
    } else {
        None
    }
}

fn extract_content_to_string(content: &Value) -> String {
    match content {
        Value::String(s) => s.trim().to_string(),
        Value::Array(arr) => {
            let parts: Vec<String> = arr
                .iter()
                .filter_map(|item| match item {
                    Value::String(s) => Some(s.trim().to_string()),
                    Value::Object(obj) if obj.get("type")?.as_str() == Some("text") => {
                        obj.get("text")?.as_str().map(|s| s.trim().to_string())
                    }
                    _ => None,
                })
                .collect();
            parts.join(" ")
        }
        Value::Object(obj) => obj
            .get("text")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .unwrap_or_default(),
        _ => String::new(),
    }
}

fn extract_claude_code_assistant_text(
    content: &Value,
    tool_use_map: &HashMap<String, String>,
) -> String {
    match content {
        Value::String(s) => s.trim().to_string(),
        Value::Array(arr) => {
            let mut parts: Vec<String> = Vec::new();
            for item in arr {
                let obj = match item.as_object() {
                    Some(o) => o,
                    None => continue,
                };
                let item_type = obj.get("type").and_then(|v| v.as_str()).unwrap_or("");

                match item_type {
                    "text" => {
                        if let Some(text) = obj.get("text").and_then(|v| v.as_str()) {
                            let trimmed = text.trim();
                            if !trimmed.is_empty() {
                                parts.push(trimmed.to_string());
                            }
                        }
                    }
                    "tool_use" => {
                        let formatted = format_tool_use(item);
                        if !formatted.is_empty() {
                            parts.push(formatted);
                        }
                    }
                    "tool_result" => {
                        let tool_id = obj
                            .get("tool_use_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let tool_name = tool_use_map.get(tool_id).map(|s| s.as_str());
                        let formatted = format_tool_result(item, tool_name);
                        if !formatted.is_empty() {
                            parts.push(formatted);
                        }
                    }
                    // thinking / thinking_content blocks are internal
                    // reasoning — skip silently
                    "thinking" | "thinking_content" => {}
                    // cache_control is metadata, not content
                    _ => {}
                }
            }
            parts.join(" ")
        }
        Value::Object(obj) => obj
            .get("text")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .unwrap_or_default(),
        _ => String::new(),
    }
}

fn extract_claude_code_user_text(
    content: &Value,
    tool_use_map: &HashMap<String, String>,
) -> (String, bool) {
    if let Value::String(s) = content {
        return (s.trim().to_string(), false);
    }

    let arr = match content.as_array() {
        Some(a) => a,
        None => return (String::new(), false),
    };

    let mut parts: Vec<String> = Vec::new();
    let mut has_tool_result = false;
    let mut has_user_text = false;

    for item in arr {
        let obj = match item.as_object() {
            Some(o) => o,
            None => continue,
        };
        let item_type = obj.get("type").and_then(|v| v.as_str()).unwrap_or("");

        if item_type == "text" {
            if let Some(text) = obj.get("text").and_then(|v| v.as_str()) {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    parts.push(trimmed.to_string());
                    has_user_text = true;
                }
            }
        } else if item_type == "tool_result" {
            has_tool_result = true;
            let tool_id = obj
                .get("tool_use_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let tool_name = tool_use_map.get(tool_id).map(|s| s.as_str());
            let formatted = format_tool_result(item, tool_name);
            if !formatted.is_empty() {
                parts.push(formatted);
            }
        }
    }

    (parts.join(" "), has_tool_result && !has_user_text)
}

fn messages_to_transcript(messages: &[(String, String)]) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut i = 0;

    while i < messages.len() {
        let (ref role, ref text) = messages[i];

        if role == "user" {
            lines.push(format!("> {}", text));
            if i + 1 < messages.len() && messages[i + 1].0 == "assistant" {
                lines.push(messages[i + 1].1.clone());
                i += 2;
            } else {
                i += 1;
            }
        } else {
            lines.push(text.clone());
            i += 1;
        }
        lines.push(String::new());
    }

    spellcheck_transcript_preserving_known_names(&lines.join("\n"))
}

pub fn detect_format(content: &str) -> Option<String> {
    let trimmed = content.trim();
    let lines: Vec<&str> = content.split('\n').collect();

    // Check for Codex JSONL by scanning all lines for session_meta
    let has_session_meta = lines.iter().any(|l| {
        if let Ok(v) = serde_json::from_str::<Value>(l) {
            if let Some(obj) = v.as_object() {
                if let Some(t) = obj.get("type").and_then(|v| v.as_str()) {
                    return t == "session_meta";
                }
            }
        }
        false
    });
    if has_session_meta {
        return Some("codex_jsonl".to_string());
    }

    // Check for SoulForge JSONL
    let has_soulforge = lines.iter().any(|l| {
        if let Ok(v) = serde_json::from_str::<Value>(l) {
            if let Some(obj) = v.as_object() {
                // Top-level markers
                if obj.contains_key("segments")
                    || obj.contains_key("toolCalls")
                    || obj.contains_key("durationMs")
                {
                    return true;
                }
                // Also check inside "message" object
                if let Some(msg) = obj.get("message").and_then(|m| m.as_object()) {
                    if msg.contains_key("segments")
                        || msg.contains_key("toolCalls")
                        || msg.contains_key("durationMs")
                    {
                        return true;
                    }
                }
            }
        }
        false
    });
    if has_soulforge {
        return Some("soulforge_jsonl".to_string());
    }

    // Check for Pi JSONL (type/kind in {user, pi, assistant} + text/message/content)
    let has_pi = lines.iter().take(20).any(|l| {
        if let Ok(v) = serde_json::from_str::<Value>(l) {
            if let Some(obj) = v.as_object() {
                let role = obj
                    .get("type")
                    .or_else(|| obj.get("kind"))
                    .and_then(|r| r.as_str());
                let is_pi_role = matches!(role, Some("user") | Some("pi") | Some("assistant"));
                let has_payload = obj.contains_key("text")
                    || obj.contains_key("message")
                    || obj.contains_key("content");
                return is_pi_role && has_payload;
            }
        }
        false
    });
    if has_pi {
        return Some("pi_jsonl".to_string());
    }

    // Check for Aider markdown format
    let has_aider =
        trimmed.contains("Aider Chat History") || trimmed.contains("aider.chat.history");
    if has_aider {
        return Some("aider_md".to_string());
    }

    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        // Try parsing as single JSON first
        if let Ok(data) = serde_json::from_str::<Value>(trimmed) {
            if let Some(obj) = data.as_object() {
                if let Some(t) = obj.get("type").and_then(|v| v.as_str()) {
                    if t == "conversation" {
                        return Some("claude_code_jsonl".to_string());
                    }
                }
                if obj.get("messages").is_some() || obj.get("chat_messages").is_some() {
                    return Some("claude_ai_json".to_string());
                }
                if obj.get("mapping").is_some() {
                    return Some("chatgpt_json".to_string());
                }
                // Gemini AI Studio single-object format
                if obj.get("contents").is_some() {
                    return Some("gemini_ai_studio_json".to_string());
                }
                // Continue.dev: has "history" or "messages" with role-based entries
                if let Some(hist) = obj.get("history").or_else(|| obj.get("chatHistory")) {
                    if let Some(arr) = hist.as_array() {
                        if arr.iter().any(|item| {
                            item.as_object()
                                .and_then(|o| o.get("role"))
                                .and_then(|r| r.as_str())
                                .map(|r| matches!(r, "user" | "assistant"))
                                .unwrap_or(false)
                        }) {
                            return Some("continue_dev_json".to_string());
                        }
                    }
                }
            }
            if let Some(arr) = data.as_array() {
                if let Some(first) = arr.first() {
                    let first_obj = first.as_object();
                    if first_obj
                        .and_then(|o| o.get("type"))
                        .and_then(|v| v.as_str())
                        == Some("message")
                    {
                        return Some("slack_json".to_string());
                    }
                    // Gemini AI Studio: array of objects with messages/contents
                    if first_obj
                        .and_then(|o| o.get("messages").or_else(|| o.get("contents")))
                        .and_then(|v| v.as_array())
                        .is_some()
                    {
                        return Some("gemini_ai_studio_json".to_string());
                    }
                }
            }
        } else if !lines.is_empty() {
            // Try parsing first line as JSON (for JSONL formats)
            if let Ok(first) = serde_json::from_str::<Value>(lines[0].trim()) {
                if let Some(obj) = first.as_object() {
                    if let Some(t) = obj.get("type").and_then(|v| v.as_str()) {
                        if t == "conversation" {
                            return Some("claude_code_jsonl".to_string());
                        }
                        // Claude Code JSONL also uses type: "human"/"user"/"assistant"
                        if matches!(t, "human" | "user" | "assistant")
                            && obj.contains_key("message")
                        {
                            return Some("claude_code_jsonl".to_string());
                        }
                    }
                }
            }
        }
    }

    let quote_count = lines.iter().filter(|l| l.trim().starts_with('>')).count();
    if quote_count >= 3 {
        return Some("transcript".to_string());
    }

    Some("plain_text".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plain_text_pass_through() {
        let content = "This is plain text\nwithout any markers";
        let result = normalize(std::path::Path::new("test.txt"), content).unwrap();
        assert_eq!(result, content);
    }

    #[test]
    fn test_transcript_pass_through() {
        let content = "> user message\nassistant response\n> another user";
        let result = normalize(std::path::Path::new("test.txt"), content).unwrap();
        assert_eq!(result, content);
    }

    #[test]
    fn test_claude_code_jsonl() {
        let content = r#"{"type":"human","message":{"content":"Hello"}}
{"type":"assistant","message":{"content":"Hi there"}}"#;
        let result = normalize(std::path::Path::new("test.jsonl"), content).unwrap();
        assert!(result.contains("> Hello"));
        assert!(result.contains("Hi there"));
    }

    #[test]
    fn test_empty_content() {
        let result = normalize(std::path::Path::new("test.txt"), "").unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_codex_jsonl() {
        let content = r#"{"type":"session_meta","sessionId":"abc123","model":"gpt-4"}
{"type":"event_msg/user_message","text":"Hello Codex"}
{"type":"event_msg/agent_message","text":"Hello from Codex agent"}"#;
        let result = normalize(std::path::Path::new("test.jsonl"), content).unwrap();
        assert!(result.contains("> Hello Codex"));
        assert!(result.contains("Hello from Codex agent"));
    }

    #[test]
    fn test_gemini_cli_jsonl() {
        // mr-uzlo: Gemini CLI session JSONL must normalize into the same
        // transcript format as the other adapters, with user/model roles
        // mapped to user/assistant.
        let content = r#"{"role":"user","content":"Hi Gemini"}
{"role":"model","content":"Hi! How can I help today?"}"#;
        let result = normalize(std::path::Path::new("session.jsonl"), content).unwrap();
        assert!(result.contains("> Hi Gemini"));
        assert!(result.contains("Hi! How can I help today?"));
        // The model-side message must NOT be wrapped in a user turn marker.
        assert!(!result.contains("> Hi! How can I help today?"));
    }

    #[test]
    fn test_gemini_cli_jsonl_handles_parts_array() {
        // mr-uzlo: the newer Gemini SDK emits `parts: [{ "text": "..." }]`
        // instead of a single `content` string. The adapter must join the
        // text parts in order.
        let content = r#"{"role":"user","parts":[{"text":"line one"},{"text":"line two"}]}
{"type":"model","parts":[{"text":"ack"}]}"#;
        let result = normalize(std::path::Path::new("session.jsonl"), content).unwrap();
        assert!(result.contains("line one"));
        assert!(result.contains("line two"));
        assert!(result.contains("ack"));
    }

    #[test]
    fn test_messages_to_transcript_routes_user_lines_through_spellcheck_path() {
        let transcript = messages_to_transcript(&[
            ("user".to_string(), "hello world test message".to_string()),
            (
                "assistant".to_string(),
                "Assistant text should stay untouched.".to_string(),
            ),
        ]);

        assert!(transcript.starts_with("> hello world test message"));
        assert!(transcript.contains("Assistant text should stay untouched."));
    }

    #[test]
    fn test_messages_to_transcript_preserves_assistant_lines() {
        let transcript = messages_to_transcript(&[
            (
                "assistant".to_string(),
                "MemPalace ChromaDB NDCG@10".to_string(),
            ),
            ("user".to_string(), "hello world".to_string()),
        ]);

        assert!(transcript.contains("MemPalace ChromaDB NDCG@10"));
    }

    #[test]
    fn test_codex_jsonl_skips_response_items() {
        // response_item entries should be skipped
        let content = r#"{"type":"session_meta","sessionId":"abc123","model":"gpt-4"}
{"type":"event_msg/user_message","text":"Hello"}
{"type":"response_item","text":"Should be skipped"}
{"type":"event_msg/agent_message","text":"Real response"}"#;
        let result = normalize(std::path::Path::new("test.jsonl"), content).unwrap();
        assert!(result.contains("> Hello"));
        assert!(result.contains("Real response"));
        assert!(!result.contains("Should be skipped"));
    }

    #[test]
    fn test_codex_jsonl_rejects_non_codex() {
        // Other JSONL format should not be detected as Codex
        let content = r#"{"type":"event","data":"something"}"#;
        let result = detect_format(content);
        // Should not be codex (no session_meta)
        assert_ne!(result, Some("codex_jsonl".to_string()));
    }

    #[test]
    fn test_detect_format_codex() {
        let content = r#"{"type":"session_meta","sessionId":"abc123"}
{"type":"event_msg/user_message","text":"Hello"}"#;
        let result = detect_format(content);
        assert_eq!(result, Some("codex_jsonl".to_string()));
    }

    #[test]
    fn test_soulforge_jsonl() {
        let content = r#"{"role":"user","message":{"text":"Hello SoulForge"}}
{"role":"assistant","message":{"text":"Hello from SoulForge"}}"#;
        let result = normalize(std::path::Path::new("test.jsonl"), content);
        assert!(result.is_ok());
    }

    #[test]
    fn test_soulforge_with_segments() {
        let content = r#"{"role":"user","message":{"segments":[{"text":"Hello"}]}}
{"role":"assistant","message":{"segments":[{"text":"Response"}]}}"#;
        let result = normalize(std::path::Path::new("test.jsonl"), content);
        assert!(result.is_ok());
    }

    #[test]
    fn test_soulforge_with_tool_calls() {
        let content = r#"{"role":"user","message":{"text":"Run a command"}}
{"role":"assistant","message":{"text":"Running...","toolCalls":[{"name":"bash","input":"ls"}]}}"#;
        let result = normalize(std::path::Path::new("test.jsonl"), content);
        assert!(result.is_ok());
        let r = result.unwrap();
        // Tool calls should be summarized
        assert!(r.contains("[tools:"));
    }

    #[test]
    fn test_detect_format_soulforge() {
        let content = r#"{"role":"user","message":{"text":"Hello"}}
{"role":"assistant","message":{"segments":[{"text":"Hi"}]}}"#;
        let result = detect_format(content);
        assert_eq!(result, Some("soulforge_jsonl".to_string()));
    }

    #[test]
    fn test_detect_format() {
        assert_eq!(
            detect_format(r#"{"messages": []}"#).unwrap(),
            "claude_ai_json"
        );
        assert_eq!(detect_format(r#"{"mapping": {}}"#).unwrap(), "chatgpt_json");
        assert_eq!(
            detect_format("[{\"type\": \"message\"}]").unwrap(),
            "slack_json"
        );
        assert!(detect_format("plain text").is_some());
    }

    // --- New format tests ---

    #[test]
    fn test_gemini_ai_studio_json_array_of_conversations() {
        let content = r#"[
            {
                "messages": [
                    {"role": "user", "content": "What is Rust?"},
                    {"role": "model", "content": "Rust is a systems programming language."}
                ]
            }
        ]"#;
        let result = normalize(std::path::Path::new("export.json"), content).unwrap();
        assert!(result.contains("> What is Rust?"));
        assert!(result.contains("Rust is a systems programming language."));
    }

    #[test]
    fn test_gemini_ai_studio_json_with_parts() {
        let content = r#"{"contents": [
            {"role": "user", "parts": [{"text": "Hello from AI Studio"}]},
            {"role": "model", "parts": [{"text": "Hi there!"}]}
        ]}"#;
        let result = normalize(std::path::Path::new("export.json"), content).unwrap();
        assert!(result.contains("> Hello from AI Studio"));
        assert!(result.contains("Hi there!"));
    }

    #[test]
    fn test_gemini_ai_studio_detect_format() {
        let content = r#"[{"messages": [{"role": "user", "content": "hi"}]}]"#;
        let result = detect_format(content);
        assert_eq!(result, Some("gemini_ai_studio_json".to_string()));
    }

    #[test]
    fn test_pi_jsonl_basic() {
        let content = r#"{"type":"user","text":"Hello Pi"}
{"type":"pi","text":"Hi! How can I help?"}"#;
        let result = normalize(std::path::Path::new("chat.jsonl"), content).unwrap();
        assert!(result.contains("> Hello Pi"));
        assert!(result.contains("Hi! How can I help?"));
    }

    #[test]
    fn test_pi_jsonl_with_kind_field() {
        let content = r#"{"kind":"user","message":"What's up?"}
{"kind":"pi","message":"Not much, you?"}"#;
        let result = normalize(std::path::Path::new("chat.jsonl"), content).unwrap();
        assert!(result.contains("> What's up?"));
        assert!(result.contains("Not much, you?"));
    }

    #[test]
    fn test_pi_jsonl_detect_format() {
        let content = r#"{"type":"user","text":"hi"}
{"type":"pi","text":"hello"}"#;
        let result = detect_format(content);
        assert_eq!(result, Some("pi_jsonl".to_string()));
    }

    #[test]
    fn test_continue_dev_json_history() {
        let content = r#"{
            "history": [
                {"role": "user", "content": "How do I deploy this"},
                {"role": "assistant", "content": "Run cargo build and then upload the binary."}
            ]
        }"#;
        let result = normalize(std::path::Path::new("chat.json"), content).unwrap();
        assert!(result.contains("> How do I deploy this"));
        assert!(result.contains("cargo build"));
    }

    #[test]
    fn test_continue_dev_json_messages() {
        let content = r#"{
            "messages": [
                {"role": "user", "parts": [{"text": "Debug this code"}]},
                {"role": "assistant", "parts": [{"text": "I see the issue."}]}
            ]
        }"#;
        let result = normalize(std::path::Path::new("chat.json"), content).unwrap();
        assert!(result.contains("> Debug this code"));
        assert!(result.contains("I see the issue."));
    }

    #[test]
    fn test_continue_dev_json_detect_format() {
        let content = r#"{"history": [
            {"role": "user", "content": "test"},
            {"role": "assistant", "content": "response"}
        ]}"#;
        let result = detect_format(content);
        assert_eq!(result, Some("continue_dev_json".to_string()));
    }

    #[test]
    fn test_claude_code_jsonl_harden_skips_malformed_lines() {
        let content = r#"not json at all
{"type":"human","message":{"content":"Hello"}}
{"broken json
{"type":"assistant","message":{"content":"Hi there"}}"#;
        let result = normalize(std::path::Path::new("test.jsonl"), content).unwrap();
        assert!(result.contains("> Hello"));
        assert!(result.contains("Hi there"));
    }

    #[test]
    fn test_claude_code_jsonl_harden_ignores_thinking_blocks() {
        let content = r#"{"type":"human","message":{"content":"Hello"}}
{"type":"assistant","message":{"content":[{"type":"thinking","thinking":"internal reasoning"},{"type":"text","text":"Hi there!"}]}}"#;
        let result = normalize(std::path::Path::new("test.jsonl"), content).unwrap();
        assert!(result.contains("> Hello"));
        assert!(result.contains("Hi there!"));
        assert!(!result.contains("internal reasoning"));
    }

    #[test]
    fn test_deterministic_obs_id_is_stable() {
        let id1 = deterministic_obs_id("test_format", 0, "hello world");
        let id2 = deterministic_obs_id("test_format", 0, "hello world");
        assert_eq!(id1, id2);
        assert!(id1.starts_with("obs-"));
        assert_eq!(id1.len(), 20); // "obs-" + 16 hex chars
    }

    #[test]
    fn test_deterministic_obs_id_varies_by_content() {
        let id1 = deterministic_obs_id("test", 0, "hello");
        let id2 = deterministic_obs_id("test", 0, "world");
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_deterministic_obs_id_varies_by_index() {
        let id1 = deterministic_obs_id("test", 0, "hello");
        let id2 = deterministic_obs_id("test", 1, "hello");
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_normalize_to_observations_claude_code() {
        let content = r#"{"type":"human","message":{"content":"Hello"}}
{"type":"assistant","message":{"content":"Hi there"}}"#;
        let obs =
            normalize_to_observations(std::path::Path::new("test.jsonl"), content, "test-session")
                .unwrap();
        assert_eq!(obs.len(), 2);
        assert_eq!(obs[0].user_prompt.as_deref(), Some("Hello"));
        assert_eq!(obs[1].assistant_response.as_deref(), Some("Hi there"));
        assert!(obs[0].id.starts_with("obs-"));
        assert!(obs[1].id.starts_with("obs-"));
        assert_ne!(obs[0].id, obs[1].id);
        assert_eq!(obs[0].session_id, "test-session");
    }

    #[test]
    fn test_normalize_to_observations_pi_jsonl() {
        let content = r#"{"type":"user","text":"Hello Pi"}
{"type":"pi","text":"Hi there!"}"#;
        let obs =
            normalize_to_observations(std::path::Path::new("chat.jsonl"), content, "pi-session")
                .unwrap();
        assert_eq!(obs.len(), 2);
        assert_eq!(obs[0].user_prompt.as_deref(), Some("Hello Pi"));
        assert_eq!(obs[1].assistant_response.as_deref(), Some("Hi there!"));
    }

    #[test]
    fn test_normalize_to_observations_empty() {
        let obs =
            normalize_to_observations(std::path::Path::new("empty.txt"), "", "session").unwrap();
        assert!(obs.is_empty());
    }

    #[test]
    fn test_normalize_to_observations_gemini_ai_studio() {
        let content = r#"{"contents": [
            {"role": "user", "parts": [{"text": "Hi"}]},
            {"role": "model", "parts": [{"text": "Hello!"}]}
        ]}"#;
        let obs = normalize_to_observations(
            std::path::Path::new("export.json"),
            content,
            "ai-studio-session",
        )
        .unwrap();
        assert_eq!(obs.len(), 2);
        assert_eq!(obs[0].user_prompt.as_deref(), Some("Hi"));
        assert_eq!(obs[1].assistant_response.as_deref(), Some("Hello!"));
    }

    #[test]
    fn test_parse_transcript_to_messages() {
        let transcript = "> Hello world\nHi there!\n\n> Another question\nAnother answer\n";
        let messages = parse_transcript_to_messages(transcript).unwrap();
        assert_eq!(messages.len(), 4);
        assert_eq!(messages[0], ("user".to_string(), "Hello world".to_string()));
        assert_eq!(
            messages[1],
            ("assistant".to_string(), "Hi there!".to_string())
        );
        assert_eq!(
            messages[2],
            ("user".to_string(), "Another question".to_string())
        );
        assert_eq!(
            messages[3],
            ("assistant".to_string(), "Another answer".to_string())
        );
    }
}
