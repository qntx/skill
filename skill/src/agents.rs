//! Agent registry and detection.
//!
//! Contains the built-in definitions for all known AI coding agents and
//! provides an extensible [`AgentRegistry`] that agent frameworks can
//! populate with custom entries.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::types::{AgentConfig, AgentId, UNIVERSAL_SKILLS_DIR};

/// Registry holding all known agent configurations.
///
/// Pre-populated with the built-in agents via [`AgentRegistry::with_defaults`].
/// Agent frameworks can register additional agents with [`AgentRegistry::register`].
#[derive(Debug)]
pub struct AgentRegistry {
    agents: HashMap<AgentId, AgentConfig>,
}

impl Default for AgentRegistry {
    fn default() -> Self {
        Self::with_defaults()
    }
}

impl AgentRegistry {
    /// Create a registry pre-populated with all built-in agent definitions.
    #[must_use]
    pub fn with_defaults() -> Self {
        let mut registry = Self {
            agents: HashMap::new(),
        };
        register_builtin_agents(&mut registry);
        registry
    }

    /// Create an empty registry with no agents.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            agents: HashMap::new(),
        }
    }

    /// Register a custom agent configuration.
    pub fn register(&mut self, config: AgentConfig) {
        self.agents.insert(config.name.clone(), config);
    }

    /// Look up an agent by its identifier.
    #[must_use]
    pub fn get(&self, id: &AgentId) -> Option<&AgentConfig> {
        self.agents.get(id)
    }

    /// Return all registered agent identifiers, sorted alphabetically.
    #[must_use]
    pub fn all_ids(&self) -> Vec<AgentId> {
        let mut ids: Vec<_> = self.agents.keys().cloned().collect();
        ids.sort();
        ids
    }

    /// Return all registered agent configurations.
    #[must_use]
    pub fn all_configs(&self) -> Vec<&AgentConfig> {
        self.agents.values().collect()
    }

    /// Detect which agents are installed by checking for their known paths.
    pub async fn detect_installed(&self) -> Vec<AgentId> {
        let mut installed = Vec::new();
        for (id, config) in &self.agents {
            for path in &config.detect_paths {
                if tokio::fs::try_exists(path).await.unwrap_or(false) {
                    installed.push(id.clone());
                    break;
                }
            }
        }
        installed.sort();
        installed
    }

    /// Return agent IDs that use the universal `.agents/skills` directory.
    #[must_use]
    pub fn universal_agents(&self) -> Vec<AgentId> {
        let mut ids: Vec<_> = self
            .agents
            .iter()
            .filter(|(_, c)| c.skills_dir == UNIVERSAL_SKILLS_DIR && c.show_in_universal_list)
            .map(|(id, _)| id.clone())
            .collect();
        ids.sort();
        ids
    }

    /// Return agent IDs that use agent-specific (non-universal) directories.
    #[must_use]
    pub fn non_universal_agents(&self) -> Vec<AgentId> {
        let mut ids: Vec<_> = self
            .agents
            .iter()
            .filter(|(_, c)| c.skills_dir != UNIVERSAL_SKILLS_DIR)
            .map(|(id, _)| id.clone())
            .collect();
        ids.sort();
        ids
    }

    /// Check whether an agent uses the universal `.agents/skills` directory.
    #[must_use]
    pub fn is_universal(&self, id: &AgentId) -> bool {
        self.agents
            .get(id)
            .is_some_and(|c| c.skills_dir == UNIVERSAL_SKILLS_DIR)
    }

    /// Return the number of registered agents.
    #[must_use]
    pub fn len(&self) -> usize {
        self.agents.len()
    }

    /// Check if the registry is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.agents.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Helper to build an agent config concisely
// ---------------------------------------------------------------------------

fn agent(
    name: &str,
    display_name: &str,
    skills_dir: &str,
    global_skills_dir: Option<PathBuf>,
    detect_paths: Vec<PathBuf>,
) -> AgentConfig {
    AgentConfig {
        name: AgentId::new(name),
        display_name: display_name.to_owned(),
        skills_dir: skills_dir.to_owned(),
        global_skills_dir,
        detect_paths,
        show_in_universal_list: true,
    }
}

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("~"))
}

