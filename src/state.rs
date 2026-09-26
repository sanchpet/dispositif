//! Per-machine state outside the config: what has been seen, and which claude
//! session belongs to which chat. Plain JSON, written atomically.

use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::config::expand_tilde;

/// Agent message ids remembered per chat, for recognising replies to the agent.
pub const AGENT_IDS_KEPT: usize = 300;

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct State {
    /// Last processed message id per peer.
    #[serde(default)]
    pub last: BTreeMap<String, i64>,
    /// Known ids of the agent's own messages per peer.
    #[serde(default)]
    pub agent_ids: BTreeMap<String, Vec<i64>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    /// Unix seconds of the run that produced it.
    pub at: f64,
}

/// Claude session per "peer:trust".
pub type Sessions = BTreeMap<String, Session>;

pub struct StateDir(PathBuf);

impl StateDir {
    /// `$DISPOSITIF_STATE_DIR`, else `~/.local/state/dispositif`.
    pub fn from_env() -> StateDir {
        let dir = std::env::var("DISPOSITIF_STATE_DIR")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "~/.local/state/dispositif".into());
        StateDir(expand_tilde(&dir))
    }

    pub fn new(dir: impl Into<PathBuf>) -> StateDir {
        StateDir(dir.into())
    }

    pub fn load_state(&self) -> Result<State> {
        load(&self.0.join("state.json"))
    }

    pub fn save_state(&self, s: &State) -> Result<()> {
        save(&self.0.join("state.json"), s)
    }

    pub fn load_sessions(&self) -> Result<Sessions> {
        load(&self.0.join("sessions.json"))
    }

    pub fn save_sessions(&self, s: &Sessions) -> Result<()> {
        save(&self.0.join("sessions.json"), s)
    }
}

/// A missing or unreadable-as-JSON file starts empty: losing state costs a re-scan,
/// not correctness, since first sight of a chat never replays old history.
fn load<T: DeserializeOwned + Default>(path: &Path) -> Result<T> {
    match std::fs::read_to_string(path) {
        Ok(raw) => Ok(serde_json::from_str(&raw).unwrap_or_else(|e| {
            crate::log(&format!("ignoring corrupt {}: {e}", path.display()));
            T::default()
        })),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(T::default()),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

fn save<T: Serialize>(path: &Path, v: &T) -> Result<()> {
    let dir = path.parent().expect("state files live in a directory");
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec(v)?)
        .with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))
}
