//! Experimental arm definitions (`B0`, `B1-notes`, `B1-grep`, `Hivemind`, `Oracle`).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryMode {
    /// Pre-populate memory from scenario's structured seed timeline; skip `History` episodes
    /// unless a scenario has no `memories` and explicitly tests an earned chain.
    Given,
    /// Run `History` episodes live so the agent writes its own memories before `Target`.
    Earned,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ArmKind {
    /// B0: clean sandbox, zero memory files, zero MCP servers.
    NoMemory,
    /// B1-notes: static always-loaded `AGENTS.md` markdown notes file in workspace root,
    /// containing append-only project notes + background noise notes.
    StaticNotes {
        #[serde(default = "default_notes_filename")]
        file_name: String,
    },
    /// B1-grep: `.memory/` directory of markdown notes that the agent must search/read on demand.
    MarkdownTree {
        #[serde(default = "default_tree_dirname")]
        dir_name: String,
    },
    /// Hivemind MCP server backed by an isolated store per `(scenario, seed)`.
    HivemindMcp {
        hivemind_bin: PathBuf,
        mode: MemoryMode,
        /// Pass scenario `context` (`key=value`) in recall instructions so `when` filters apply.
        #[serde(default = "default_true")]
        use_applicability: bool,
    },
    /// Upper-bound reference: injects only verified gold statements directly into the prompt.
    OracleNotes,
}

fn default_notes_filename() -> String {
    "AGENTS.md".into()
}

fn default_tree_dirname() -> String {
    ".memory".into()
}

fn default_true() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArmSpec {
    pub id: String,
    pub label: String,
    pub kind: ArmKind,
}

impl ArmSpec {
    pub fn b0() -> Self {
        Self {
            id: "b0".into(),
            label: "B0 (No Memory)".into(),
            kind: ArmKind::NoMemory,
        }
    }

    pub fn b1_notes() -> Self {
        Self {
            id: "b1-notes".into(),
            label: "B1 (Static AGENTS.md Notes)".into(),
            kind: ArmKind::StaticNotes {
                file_name: "AGENTS.md".into(),
            },
        }
    }

    pub fn b1_grep() -> Self {
        Self {
            id: "b1-grep".into(),
            label: "B1-grep (Markdown Directory + Grep)".into(),
            kind: ArmKind::MarkdownTree {
                dir_name: ".memory".into(),
            },
        }
    }

    pub fn hivemind(hivemind_bin: PathBuf, mode: MemoryMode) -> Self {
        let suffix = match mode {
            MemoryMode::Given => "hivemind",
            MemoryMode::Earned => "hivemind-earned",
        };
        let label = match mode {
            MemoryMode::Given => "Hivemind MCP",
            MemoryMode::Earned => "Hivemind MCP (Earned)",
        };
        Self {
            id: suffix.into(),
            label: label.into(),
            kind: ArmKind::HivemindMcp {
                hivemind_bin,
                mode,
                use_applicability: true,
            },
        }
    }

    pub fn oracle() -> Self {
        Self {
            id: "oracle".into(),
            label: "Oracle (Gold Injected)".into(),
            kind: ArmKind::OracleNotes,
        }
    }
}
