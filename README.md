<!-- markdownlint-disable MD033 MD041 MD036 -->

<div align="center">

# Skill

**The open agent skills ecosystem — supercharged with Rust.**

[![Crates.io](https://img.shields.io/crates/v/skill.svg)](https://crates.io/crates/skill)
[![docs.rs](https://img.shields.io/docsrs/skill)](https://docs.rs/skill)
[![CI](https://img.shields.io/github/actions/workflow/status/qntx/skill/rust.yml?label=CI)](https://github.com/qntx/skill/actions)
[![License: MIT/Apache-2.0](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE)

A high-performance Rust implementation of the [Vercel `skills` CLI](https://github.com/vercel-labs/skills).
Single static binary. Zero runtime dependencies. **Rust-exclusive features** that TypeScript can't match.

<!-- agent-list:start -->
Supports **Cursor**, **Claude Code**, **Windsurf**, **Codex**, and [35 more](#supported-agents).
<!-- agent-list:end -->

</div>

---

## Why Rust?

| Feature | TypeScript CLI | Rust CLI |
| ------- | -------------- | -------- |
| **Startup time** | ~300ms (Node.js cold start) | **<10ms** |
| **Binary size** | ~150MB (node_modules) | **~8MB** single static binary |
| **Shell completions** | ❌ Not possible | ✅ `skills completions <shell>` |
| **Self-update** | ❌ Requires npm | ✅ `skills upgrade` |
| **Health diagnostics** | ❌ None | ✅ `skills doctor` |
| **Dry-run mode** | ❌ None | ✅ `skills add --dry-run` |
| **Parallel I/O** | Sequential | ✅ Concurrent overwrite checks |
| **Runtime deps** | Node.js 18+, npm/npx | **None** |
| **Memory usage** | ~80MB baseline | **~5MB** |

### Rust-Exclusive Commands

```bash
# Generate shell completions (bash, zsh, fish, powershell)
skills completions bash >> ~/.bashrc
skills completions zsh >> ~/.zshrc
skills completions fish > ~/.config/fish/completions/skills.fish

# Self-update to latest release (like bun upgrade)
skills upgrade

# Health check: broken symlinks, lock consistency, SKILL.md validity
skills doctor

# Preview installation without making changes (CI-friendly)
skills add qntx/skills --dry-run
```

---

## Quick Start

```bash
# Install a skill
skills add qntx/skills

# Search and install interactively
skills find
```

## Install

**Shell** (macOS / Linux):

```sh
curl -fsSL https://sh.qntx.fun/skill | sh
```

**PowerShell** (Windows):

```powershell
irm https://sh.qntx.fun/skill/ps | iex
```

**Cargo**:

```bash
cargo install skills-cli
```

**From Source**:

```bash
git clone https://github.com/qntx/skill.git
cd skill
cargo install --path skills-cli
```

---

## Overview

**skill** manages portable instruction sets ("skills") for AI coding agents.
A skill is a directory containing a `SKILL.md` file with YAML frontmatter that
describes behaviour an agent should adopt. This project provides:

| Crate            | Description                                                                              |
| ---------------  | ---------------------------------------------------------------------------------------- |
| **`skill`**      | Core library — discovery, parsing, installation, lock-file management, provider registry |
| **`skills-cli`** | Binary (`skills`) — interactive CLI with enhanced UX beyond the TypeScript original      |

The Rust implementation achieves **100% command parity** with the original TypeScript CLI
while adding **exclusive features** only possible with native code.

## Features

### Core Capabilities

- **39 supported agents** — Cursor, Claude Code, Windsurf, Cline, Codex, Roo Code, GitHub Copilot, Kilo Code, and [many more](#supported-agents)
- **All source types** — GitHub shorthand (`owner/repo`), GitHub / GitLab URLs with branch + subpath, local paths, well-known HTTP endpoints, direct git URLs
- **Plugin manifests** — `.claude-plugin/marketplace.json` and `plugin.json` discovery for Claude Code ecosystem compatibility
- **Symlink-first install** — canonical storage + per-agent symlinks (or junctions on Windows); copy mode available
- **Lock files** — global (`~/.agents/.skill-lock.json`) and project-scoped (`skills-lock.json`) for reproducible setups
- **Security audits** — displays partner security assessments (Gen, Socket, Snyk) before installation

### Rust-Exclusive Advantages

- **Shell completions** — native tab completion for bash, zsh, fish, PowerShell
- **Self-update** — `skills upgrade` downloads latest release directly from GitHub
- **Health diagnostics** — `skills doctor` checks broken symlinks, lock consistency, SKILL.md validity
- **Dry-run mode** — `skills add --dry-run` previews changes without modifying filesystem
- **Parallel I/O** — concurrent filesystem operations via `tokio::JoinSet`
- **Instant startup** — no JIT warmup, no module resolution, just native machine code
- **Embeddable library** — `skill` crate with clean `SkillManager` API for agent frameworks

## Source Formats

```bash
# GitHub shorthand (owner/repo)
skills add qntx/skills

# Full GitHub URL
skills add https://github.com/qntx/skills

# Direct path to a skill in a repo
skills add https://github.com/qntx/skills/tree/main/skills/code-review

# GitLab URL
skills add https://gitlab.com/org/repo

# Prefix shorthand
skills add github:owner/repo
skills add gitlab:owner/repo

# Local path
skills add ./my-local-skills
skills add /absolute/path/to/skills

# Well-known HTTP endpoints
skills add https://example.com   # checks /.well-known/skills/index.json
```

## `skills add` Options

| Option                    | Description                                                                                   |
| ------------------------- | -------------------------------------------------------------------------------               |
| `-g, --global`            | Install to user directory instead of project                                                  |
| `-a, --agent <agents...>` | Target specific agents (e.g., `claude-code`, `cursor`). Use `'*'` for all                     |
| `-s, --skill <skills...>` | Install specific skills by name (use `'*'` for all skills)                                    |
| `-l, --list`              | List available skills without installing                                                      |
| `--copy`                  | Copy files instead of symlinking to agent directories                                         |
| `-y, --yes`               | Skip all confirmation prompts                                                                 |
| `--all`                   | Install all skills to all agents without prompts (shorthand for `--skill '*' --agent '*' -y`) |

### Examples

```bash
# List skills in a repository
skills add qntx/skills --list

# Install specific skills
skills add qntx/skills --skill frontend-design --skill code-review

# Install a skill with spaces in the name (must be quoted)
skills add owner/repo --skill "Convex Best Practices"

# Install to specific agents
skills add qntx/skills -a claude-code -a cursor

# Non-interactive installation (CI/CD friendly)
skills add qntx/skills --skill frontend-design -g -a claude-code -y

# Install all skills from a repo to all agents
skills add qntx/skills --all

# Install all skills to specific agents
skills add qntx/skills --skill '*' -a claude-code

# Install specific skills to all agents
skills add qntx/skills --agent '*' --skill frontend-design
```

### Installation Scope

| Scope       | Flag      | Location            | Use Case                                      |
| ----------- | --------- | ------------------- | --------------------------------------------- |
| **Project** | (default) | `./<agent>/skills/` | Committed with your project, shared with team |
| **Global**  | `-g`      | `~/<agent>/skills/` | Available across all projects                 |

### Installation Methods

| Method                    | Description                                                                                  |
| ------------------------- | -------------------------------------------------------------------------------------------- |
| **Symlink** (Recommended) | Creates symlinks from each agent to a canonical copy. Single source of truth, easy updates.  |
| **Copy**                  | Creates independent copies for each agent. Use when symlinks aren't supported.               |

> **Note:** On Windows, junctions are used instead of symlinks for directory linking.

## All Commands

| Command                       | Aliases             | Description                                  |
| ----------------------------- | ------------------- | -------------------------------------------- |
| `skills add <source>`         | `a`, `install`, `i` | Add a skill package from any source          |
| `skills remove <name>`        | `rm`, `r`           | Remove installed skills                      |
| `skills list`                 | `ls`                | List installed skills                        |
| `skills find <query>`         | `f`, `s`, `search`  | Search the skills registry                   |
| `skills check`                |                     | Check for available skill updates            |
| `skills update`               |                     | Update all skills to latest versions         |
| `skills init`                 |                     | Scaffold a new `SKILL.md`                    |
| `skills completions <shell>`  |                     | Generate shell completions *(Rust-only)*     |
| `skills doctor`               |                     | Health check: symlinks, locks *(Rust-only)*  |
| `skills upgrade`              |                     | Self-update CLI binary *(Rust-only)*         |
| `skills experimental_install` |                     | Restore skills from `skills-lock.json`       |
| `skills experimental_sync`    |                     | Sync skills from `node_modules`              |

### `skills list`

List all installed skills. Similar to `npm ls`.

```bash
# List all installed skills (project and global)
skills list

# List only global skills
skills ls -g

# Filter by specific agents
skills ls -a claude-code -a cursor
```

### `skills find`

Search for skills interactively or by keyword.

```bash
# Interactive search (fzf-style)
skills find

# Search by keyword
skills find typescript
```

### `skills check` / `skills update`

```bash
# Check if any installed skills have updates
skills check

# Update all skills to latest versions
skills update
```

### `skills init`

```bash
# Create SKILL.md in current directory
skills init

# Create a new skill in a subdirectory
skills init my-skill
```

### `skills remove`

Remove installed skills from agents.

```bash
# Remove interactively (select from installed skills)
skills remove

# Remove specific skill by name
skills remove web-design-guidelines

# Remove multiple skills
skills remove frontend-design web-design-guidelines

# Remove from global scope
skills remove --global web-design-guidelines

# Remove from specific agents only
skills remove --agent claude-code cursor my-skill

# Remove all installed skills without confirmation
skills remove --all

# Remove all skills from a specific agent
skills remove --skill '*' -a cursor

# Remove a specific skill from all agents
skills remove my-skill --agent '*'

# Use 'rm' alias
skills rm my-skill
```

| Option         | Description                                      |
| -------------- | ------------------------------------------------ |
| `-g, --global` | Remove from global scope (~/) instead of project |
| `-a, --agent`  | Remove from specific agents (use `'*'` for all)  |
| `-s, --skill`  | Specify skills to remove (use `'*'` for all)     |
| `-y, --yes`    | Skip confirmation prompts                        |
| `--all`        | Shorthand for `--skill '*' --agent '*' -y`       |

## Library Usage

```rust
use skill::SkillManager;
use skill::types::{DiscoverOptions, InstallOptions, InstallScope, InstallMode, AgentId};

#[tokio::main]
async fn main() -> skill::Result<()> {
    let manager = SkillManager::builder().build();

    // Discover skills in a directory
    let skills = manager
        .discover_skills(std::path::Path::new("./my-repo"), &DiscoverOptions::default())
        .await?;

    // Install a skill for a specific agent
    let opts = InstallOptions {
        scope: InstallScope::Project,
        mode: InstallMode::Symlink,
        cwd: None,
    };
    for skill in &skills {
        manager.install_skill(skill, &AgentId::new("cursor"), &opts).await?;
    }

    // List installed skills
    let installed = manager.list_installed(&Default::default()).await?;
    println!("Installed: {}", installed.len());

    Ok(())
}
```

## Supported Agents

<details>
<summary>Click to expand the full list of 39 supported agents</summary>

| Agent          | Skills Dir             | Global Skills Dir                |
| -------------- | ---------------------- | -------------------------------- |
| Amp            | `.agents/skills`       | `$XDG_CONFIG_HOME/agents/skills` |
| Antigravity    | `.agent/skills`        | `~/.gemini/antigravity/skills`   |
| Augment        | `.augment/skills`      | `~/.augment/skills`              |
| Claude Code    | `.claude/skills`       | `~/.claude/skills`               |
| OpenClaw       | `.agents/skills`       | `~/.openclaw/skills`             |
| Cline          | `.agents/skills`       | `~/.agents/skills`               |
| CodeBuddy      | `.codebuddy/skills`    | `~/.codebuddy/skills`            |
| Codex          | `.agents/skills`       | `$CODEX_HOME/skills`             |
| Command Code   | `.commandcode/skills`  | `~/.commandcode/skills`          |
| Continue       | `.continue/skills`     | `~/.continue/skills`             |
| Cortex Code    | `.cortex/skills`       | `~/.snowflake/cortex/skills`     |
| Crush          | `.crush/skills`        | `~/.crush/skills`                |
| Cursor         | `.cursor/rules/skills` | `~/.cursor/rules/skills`         |
| Droid          | `.droid/skills`        | `~/.droid/skills`                |
| Factory Code   | `.factory/skills`      | `~/.factory/skills`              |
| Gemini CLI     | `.gemini/skills`       | `~/.gemini/skills`               |
| GitHub Copilot | `.github/skills`       | `~/.github/skills`               |
| Goose          | `.goose/skills`        | `~/.config/goose/skills`         |
| iFlow CLI      | `.iflow/skills`        | `~/.iflow/skills`                |
| Junie          | `.junie/skills`        | `~/.junie/skills`                |
| Kilo Code      | `.kilocode/skills`     | `~/.kilocode/skills`             |
| Kimi K2        | `.kimi/skills`         | `~/.kimi/skills`                 |
| Kiro           | `.kiro/skills`         | `~/.kiro/skills`                 |
| Kode           | `.kode/skills`         | `~/.kode/skills`                 |
| MCPJam         | `.mcpjam/skills`       | `~/.mcpjam/skills`               |
| Mistral Vibe   | `.vibe/skills`         | `~/.vibe/skills`                 |
| Mux            | `.mux/skills`          | `~/.mux/skills`                  |
| Neovate        | `.neovate/skills`      | `~/.neovate/skills`              |
| OpenCode       | `.opencode/skills`     | `~/.opencode/skills`             |
| OpenHands      | `.openhands/skills`    | `~/.openhands/skills`            |
| Pi             | `.pi/skills`           | `~/.pi/skills`                   |
| Qoder          | `.qoder/skills`        | `~/.qoder/skills`                |
| Qwen Code      | `.qwen/skills`         | `~/.qwen/skills`                 |
| Replit         | `.agents/skills`       | `$XDG_CONFIG_HOME/agents/skills` |
| Roo Code       | `.roo/skills`          | `~/.roo/skills`                  |
| Trae           | `.trae/skills`         | `~/.trae/skills`                 |
| Trae CN        | `.trae/skills`         | `~/.trae-cn/skills`              |
| Windsurf       | `.windsurf/skills`     | `~/.windsurf/skills`             |
| ZenCoder       | `.zencoder/skills`     | `~/.zencoder/skills`             |

</details>

## Creating Skills

Skills are directories containing a `SKILL.md` file with YAML frontmatter:

```markdown
---
name: my-skill
description: What this skill does and when to use it
---

# My Skill

Instructions for the agent to follow when this skill is activated.

## When to Use

Describe the scenarios where this skill should be used.

## Steps

1. First, do this
2. Then, do that
```

### Required Fields

- **`name`** — Unique identifier (lowercase, hyphens allowed)
- **`description`** — Brief explanation of what the skill does

### Optional Fields

- **`metadata.internal`** — Set to `true` to hide the skill from normal discovery. Internal skills are only visible when `INSTALL_INTERNAL_SKILLS=1` is set.
- **`metadata.tags`** — Array of tags for categorization and search.

```markdown
---
name: my-internal-skill
description: An internal skill not shown by default
metadata:
  internal: true
  tags: [internal, wip]
---
```

### Skill Discovery

The CLI searches for skills in these locations within a repository:

- Root directory (if it contains `SKILL.md`)
- `skills/`, `skills/.curated/`, `skills/.experimental/`, `skills/.system/`
- Agent-specific directories: `.claude/skills/`, `.cursor/skills/`, `.windsurf/skills/`, etc.

### Plugin Manifest Discovery

If `.claude-plugin/marketplace.json` or `.claude-plugin/plugin.json` exists, skills declared in those files are also discovered:

```json
{
  "metadata": { "pluginRoot": "./plugins" },
  "plugins": [
    {
      "name": "my-plugin",
      "source": "my-plugin",
      "skills": ["./skills/review", "./skills/test"]
    }
  ]
}
```

This enables compatibility with the [Claude Code plugin marketplace](https://code.claude.com/docs/en/plugin-marketplaces) ecosystem.

## Design Principles

- **Feature parity** — Every command, source format, agent, and behaviour from
  the Vercel TypeScript implementation is faithfully reproduced.
- **Zero runtime dependencies** — Single static binary; no Node.js, npm, or npx
  required.
- **Library-first** — The `skill` crate exposes a clean `SkillManager` API so
  agent frameworks can embed skill support directly.
- **Security by default** — Path traversal protection on subpaths and skill
  names; plugin manifest paths validated against base directory.
- **Extensible** — Custom agents via `AgentRegistry::register()`, custom
  providers via the `HostProvider` trait, feature-gated network and telemetry.

## Feature Flags

| Flag        | Default | Description                                                                  |
| ----------- | ------- | ---------------------------------------------------------------------------- |
| `network`   | off     | Enables `reqwest`-based network operations (GitHub API, well-known fetching) |
| `telemetry` | off     | Enables anonymous usage telemetry (GET requests with URL params)             |

The CLI binary enables both flags. Library consumers can opt in selectively.

## Environment Variables

| Variable                             | Description                                                            |
| ------------------------------------ | ---------------------------------------------------------------------- |
| `CODEX_HOME`                         | Override Codex config directory (default: `~/.codex`)                  |
| `CLAUDE_CONFIG_DIR`                  | Override Claude Code config directory (default: `~/.claude`)           |
| `XDG_CONFIG_HOME`                    | Override XDG config base (default: `~/.config`)                        |
| `INSTALL_INTERNAL_SKILLS`            | Set to `1` or `true` to include internal skills                        |
| `GITHUB_TOKEN` / `GH_TOKEN`          | GitHub API token for update checks (falls back to `gh auth token`)     |
| `SKILLS_API_URL`                     | Override the skills search API endpoint (default: `https://skills.sh`) |
| `DISABLE_TELEMETRY` / `DO_NOT_TRACK` | Set to `1` or `true` to disable telemetry                              |
| `GIT_TERMINAL_PROMPT`                | Set to `0` to disable git credential prompts (auto-set by CLI)         |

## Compatibility

This project is a **drop-in replacement** for the
[Vercel `skills` CLI](https://github.com/vercel-labs/skills) (`npx skills`).

- Same CLI interface, flags, and aliases
- Same `SKILL.md` format and YAML frontmatter schema
- Same lock-file format (`skill-lock.json` v3, `skills-lock.json` v1)
- Same agent directory structures and detection logic
- Same source parsing rules (GitHub, GitLab, local, well-known, git)
- Same telemetry endpoint and event schema
- Same installation semantics (canonical + symlink/junction)

## Agent Compatibility

Skills are generally compatible across agents since they follow a shared [Agent Skills specification](https://agentskills.io). However, some features may be agent-specific:

| Feature         | Claude Code | Cursor | Windsurf | Codex | Cline | Roo Code | OpenCode |
| --------------- | ----------- | ------ | -------- | ----- | ----- | -------- | -------- |
| Basic skills    | ✅          | ✅     | ✅       | ✅    | ✅    | ✅       | ✅       |
| `allowed-tools` | ✅          | ✅     | ✅       | ✅    | ✅    | ✅       | ✅       |
| `context: fork` | ✅          | ❌     | ❌       | ❌    | ❌    | ❌       | ❌       |
| Hooks           | ✅          | ❌     | ❌       | ❌    | ✅    | ❌       | ❌       |

## Troubleshooting

### "No skills found"

Ensure the repository contains valid `SKILL.md` files with both `name` and `description` in the frontmatter.

### Skill not loading in agent

- Verify the skill was installed to the correct path
- Check the agent's documentation for skill loading requirements
- Ensure the `SKILL.md` frontmatter is valid YAML

### Permission errors

Ensure you have write access to the target directory. On Windows, you may need to run as Administrator for junction creation.

### Symlink issues on Windows

Windows requires Developer Mode or Administrator privileges for symlinks. The CLI automatically falls back to junctions for directories.

## Security

- **Path traversal protection** — All subpaths and skill names are validated to
  prevent directory escape attacks.
- **Plugin manifest sandboxing** — Paths declared in `.claude-plugin/` manifests
  must be contained within the base directory and start with `./`.
- **No arbitrary code execution** — Skills are passive markdown instruction files;
  they do not execute code during installation.
- **Dependency audit** — Minimal dependency tree; only well-known crates from
  the Rust ecosystem.

## Related Links

- [Agent Skills Specification](https://agentskills.io)
- [Skills Directory](https://skills.sh)
- [Claude Code Skills Documentation](https://code.claude.com/docs/en/skills)
- [Cursor Skills Documentation](https://cursor.com/docs/context/skills)
- [Windsurf Skills Documentation](https://docs.codeium.com/windsurf/skills)
- [Codex Skills Documentation](https://developers.openai.com/codex/skills)
- [Cline Skills Documentation](https://docs.cline.bot/features/skills)
- [Roo Code Skills Documentation](https://docs.roocode.com/features/skills)
- [OpenCode Skills Documentation](https://opencode.ai/docs/skills)
- [GitHub Copilot Agent Skills](https://docs.github.com/en/copilot/concepts/agents/about-agent-skills)
- [Vercel Agent Skills Repository](https://github.com/vercel-labs/agent-skills)

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT License ([LICENSE-MIT](LICENSE-MIT) or <https://opensource.org/licenses/MIT>)

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this project shall be dual-licensed as above, without any additional terms or conditions.

---

<div align="center">

A **[QNTX](https://qntx.fun)** open-source project.

<a href="https://qntx.fun"><img alt="QNTX" width="369" src="https://raw.githubusercontent.com/qntx/.github/main/profile/qntx-banner.svg" /></a>

<!--prettier-ignore-->
Code is law. We write both.

</div>
