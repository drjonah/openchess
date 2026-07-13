//! Named strength profiles for slot assignment (P11-07).
//!
//! A profile pairs a White and Black [`SideStrength`] under a name, so an
//! arena can lay out "strong vs weak" tournaments across many slots.

use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::config::SideStrength;

/// A named `{ white, black }` strength preset.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArenaProfile {
    pub name: String,
    #[serde(default)]
    pub white: SideStrength,
    #[serde(default)]
    pub black: SideStrength,
}

/// A JSON file holding a list of profiles.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ProfileSet {
    #[serde(default)]
    pub profiles: Vec<ArenaProfile>,
}

impl ProfileSet {
    /// Load a profile set from a JSON file.
    ///
    /// Accepts either `{ "profiles": [...] }` or a bare `[...]` array.
    pub fn load(path: impl AsRef<Path>) -> io::Result<Self> {
        let text = fs::read_to_string(path)?;
        if let Ok(set) = serde_json::from_str::<ProfileSet>(&text) {
            if !set.profiles.is_empty() {
                return Ok(set);
            }
        }
        let profiles: Vec<ArenaProfile> = serde_json::from_str(&text)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        Ok(Self { profiles })
    }

    pub fn is_empty(&self) -> bool {
        self.profiles.is_empty()
    }

    pub fn len(&self) -> usize {
        self.profiles.len()
    }
}
