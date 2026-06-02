//! Continue.dev adapter.
//!
//! Continue.dev v1+ prefers `~/.continue/config.yaml`; `config.json`
//! is deprecated and ignored when yaml is present.  Three branches:
//!   - config.yaml exists  → stub (manual edit required)
//!   - config.json exists  → modify it (legacy path)
//!   - neither exists       → create config.json (no YAML dep in tree)

use std::fs;
use std::path::PathBuf;

use crate::connect::json_mcp::write_mcp_config;
use crate::connect::types::{ConnectOptions, ConnectResult};
use crate::connect::ConnectAdapter;

const CONTINUE_DIR: &str = ".continue";
const YAML_NAME: &str = "config.yaml";
const JSON_NAME: &str = "config.json";

pub struct ContinueDevAdapter;

impl ConnectAdapter for ContinueDevAdapter {
    fn name(&self) -> &'static str {
        "continue"
    }

    fn config_path(&self) -> PathBuf {
        dirs::home_dir()
            .map(|p| p.join(".continue/config.json"))
            .unwrap_or_else(|| PathBuf::from("~/.continue/config.json"))
    }

    fn detect(&self) -> bool {
        dirs::home_dir()
            .map(|p| p.join(CONTINUE_DIR).exists())
            .unwrap_or(false)
    }

    fn connect(&self, opts: &ConnectOptions) -> std::result::Result<ConnectResult, anyhow::Error> {
        let home = dirs::home_dir().unwrap_or_default();
        let yaml_path = home.join(CONTINUE_DIR).join(YAML_NAME);
        let json_path = home.join(CONTINUE_DIR).join(JSON_NAME);

        // Branch 1: yaml exists → stub (no YAML dep in tree)
        if yaml_path.exists() {
            tracing::info!("connect: continue config.yaml exists, stub (manual edit required)");
            return Ok(ConnectResult {
                adapter: self.name().to_string(),
                config_path: yaml_path,
                wrote: false,
                note: Some("config.yaml exists — manual edit required (YAML)".to_string()),
            });
        }

        // Branch 2: json exists → modify in place
        if json_path.exists() {
            let result = write_mcp_config(&json_path, "mempalace", "mcpServers");
            if opts.dry_run {
                tracing::info!(
                    "connect [dry-run] {} → {:?} (wrote={})",
                    self.name(),
                    json_path,
                    result.wrote
                );
            }
            return Ok(result);
        }

        // Branch 3: neither exists → create config.json
        if opts.dry_run {
            tracing::info!(
                "connect [dry-run] {} would create {:?}",
                self.name(),
                json_path
            );
            return Ok(ConnectResult {
                adapter: self.name().to_string(),
                config_path: json_path.clone(),
                wrote: false,
                note: Some("dry-run: would create config.json".to_string()),
            });
        }

        // Ensure parent dir
        if let Some(parent) = json_path.parent() {
            fs::create_dir_all(parent).ok();
        }

        // Write fresh config.json with mcpServers entry
        let result = write_mcp_config(&json_path, "mempalace", "mcpServers");
        Ok(result)
    }
}