/// XDG config home, matching the behavior of the `xdg-basedir` npm package:
/// use `$XDG_CONFIG_HOME` if set, otherwise fall back to `~/.config` on all
/// platforms.  This differs from `dirs::config_dir()` which returns
/// platform-specific paths (e.g. `~/Library/Application Support` on macOS,
/// `%APPDATA%` on Windows) that would break interop with the Vercel TS CLI.
fn xdg_config_home() -> PathBuf {
    std::env::var("XDG_CONFIG_HOME")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map_or_else(|| home().join(".config"), PathBuf::from)
}

/// Register all built-in agents matching the `TypeScript` reference.
fn register_builtin_agents(reg: &mut AgentRegistry) {
    let h = home();
    let cfg = xdg_config_home();
    let codex_home = std::env::var("CODEX_HOME")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map_or_else(|| h.join(".codex"), PathBuf::from);
    let claude_home = std::env::var("CLAUDE_CONFIG_DIR")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map_or_else(|| h.join(".claude"), PathBuf::from);

    reg.register(agent(
        "amp",
        "Amp",
        ".agents/skills",
        Some(cfg.join("agents/skills")),
        vec![cfg.join("amp")],
    ));
    reg.register(agent(
        "antigravity",
        "Antigravity",
        ".agent/skills",
        Some(h.join(".gemini/antigravity/skills")),
        vec![h.join(".gemini/antigravity")],
    ));
    reg.register(agent(
        "augment",
        "Augment",
        ".augment/skills",
        Some(h.join(".augment/skills")),
        vec![h.join(".augment")],
    ));
    reg.register(agent(
        "claude-code",
        "Claude Code",
        ".claude/skills",
        Some(claude_home.join("skills")),
        vec![claude_home.clone()],
    ));
    reg.register({
        let global = if h.join(".openclaw").exists() {
            h.join(".openclaw/skills")
        } else if h.join(".clawdbot").exists() {
            h.join(".clawdbot/skills")
        } else if h.join(".moltbot").exists() {
            h.join(".moltbot/skills")
        } else {
            h.join(".openclaw/skills")
        };
        agent(
            "openclaw",
            "OpenClaw",
            "skills",
            Some(global),
            vec![h.join(".openclaw"), h.join(".clawdbot"), h.join(".moltbot")],
        )
    });
    reg.register(agent(
        "cline",
        "Cline",
        ".agents/skills",
        Some(h.join(".agents/skills")),
        vec![h.join(".cline")],
    ));
    reg.register(agent(
        "codebuddy",
        "CodeBuddy",
        ".codebuddy/skills",
        Some(h.join(".codebuddy/skills")),
        vec![h.join(".codebuddy")],
    ));
    reg.register(agent(
        "codex",
        "Codex",
        ".agents/skills",
        Some(codex_home.join("skills")),
        vec![codex_home],
    ));
    reg.register(agent(
        "command-code",
        "Command Code",
        ".commandcode/skills",
        Some(h.join(".commandcode/skills")),
        vec![h.join(".commandcode")],
    ));
    reg.register(agent(
        "continue",
        "Continue",
        ".continue/skills",
        Some(h.join(".continue/skills")),
        vec![h.join(".continue")],
    ));
    reg.register(agent(
        "cortex",
        "Cortex Code",
        ".cortex/skills",
        Some(h.join(".snowflake/cortex/skills")),
        vec![h.join(".snowflake/cortex")],
    ));
    reg.register(agent(
        "crush",
        "Crush",
        ".crush/skills",
        Some(h.join(".config/crush/skills")),
        vec![h.join(".config/crush")],
    ));
    reg.register(agent(
        "cursor",
        "Cursor",
        ".agents/skills",
        Some(h.join(".cursor/skills")),
        vec![h.join(".cursor")],
    ));
    reg.register(agent(
        "droid",
        "Droid",
        ".factory/skills",
        Some(h.join(".factory/skills")),
        vec![h.join(".factory")],
    ));
    reg.register(agent(
        "gemini-cli",
        "Gemini CLI",
        ".agents/skills",
        Some(h.join(".gemini/skills")),
        vec![h.join(".gemini")],
    ));
    reg.register(agent(
        "github-copilot",
        "GitHub Copilot",
        ".agents/skills",
        Some(h.join(".copilot/skills")),
        vec![h.join(".copilot")],
    ));
    reg.register(agent(
        "goose",
        "Goose",
        ".goose/skills",
        Some(cfg.join("goose/skills")),
        vec![cfg.join("goose")],
    ));
    reg.register(agent(
        "junie",
        "Junie",
        ".junie/skills",
        Some(h.join(".junie/skills")),
        vec![h.join(".junie")],
    ));
    reg.register(agent(
        "iflow-cli",
        "iFlow CLI",
        ".iflow/skills",
        Some(h.join(".iflow/skills")),
        vec![h.join(".iflow")],
    ));
    reg.register(agent(
        "kilo",
        "Kilo Code",
        ".kilocode/skills",
        Some(h.join(".kilocode/skills")),
        vec![h.join(".kilocode")],
    ));
    reg.register(agent(
        "kimi-cli",
        "Kimi Code CLI",
        ".agents/skills",
        Some(cfg.join("agents/skills")),
        vec![h.join(".kimi")],
    ));
    reg.register(agent(
        "kiro-cli",
        "Kiro CLI",
        ".kiro/skills",
        Some(h.join(".kiro/skills")),
        vec![h.join(".kiro")],
    ));
    reg.register(agent(
        "kode",
        "Kode",
        ".kode/skills",
        Some(h.join(".kode/skills")),
        vec![h.join(".kode")],
    ));
    reg.register(agent(
        "mcpjam",
        "MCPJam",
        ".mcpjam/skills",
        Some(h.join(".mcpjam/skills")),
        vec![h.join(".mcpjam")],
    ));
    reg.register(agent(
        "mistral-vibe",
        "Mistral Vibe",
        ".vibe/skills",
        Some(h.join(".vibe/skills")),
        vec![h.join(".vibe")],
    ));
    reg.register(agent(
        "mux",
        "Mux",
        ".mux/skills",
        Some(h.join(".mux/skills")),
        vec![h.join(".mux")],
    ));
    reg.register(agent(
        "opencode",
        "OpenCode",
        ".agents/skills",
        Some(cfg.join("opencode/skills")),
        vec![cfg.join("opencode")],
    ));
    reg.register(agent(
        "openhands",
        "OpenHands",
        ".openhands/skills",
        Some(h.join(".openhands/skills")),
        vec![h.join(".openhands")],
    ));
    reg.register(agent(
        "pi",
        "Pi",
        ".pi/skills",
        Some(h.join(".pi/agent/skills")),
        vec![h.join(".pi/agent")],
    ));
    reg.register(agent(
        "qoder",
        "Qoder",
        ".qoder/skills",
        Some(h.join(".qoder/skills")),
        vec![h.join(".qoder")],
    ));
    reg.register(agent(
        "qwen-code",
        "Qwen Code",
        ".qwen/skills",
        Some(h.join(".qwen/skills")),
        vec![h.join(".qwen")],
    ));
    reg.register({
        let mut c = agent(
            "replit",
            "Replit",
            ".agents/skills",
            Some(cfg.join("agents/skills")),
            vec![], // detected differently
        );
        c.show_in_universal_list = false;
        c
    });
    reg.register(agent(
        "roo",
        "Roo Code",
        ".roo/skills",
        Some(h.join(".roo/skills")),
        vec![h.join(".roo")],
    ));
    reg.register(agent(
        "trae",
        "Trae",
        ".trae/skills",
        Some(h.join(".trae/skills")),
        vec![h.join(".trae")],
    ));
    reg.register(agent(
        "trae-cn",
        "Trae CN",
        ".trae/skills",
        Some(h.join(".trae-cn/skills")),
        vec![h.join(".trae-cn")],
    ));
    reg.register(agent(
        "windsurf",
        "Windsurf",
        ".windsurf/skills",
        Some(h.join(".codeium/windsurf/skills")),
        vec![h.join(".codeium/windsurf")],
    ));
    reg.register(agent(
        "zencoder",
        "Zencoder",
        ".zencoder/skills",
        Some(h.join(".zencoder/skills")),
        vec![h.join(".zencoder")],
    ));
    reg.register(agent(
        "neovate",
        "Neovate",
        ".neovate/skills",
        Some(h.join(".neovate/skills")),
        vec![h.join(".neovate")],
    ));
    reg.register(agent(
        "pochi",
        "Pochi",
        ".pochi/skills",
        Some(h.join(".pochi/skills")),
        vec![h.join(".pochi")],
    ));
    reg.register(agent(
        "adal",
        "AdaL",
        ".adal/skills",
        Some(h.join(".adal/skills")),
        vec![h.join(".adal")],
    ));
    reg.register({
        let mut c = agent(
            "universal",
            "Universal",
            ".agents/skills",
            Some(cfg.join("agents/skills")),
            vec![],
        );
        c.show_in_universal_list = false;
        c
    });
}
