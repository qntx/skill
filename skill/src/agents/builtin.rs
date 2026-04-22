//! Built-in agent definitions.
//!
//! This module is a *data* module: it describes where each known agent
//! keeps its skills, how to detect it, and any per-agent quirks. All
//! behaviour (lookups, detection, classification) lives in the parent
//! module's [`super::AgentRegistry`].
//!
//! New agents are added by appending an entry to [`builtin_agents`].
//! The helper [`AgentSpec`] keeps each row compact and readable; subpath
//! expressions are evaluated lazily against the current process
//! environment so adding an agent never requires threading environment
//! state through multiple layers.

use std::path::PathBuf;

use crate::types::{AgentConfig, AgentId};

/// Declarative description of one built-in agent.
///
/// Paths are expressed as closures so they capture the current user's
/// environment (`HOME`, `XDG_CONFIG_HOME`, `CWD`, `CODEX_HOME`,
/// `CLAUDE_CONFIG_DIR`) at registration time rather than at module-load
/// time (which would give wrong results if the user changed these
/// variables after importing the crate).
struct AgentSpec {
    /// Machine-readable identifier (e.g. `"cursor"`).
    id: &'static str,
    /// Human-readable display name (e.g. `"Cursor"`).
    display_name: &'static str,
    /// Project-relative skills directory (e.g. `".agents/skills"`).
    skills_dir: &'static str,
    /// Lazily resolved global install directory.
    global_skills_dir: fn(&Env) -> Option<PathBuf>,
    /// Lazily resolved paths probed for agent detection.
    detect_paths: fn(&Env) -> Vec<PathBuf>,
    /// Whether this agent appears in the universal list.
    show_in_universal_list: bool,
}

impl AgentSpec {
    /// Materialise an [`AgentSpec`] against a concrete environment.
    fn resolve(&self, env: &Env) -> AgentConfig {
        AgentConfig {
            name: AgentId::new(self.id),
            display_name: self.display_name.to_owned(),
            skills_dir: self.skills_dir.to_owned(),
            global_skills_dir: (self.global_skills_dir)(env),
            detect_paths: (self.detect_paths)(env),
            show_in_universal_list: self.show_in_universal_list,
        }
    }
}

/// Captured per-process environment used to materialise agent paths.
struct Env {
    /// User's home directory.
    home: PathBuf,
    /// XDG config home (`$XDG_CONFIG_HOME` or `~/.config`).
    config: PathBuf,
    /// Current working directory.
    cwd: PathBuf,
    /// Codex home (`$CODEX_HOME` or `~/.codex`).
    codex: PathBuf,
    /// Claude config dir (`$CLAUDE_CONFIG_DIR` or `~/.claude`).
    claude: PathBuf,
}

impl Env {
    /// Capture the current environment once per registry build.
    fn capture() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("~"));
        let config = env_override("XDG_CONFIG_HOME").unwrap_or_else(|| home.join(".config"));
        let cwd = std::env::current_dir().unwrap_or_default();
        let codex = env_override("CODEX_HOME").unwrap_or_else(|| home.join(".codex"));
        let claude = env_override("CLAUDE_CONFIG_DIR").unwrap_or_else(|| home.join(".claude"));
        Self {
            home,
            config,
            cwd,
            codex,
            claude,
        }
    }
}

/// Read an environment variable as a `PathBuf`, treating empty values as
/// unset (matching `xdg-basedir` behaviour).
fn env_override(key: &str) -> Option<PathBuf> {
    std::env::var(key)
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
}

/// Resolve `OpenClaw`'s global skills dir with the three-way fallback chain
/// used by the TS reference. Kept inline because it's the only agent with
/// conditional global-dir logic.
fn openclaw_global_dir(env: &Env) -> PathBuf {
    let candidates = [".openclaw", ".clawdbot", ".moltbot"];
    for marker in candidates {
        if env.home.join(marker).exists() {
            return env.home.join(marker).join("skills");
        }
    }
    env.home.join(".openclaw").join("skills")
}

