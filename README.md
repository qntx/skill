<div align="center">

# Skill

**The open agent skills ecosystem — rewritten in Rust.**

[![Crates.io](https://img.shields.io/crates/v/skill.svg)](https://crates.io/crates/skill)
[![docs.rs](https://img.shields.io/docsrs/skill)](https://docs.rs/skill)
[![CI](https://img.shields.io/github/actions/workflow/status/qntx/skill/rust.yml?label=CI)](https://github.com/qntx/skill/actions)
[![License: MIT/Apache-2.0](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE)

A drop-in, feature-equivalent Rust replacement for the
[Vercel `skills` CLI](https://github.com/vercel-labs/skills).
Single static binary, zero runtime dependencies, full API compatibility.

</div>

---

## Overview

**skill** manages portable instruction sets ("skills") for AI coding agents.
A skill is a directory containing a `SKILL.md` file with YAML frontmatter that
describes behaviour an agent should adopt. This project provides:

| Crate           | Description                                                                              |
| --------------- | ---------------------------------------------------------------------------------------- |
| **`skill`**     | Core library — discovery, parsing, installation, lock-file management, provider registry |
| **`skill-cli`** | Binary (`skills`) — interactive CLI with the same UX as the TypeScript original          |

The Rust port achieves **100 % command parity** with the original TypeScript CLI
while adding the performance and reliability benefits of a native compiled binary.

## Features

- **39 supported agents** — Cursor, Claude Code, Windsurf, Cline, Codex, Roo Code, GitHub Copilot, Kilo Code, and [many more](#supported-agents)
- **All source types** — GitHub shorthand (`owner/repo`), GitHub / GitLab URLs with branch + subpath, local paths, well-known HTTP endpoints, direct git URLs
- **Plugin manifests** — `.claude-plugin/marketplace.json` and `plugin.json` discovery for Claude Code ecosystem compatibility
- **Symlink-first install** — canonical storage + per-agent symlinks (or junctions on Windows); copy mode available
- **Lock files** — global (`~/.agents/.skill-lock.json`) and project-scoped (`skills-lock.json`) for reproducible setups
- **Extensible providers** — `HostProvider` trait for adding custom skill hosts beyond GitHub / GitLab / well-known
- **Telemetry-aware** — opt-in anonymous telemetry (respects `DO_NOT_TRACK` / `DISABLE_TELEMETRY`)
- **Cross-platform** — Linux, macOS, Windows (with native junction support)

## Quick Start

### Install the CLI

**Shell** (macOS / Linux):

```sh
curl -fsSL https://sh.qntx.fun/skill | sh
```

**PowerShell** (Windows):

```powershell
irm https://sh.qntx.fun/skill/ps | iex
```

### CLI

```bash
# Add skills from a GitHub repo
skills add qntx/skills

# Add a specific skill for all agents, non-interactively
skills add owner/repo --skill '*' --agent '*' -y

# Add from a local directory
skills add ./my-skills

# List installed skills
skills list

# Search the skills registry
skills find "code review"

# Check for updates
skills check

# Update all skills
skills update

# Remove a skill
skills remove my-skill

# Initialize a new skill
skills init
```

### Library

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

## CLI Commands

| Command                       | Aliases             | Description                            |
| ----------------------------- | ------------------- | -------------------------------------- |
| `skills add <source>`         | `a`, `install`, `i` | Add a skill package from any source    |
| `skills remove <name>`        | `rm`, `r`           | Remove installed skills                |
| `skills list`                 | `ls`                | List installed skills                  |
| `skills find <query>`         | `f`, `s`, `search`  | Search the skills registry             |
| `skills check`                |                     | Check for available skill updates      |
| `skills update`               | `upgrade`           | Update all skills to latest versions   |
| `skills init`                 |                     | Scaffold a new `SKILL.md`              |
| `skills experimental-install` |                     | Restore skills from `skills-lock.json` |
| `skills experimental-sync`    |                     | Sync skills from `node_modules`        |

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

## Source Formats

The `add` command accepts many source formats:

```bash
# GitHub shorthand
skills add owner/repo
skills add owner/repo/path/to/skill
skills add owner/repo@skill-name

# GitHub URLs
skills add https://github.com/owner/repo
skills add https://github.com/owner/repo/tree/branch/path

# GitLab URLs
skills add https://gitlab.com/group/repo
skills add https://gitlab.com/group/repo/-/tree/branch/path

# Prefix shorthand
skills add github:owner/repo
skills add gitlab:owner/repo

# Local paths
skills add ./my-local-skills
skills add /absolute/path/to/skills

# Well-known HTTP endpoints
skills add https://example.com   # checks /.well-known/skills/index.json
```

## Creating a Skill

A skill is a directory containing a `SKILL.md` file:

```markdown
---
name: my-skill
description: A brief description of what this skill does
metadata:
  tags: [rust, testing]
---

Instructions for the AI agent go here.
The agent will follow these instructions when this skill is active.
```

Use `skills init` to scaffold a new skill interactively.

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
| `telemetry` | off     | Enables anonymous usage telemetry to `https://add-skill.vercel.sh/t`         |

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

## Security

- **Path traversal protection** — All subpaths and skill names are validated to
  prevent directory escape attacks.
- **Plugin manifest sandboxing** — Paths declared in `.claude-plugin/` manifests
  must be contained within the base directory and start with `./`.
- **No arbitrary code execution** — Skills are passive markdown instruction files;
  they do not execute code during installation.
- **Dependency audit** — Minimal dependency tree; only well-known crates from
  the Rust ecosystem.

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
