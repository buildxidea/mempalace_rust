//! OpenHuman adapter (stub).
//!
//! OpenHuman integration is not yet automated.  The REST proxy at
//! http://localhost:3111 plus an OpenHuman-side Memory trait impl is
//! expected, but no `integrations/openhuman/` folder exists yet.

use std::path::PathBuf;

use crate::connect::types::{ConnectOptions, ConnectResult};
use crate::connect::ConnectAdapter;

pub struct OpenHumanAdapter;

impl ConnectAdapter for OpenHumanAdapter {
    fn name(&self) -> &'static str {
        "openhuman"
    }

    fn config_path(&self) -> PathBuf {
        dirs::home_dir()
            .map(|p| p.join(".openhuman/mcp.json"))
            .unwrap_or_else(|| PathBuf::from("~/.openhuman/mcp.json"))
    }

    fn detect(&self) -> bool {
        dirs::home_dir()
            .map(|p| p.join(".openhuman").exists())
            .unwrap_or(false)
    }

    fn connect(&self, _opts: &ConnectOptions) -> std::result::Result<ConnectResult, anyhow::Error> {
        tracing::warn!(
            "connect: openhuman integration is not yet automated (stub)"
        );
        Ok(ConnectResult {
            adapter: self.name().to_string(),
            config_path: self.config_path(),
            wrote: false,
            note: Some(
                "no-integration-folder-yet: manual install required".to_string(),
            ),
        })
    }
}