/// Return every built-in agent config, resolved against the current
/// environment.
pub(super) fn builtin_agents() -> Vec<AgentConfig> {
    let env = Env::capture();
    SPECS.iter().map(|spec| spec.resolve(&env)).collect()
}

/// The authoritative list of built-in agents, kept in sync with
/// `3rdparty/skills/src/agents.ts`. Kept alphabetically sorted by `id`; the
/// sort invariant is enforced by `test_spec_ids_are_sorted`.
const SPECS: &[AgentSpec] = &[
    AgentSpec {
        id: "adal",
        display_name: "AdaL",
        skills_dir: ".adal/skills",
        global_skills_dir: |e| Some(e.home.join(".adal/skills")),
        detect_paths: |e| vec![e.home.join(".adal")],
        show_in_universal_list: true,
    },
    AgentSpec {
        id: "amp",
        display_name: "Amp",
        skills_dir: ".agents/skills",
        global_skills_dir: |e| Some(e.config.join("agents/skills")),
        detect_paths: |e| vec![e.config.join("amp")],
        show_in_universal_list: true,
    },
    AgentSpec {
        id: "antigravity",
        display_name: "Antigravity",
        skills_dir: ".agents/skills",
        global_skills_dir: |e| Some(e.home.join(".gemini/antigravity/skills")),
        detect_paths: |e| vec![e.home.join(".gemini/antigravity")],
        show_in_universal_list: true,
    },
    AgentSpec {
        id: "augment",
        display_name: "Augment",
        skills_dir: ".augment/skills",
        global_skills_dir: |e| Some(e.home.join(".augment/skills")),
        detect_paths: |e| vec![e.home.join(".augment")],
        show_in_universal_list: true,
    },
    AgentSpec {
        id: "bob",
        display_name: "IBM Bob",
        skills_dir: ".bob/skills",
        global_skills_dir: |e| Some(e.home.join(".bob/skills")),
        detect_paths: |e| vec![e.home.join(".bob")],
        show_in_universal_list: true,
    },
    AgentSpec {
        id: "claude-code",
        display_name: "Claude Code",
        skills_dir: ".claude/skills",
        global_skills_dir: |e| Some(e.claude.join("skills")),
        detect_paths: |e| vec![e.claude.clone()],
        show_in_universal_list: true,
    },
    AgentSpec {
        id: "cline",
        display_name: "Cline",
        skills_dir: ".agents/skills",
        global_skills_dir: |e| Some(e.home.join(".agents/skills")),
        detect_paths: |e| vec![e.home.join(".cline")],
        show_in_universal_list: true,
    },
    AgentSpec {
        id: "codebuddy",
        display_name: "CodeBuddy",
        skills_dir: ".codebuddy/skills",
        global_skills_dir: |e| Some(e.home.join(".codebuddy/skills")),
        detect_paths: |e| vec![e.cwd.join(".codebuddy"), e.home.join(".codebuddy")],
        show_in_universal_list: true,
    },
    AgentSpec {
        id: "codex",
        display_name: "Codex",
        skills_dir: ".agents/skills",
        global_skills_dir: |e| Some(e.codex.join("skills")),
        detect_paths: |e| vec![e.codex.clone(), PathBuf::from("/etc/codex")],
        show_in_universal_list: true,
    },
    AgentSpec {
        id: "command-code",
        display_name: "Command Code",
        skills_dir: ".commandcode/skills",
        global_skills_dir: |e| Some(e.home.join(".commandcode/skills")),
        detect_paths: |e| vec![e.home.join(".commandcode")],
        show_in_universal_list: true,
    },
    AgentSpec {
        id: "continue",
        display_name: "Continue",
        skills_dir: ".continue/skills",
        global_skills_dir: |e| Some(e.home.join(".continue/skills")),
        detect_paths: |e| vec![e.cwd.join(".continue"), e.home.join(".continue")],
        show_in_universal_list: true,
    },
    AgentSpec {
        id: "cortex",
        display_name: "Cortex Code",
        skills_dir: ".cortex/skills",
        global_skills_dir: |e| Some(e.home.join(".snowflake/cortex/skills")),
        detect_paths: |e| vec![e.home.join(".snowflake/cortex")],
        show_in_universal_list: true,
    },
    AgentSpec {
        id: "crush",
        display_name: "Crush",
        skills_dir: ".crush/skills",
        global_skills_dir: |e| Some(e.home.join(".config/crush/skills")),
        detect_paths: |e| vec![e.home.join(".config/crush")],
        show_in_universal_list: true,
    },
    AgentSpec {
        id: "cursor",
        display_name: "Cursor",
        skills_dir: ".agents/skills",
        global_skills_dir: |e| Some(e.home.join(".cursor/skills")),
        detect_paths: |e| vec![e.home.join(".cursor")],
        show_in_universal_list: true,
    },
    AgentSpec {
        id: "deepagents",
        display_name: "Deep Agents",
        skills_dir: ".agents/skills",
        global_skills_dir: |e| Some(e.home.join(".deepagents/agent/skills")),
        detect_paths: |e| vec![e.home.join(".deepagents")],
        show_in_universal_list: true,
    },
    AgentSpec {
        id: "droid",
        display_name: "Droid",
        skills_dir: ".factory/skills",
        global_skills_dir: |e| Some(e.home.join(".factory/skills")),
        detect_paths: |e| vec![e.home.join(".factory")],
        show_in_universal_list: true,
    },
    AgentSpec {
        id: "firebender",
        display_name: "Firebender",
        skills_dir: ".agents/skills",
        global_skills_dir: |e| Some(e.home.join(".firebender/skills")),
        detect_paths: |e| vec![e.home.join(".firebender")],
        show_in_universal_list: true,
    },
    AgentSpec {
        id: "gemini-cli",
        display_name: "Gemini CLI",
        skills_dir: ".agents/skills",
        global_skills_dir: |e| Some(e.home.join(".gemini/skills")),
        detect_paths: |e| vec![e.home.join(".gemini")],
        show_in_universal_list: true,
    },
    AgentSpec {
        id: "github-copilot",
        display_name: "GitHub Copilot",
        skills_dir: ".agents/skills",
        global_skills_dir: |e| Some(e.home.join(".copilot/skills")),
        detect_paths: |e| vec![e.home.join(".copilot")],
        show_in_universal_list: true,
    },
    AgentSpec {
        id: "goose",
        display_name: "Goose",
        skills_dir: ".goose/skills",
        global_skills_dir: |e| Some(e.config.join("goose/skills")),
        detect_paths: |e| vec![e.config.join("goose")],
        show_in_universal_list: true,
    },
    AgentSpec {
        id: "iflow-cli",
        display_name: "iFlow CLI",
        skills_dir: ".iflow/skills",
        global_skills_dir: |e| Some(e.home.join(".iflow/skills")),
        detect_paths: |e| vec![e.home.join(".iflow")],
        show_in_universal_list: true,
    },
    AgentSpec {
        id: "junie",
        display_name: "Junie",
        skills_dir: ".junie/skills",
        global_skills_dir: |e| Some(e.home.join(".junie/skills")),
        detect_paths: |e| vec![e.home.join(".junie")],
        show_in_universal_list: true,
    },
    AgentSpec {
        id: "kilo",
        display_name: "Kilo Code",
        skills_dir: ".kilocode/skills",
        global_skills_dir: |e| Some(e.home.join(".kilocode/skills")),
        detect_paths: |e| vec![e.home.join(".kilocode")],
        show_in_universal_list: true,
    },
    AgentSpec {
        id: "kimi-cli",
        display_name: "Kimi Code CLI",
        skills_dir: ".agents/skills",
        global_skills_dir: |e| Some(e.config.join("agents/skills")),
        detect_paths: |e| vec![e.home.join(".kimi")],
        show_in_universal_list: true,
    },
    AgentSpec {
        id: "kiro-cli",
        display_name: "Kiro CLI",
        skills_dir: ".kiro/skills",
        global_skills_dir: |e| Some(e.home.join(".kiro/skills")),
        detect_paths: |e| vec![e.home.join(".kiro")],
        show_in_universal_list: true,
    },
    AgentSpec {
        id: "kode",
        display_name: "Kode",
        skills_dir: ".kode/skills",
        global_skills_dir: |e| Some(e.home.join(".kode/skills")),
        detect_paths: |e| vec![e.home.join(".kode")],
        show_in_universal_list: true,
    },
    AgentSpec {
        id: "mcpjam",
        display_name: "MCPJam",
        skills_dir: ".mcpjam/skills",
        global_skills_dir: |e| Some(e.home.join(".mcpjam/skills")),
        detect_paths: |e| vec![e.home.join(".mcpjam")],
        show_in_universal_list: true,
    },
    AgentSpec {
        id: "mistral-vibe",
        display_name: "Mistral Vibe",
        skills_dir: ".vibe/skills",
        global_skills_dir: |e| Some(e.home.join(".vibe/skills")),
        detect_paths: |e| vec![e.home.join(".vibe")],
        show_in_universal_list: true,
    },
    AgentSpec {
        id: "mux",
        display_name: "Mux",
        skills_dir: ".mux/skills",
        global_skills_dir: |e| Some(e.home.join(".mux/skills")),
        detect_paths: |e| vec![e.home.join(".mux")],
        show_in_universal_list: true,
    },
    AgentSpec {
        id: "neovate",
        display_name: "Neovate",
        skills_dir: ".neovate/skills",
        global_skills_dir: |e| Some(e.home.join(".neovate/skills")),
        detect_paths: |e| vec![e.home.join(".neovate")],
        show_in_universal_list: true,
    },
    AgentSpec {
        id: "openclaw",
        display_name: "OpenClaw",
        skills_dir: "skills",
        global_skills_dir: |e| Some(openclaw_global_dir(e)),
        detect_paths: |e| {
            vec![
                e.home.join(".openclaw"),
                e.home.join(".clawdbot"),
                e.home.join(".moltbot"),
            ]
        },
        show_in_universal_list: true,
    },
    AgentSpec {
        id: "opencode",
        display_name: "OpenCode",
        skills_dir: ".agents/skills",
        global_skills_dir: |e| Some(e.config.join("opencode/skills")),
        detect_paths: |e| vec![e.config.join("opencode")],
        show_in_universal_list: true,
    },
    AgentSpec {
        id: "openhands",
        display_name: "OpenHands",
        skills_dir: ".openhands/skills",
        global_skills_dir: |e| Some(e.home.join(".openhands/skills")),
        detect_paths: |e| vec![e.home.join(".openhands")],
        show_in_universal_list: true,
    },
    AgentSpec {
        id: "pi",
        display_name: "Pi",
        skills_dir: ".pi/skills",
        global_skills_dir: |e| Some(e.home.join(".pi/agent/skills")),
        detect_paths: |e| vec![e.home.join(".pi/agent")],
        show_in_universal_list: true,
    },
    AgentSpec {
        id: "pochi",
        display_name: "Pochi",
        skills_dir: ".pochi/skills",
        global_skills_dir: |e| Some(e.home.join(".pochi/skills")),
        detect_paths: |e| vec![e.home.join(".pochi")],
        show_in_universal_list: true,
    },
    AgentSpec {
        id: "qoder",
        display_name: "Qoder",
        skills_dir: ".qoder/skills",
        global_skills_dir: |e| Some(e.home.join(".qoder/skills")),
        detect_paths: |e| vec![e.home.join(".qoder")],
        show_in_universal_list: true,
    },
    AgentSpec {
        id: "qwen-code",
        display_name: "Qwen Code",
        skills_dir: ".qwen/skills",
        global_skills_dir: |e| Some(e.home.join(".qwen/skills")),
        detect_paths: |e| vec![e.home.join(".qwen")],
        show_in_universal_list: true,
    },
    AgentSpec {
        id: "replit",
        display_name: "Replit",
        skills_dir: ".agents/skills",
        global_skills_dir: |e| Some(e.config.join("agents/skills")),
        detect_paths: |e| vec![e.cwd.join(".replit")],
        show_in_universal_list: false,
    },
    AgentSpec {
        id: "roo",
        display_name: "Roo Code",
        skills_dir: ".roo/skills",
        global_skills_dir: |e| Some(e.home.join(".roo/skills")),
        detect_paths: |e| vec![e.home.join(".roo")],
        show_in_universal_list: true,
    },
    AgentSpec {
        id: "trae",
        display_name: "Trae",
        skills_dir: ".trae/skills",
        global_skills_dir: |e| Some(e.home.join(".trae/skills")),
        detect_paths: |e| vec![e.home.join(".trae")],
        show_in_universal_list: true,
    },
    AgentSpec {
        id: "trae-cn",
        display_name: "Trae CN",
        skills_dir: ".trae/skills",
        global_skills_dir: |e| Some(e.home.join(".trae-cn/skills")),
        detect_paths: |e| vec![e.home.join(".trae-cn")],
        show_in_universal_list: true,
    },
    AgentSpec {
        id: "universal",
        display_name: "Universal",
        skills_dir: ".agents/skills",
        global_skills_dir: |e| Some(e.config.join("agents/skills")),
        detect_paths: |_| vec![],
        show_in_universal_list: false,
    },
    AgentSpec {
        id: "warp",
        display_name: "Warp",
        skills_dir: ".agents/skills",
        global_skills_dir: |e| Some(e.home.join(".agents/skills")),
        detect_paths: |e| vec![e.home.join(".warp")],
        show_in_universal_list: true,
    },
    AgentSpec {
        id: "windsurf",
        display_name: "Windsurf",
        skills_dir: ".windsurf/skills",
        global_skills_dir: |e| Some(e.home.join(".codeium/windsurf/skills")),
        detect_paths: |e| vec![e.home.join(".codeium/windsurf")],
        show_in_universal_list: true,
    },
    AgentSpec {
        id: "zencoder",
        display_name: "Zencoder",
        skills_dir: ".zencoder/skills",
        global_skills_dir: |e| Some(e.home.join(".zencoder/skills")),
        detect_paths: |e| vec![e.home.join(".zencoder")],
        show_in_universal_list: true,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spec_ids_are_sorted() {
        for pair in SPECS.windows(2) {
            let [a, b] = pair else { unreachable!() };
            assert!(
                a.id < b.id,
                "SPECS must stay alphabetically sorted: {} !< {}",
                a.id,
                b.id
            );
        }
    }

    #[test]
    fn test_spec_ids_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for spec in SPECS {
            assert!(seen.insert(spec.id), "duplicate spec id: {}", spec.id);
        }
    }

    #[test]
    fn test_spec_count_matches_ts_reference() {
        // 45 built-in agents including `universal` (TS reference at
        // `3rdparty/skills/src/agents.ts`). Kept synced by manual audit:
        // bumping this count is a conscious TS-parity change.
        assert_eq!(SPECS.len(), 45);
    }

    #[test]
    fn test_hidden_from_universal_list() {
        let hidden: Vec<&str> = SPECS
            .iter()
            .filter(|s| !s.show_in_universal_list)
            .map(|s| s.id)
            .collect();
        assert_eq!(hidden, vec!["replit", "universal"]);
    }
}
