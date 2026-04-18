//! Extension discovery and lightweight manifest loading for ccodex.

mod agents;
mod hooks;
mod markdown;
mod paths;
mod plugins;
mod registry;
mod util;

use std::path::Path;

use thiserror::Error;

use ccodex_protocol::{AgentSpec, ExtensionKind, ExtensionManifest};

use agents::load_agent_layers;
use hooks::{load_hook_layers, scan_hook_layers};
use markdown::load_markdown_layers;
pub use paths::ExtensionRoots;
use paths::skill_dir_name;
use plugins::{scan_agent_layers, scan_plugin_layers, scan_skill_layers};

#[derive(Debug, Error)]
pub enum ExtensionError {
    #[error("failed to read directory {path}: {message}")]
    ReadDirectory { path: String, message: String },
    #[error("failed to inspect extension source {path}: {message}")]
    InspectSource { path: String, message: String },
}

#[derive(Debug, Clone, Default)]
pub struct ExtensionRegistry {
    manifests: Vec<ExtensionManifest>,
}

#[derive(Debug, Clone)]
pub struct MarkdownExtension {
    pub manifest: ExtensionManifest,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookEvent {
    PreTurn,
    PostTurn,
    PreTool,
    PostTool,
}

#[derive(Debug, Clone)]
pub struct HookDefinition {
    pub manifest: ExtensionManifest,
    pub event: HookEvent,
    pub command: String,
    pub timeout_ms: u64,
}

fn sort_manifests(manifests: &mut [ExtensionManifest]) {
    manifests.sort_by(|a, b| a.kind.cmp(&b.kind).then_with(|| a.name.cmp(&b.name)));
}

impl ExtensionRegistry {
    pub fn new() -> Self {
        Self {
            manifests: Vec::new(),
        }
    }

    pub fn discover_for_workspace(workspace_root: &Path) -> Result<Self, ExtensionError> {
        Self::discover_for_roots(&ExtensionRoots::for_ccodex_workspace(workspace_root))
    }

    pub fn discover_for_roots(roots: &ExtensionRoots) -> Result<Self, ExtensionError> {
        let mut registry = Self::new();

        scan_plugin_layers(&mut registry, roots)?;
        scan_skill_layers(&mut registry, roots)?;
        scan_agent_layers(&mut registry, roots)?;
        scan_hook_layers(&mut registry, roots)?;

        sort_manifests(&mut registry.manifests);

        Ok(registry)
    }

    pub fn manifests(&self) -> &[ExtensionManifest] {
        &self.manifests
    }

    pub fn load_skill_instructions_for_workspace(
        workspace_root: &Path,
    ) -> Result<Vec<MarkdownExtension>, ExtensionError> {
        Self::load_skill_instructions_for_roots(&ExtensionRoots::for_ccodex_workspace(
            workspace_root,
        ))
    }

    pub fn load_skill_instructions_for_roots(
        roots: &ExtensionRoots,
    ) -> Result<Vec<MarkdownExtension>, ExtensionError> {
        load_markdown_layers(roots, skill_dir_name(), ExtensionKind::Skill)
    }

    pub fn load_agents_for_workspace(
        workspace_root: &Path,
    ) -> Result<Vec<AgentSpec>, ExtensionError> {
        Self::load_agents_for_roots(&ExtensionRoots::for_ccodex_workspace(workspace_root))
    }

    pub fn load_agents_for_roots(roots: &ExtensionRoots) -> Result<Vec<AgentSpec>, ExtensionError> {
        load_agent_layers(roots)
    }

    pub fn load_hooks_for_workspace(
        workspace_root: &Path,
    ) -> Result<Vec<HookDefinition>, ExtensionError> {
        Self::load_hooks_for_roots(&ExtensionRoots::for_ccodex_workspace(workspace_root))
    }

    pub fn load_hooks_for_roots(
        roots: &ExtensionRoots,
    ) -> Result<Vec<HookDefinition>, ExtensionError> {
        load_hook_layers(roots)
    }

    pub(crate) fn push_manifest(&mut self, manifest: ExtensionManifest) {
        if let Some(existing) = self
            .manifests
            .iter_mut()
            .find(|existing| existing.kind == manifest.kind && existing.name == manifest.name)
        {
            *existing = manifest;
        } else {
            self.manifests.push(manifest);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use ccodex_protocol::ExtensionKind;

    use super::{ExtensionRegistry, ExtensionRoots, HookEvent};

    fn external_roots(root: &Path) -> ExtensionRoots {
        ExtensionRoots::new(vec![
            root.join("plugins").join("builtin"),
            root.join(".ccodex"),
            root.join(".claude"),
            root.join(".codex"),
            root.join(".hermes"),
        ])
    }

    #[test]
    fn discovers_project_extensions() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock should be monotonic enough")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("ccodex-extensions-{unique}"));
        let project_dir = root.join(".ccodex");
        let claude_dir = root.join(".claude");
        let codex_dir = root.join(".codex");
        let hermes_dir = root.join(".hermes");

        std::fs::create_dir_all(project_dir.join("plugins").join("demo-plugin"))
            .expect("plugin dir should exist");
        std::fs::create_dir_all(
            project_dir
                .join("plugins")
                .join("demo-plugin")
                .join("skills"),
        )
        .expect("plugin skill dir should exist");
        std::fs::create_dir_all(
            project_dir
                .join("plugins")
                .join("demo-plugin")
                .join("agents"),
        )
        .expect("plugin agent dir should exist");
        std::fs::create_dir_all(
            project_dir
                .join("plugins")
                .join("demo-plugin")
                .join("hooks"),
        )
        .expect("plugin hook dir should exist");
        std::fs::create_dir_all(project_dir.join("skills")).expect("skills dir should exist");
        std::fs::create_dir_all(project_dir.join("agents")).expect("agents dir should exist");
        std::fs::create_dir_all(project_dir.join("hooks")).expect("hooks dir should exist");
        std::fs::create_dir_all(claude_dir.join("skills")).expect("claude skills dir should exist");
        std::fs::create_dir_all(claude_dir.join("agents")).expect("claude agents dir should exist");
        std::fs::create_dir_all(codex_dir.join("skills")).expect("codex skills dir should exist");
        std::fs::create_dir_all(hermes_dir.join("hooks")).expect("hermes hooks dir should exist");
        std::fs::write(
            project_dir
                .join("plugins")
                .join("demo-plugin")
                .join("plugin.toml"),
            "name = \"review-pack\"\nversion = \"0.1.0\"\ndescription = \"Review workflows\"",
        )
        .expect("plugin manifest should write");

        std::fs::write(
            project_dir.join("skills").join("review.md"),
            "# Review Skill",
        )
        .expect("skill file should write");
        std::fs::write(
            project_dir.join("agents").join("builder.md"),
            "# Builder Agent",
        )
        .expect("agent file should write");
        std::fs::write(
            project_dir.join("hooks").join("format.toml"),
            "command = \"echo hi\"",
        )
        .expect("hook file should write");
        std::fs::write(
            project_dir
                .join("plugins")
                .join("demo-plugin")
                .join("skills")
                .join("plugin-review.md"),
            "# Plugin Review Skill",
        )
        .expect("plugin skill should write");
        std::fs::write(
            project_dir
                .join("plugins")
                .join("demo-plugin")
                .join("agents")
                .join("plugin-builder.md"),
            "# Plugin Builder Agent",
        )
        .expect("plugin agent should write");
        std::fs::write(
            project_dir
                .join("plugins")
                .join("demo-plugin")
                .join("hooks")
                .join("plugin-post.toml"),
            "command = \"echo plugin-hi\"",
        )
        .expect("plugin hook should write");
        std::fs::write(
            claude_dir.join("skills").join("claude-review.md"),
            "# Claude Review Skill",
        )
        .expect("claude skill should write");
        std::fs::write(
            claude_dir.join("agents").join("claude-builder.md"),
            "# Claude Builder Agent",
        )
        .expect("claude agent should write");
        std::fs::write(
            codex_dir.join("skills").join("codex-review.md"),
            "# Codex Review Skill",
        )
        .expect("codex skill should write");
        std::fs::write(
            hermes_dir.join("hooks").join("hermes-post.toml"),
            "command = \"echo hermes-hi\"",
        )
        .expect("hermes hook should write");

        let registry = ExtensionRegistry::discover_for_roots(&external_roots(&root))
            .expect("registry should load");
        let names = registry
            .manifests()
            .iter()
            .map(|manifest| format!("{:?}:{}", manifest.kind, manifest.name))
            .collect::<Vec<_>>();

        assert!(names.iter().any(|item| item == "Plugin:review-pack"));
        assert!(names.iter().any(|item| item == "Skill:review"));
        assert!(names.iter().any(|item| item == "Skill:plugin-review"));
        assert!(names.iter().any(|item| item == "Skill:claude-review"));
        assert!(names.iter().any(|item| item == "Skill:codex-review"));
        assert!(names.iter().any(|item| item == "Agent:builder"));
        assert!(names.iter().any(|item| item == "Agent:plugin-builder"));
        assert!(names.iter().any(|item| item == "Agent:claude-builder"));
        assert!(names.iter().any(|item| item == "Hook:format"));
        assert!(names.iter().any(|item| item == "Hook:plugin-post"));
        assert!(names.iter().any(|item| item == "Hook:hermes-post"));
        let plugin = registry
            .manifests()
            .iter()
            .find(|manifest| manifest.kind == ExtensionKind::Plugin)
            .expect("plugin manifest should exist");
        assert_eq!(plugin.version.as_deref(), Some("0.1.0"));
        assert_eq!(plugin.description.as_deref(), Some("Review workflows"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn loads_skill_instructions_from_builtin_and_project_layers() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock should be monotonic enough")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("ccodex-skill-load-{unique}"));
        let builtin_skills = root.join("plugins").join("builtin").join("skills");
        let plugin_skills = root
            .join(".ccodex")
            .join("plugins")
            .join("review-pack")
            .join("skills");
        let project_skills = root.join(".ccodex").join("skills");
        let claude_skills = root.join(".claude").join("skills");
        let codex_skills = root.join(".codex").join("skills");
        let hermes_skills = root.join(".hermes").join("skills");

        std::fs::create_dir_all(&builtin_skills).expect("builtin skills dir should exist");
        std::fs::create_dir_all(&plugin_skills).expect("plugin skills dir should exist");
        std::fs::create_dir_all(&project_skills).expect("project skills dir should exist");
        std::fs::create_dir_all(&claude_skills).expect("claude skills dir should exist");
        std::fs::create_dir_all(&codex_skills).expect("codex skills dir should exist");
        std::fs::create_dir_all(&hermes_skills).expect("hermes skills dir should exist");

        std::fs::write(
            builtin_skills.join("repo.md"),
            "---\ntitle: Repo\n---\n# Repo Skill\nExplain the repository.",
        )
        .expect("builtin skill should write");
        std::fs::write(
            plugin_skills.join("plugin.md"),
            "# Plugin Skill\nUse plugin-specific guidance.",
        )
        .expect("plugin skill should write");
        std::fs::write(
            project_skills.join("local.md"),
            "# Local Skill\nFocus on the current workspace.",
        )
        .expect("project skill should write");
        std::fs::write(
            claude_skills.join("legacy.md"),
            "# Claude Skill\nApply legacy Claude-compatible behavior.",
        )
        .expect("claude skill should write");
        std::fs::write(
            codex_skills.join("workspace.md"),
            "# Codex Skill\nUse codex-specific conventions.",
        )
        .expect("codex skill should write");
        std::fs::write(
            hermes_skills.join("memory.md"),
            "# Hermes Skill\nUse memory-aware summaries.",
        )
        .expect("hermes skill should write");

        let skills = ExtensionRegistry::load_skill_instructions_for_roots(&external_roots(&root))
            .expect("skills should load");

        assert_eq!(skills.len(), 6);
        let names = skills
            .iter()
            .map(|skill| skill.manifest.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names[0], "repo");
        assert!(names.contains(&"plugin"));
        assert!(names.contains(&"local"));
        assert!(names.contains(&"legacy"));
        assert!(names.contains(&"workspace"));
        assert!(names.contains(&"memory"));
        assert!(
            skills
                .iter()
                .any(|skill| skill.content.contains("Repo Skill"))
        );
        assert!(
            skills
                .iter()
                .any(|skill| skill.content.contains("plugin-specific guidance"))
        );
        assert!(
            skills
                .iter()
                .any(|skill| skill.content.contains("Focus on the current workspace."))
        );
        assert!(skills.iter().any(|skill| {
            skill
                .content
                .contains("Apply legacy Claude-compatible behavior.")
        }));
        assert!(
            skills
                .iter()
                .any(|skill| skill.content.contains("Use codex-specific conventions."))
        );
        assert!(
            skills
                .iter()
                .any(|skill| skill.content.contains("Use memory-aware summaries."))
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn skill_layers_override_lower_precedence_entries_by_name() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock should be monotonic enough")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("ccodex-skill-override-{unique}"));
        let builtin_skills = root.join("plugins").join("builtin").join("skills");
        let project_skills = root.join(".ccodex").join("skills");
        let claude_skills = root.join(".claude").join("skills");

        std::fs::create_dir_all(&builtin_skills).expect("builtin skills dir should exist");
        std::fs::create_dir_all(&project_skills).expect("project skills dir should exist");
        std::fs::create_dir_all(&claude_skills).expect("claude skills dir should exist");

        std::fs::write(
            builtin_skills.join("review.md"),
            "# Builtin Review\nUse builtin guidance.",
        )
        .expect("builtin skill should write");
        std::fs::write(
            project_skills.join("review.md"),
            "# Project Review\nUse project guidance.",
        )
        .expect("project skill should write");
        std::fs::write(
            claude_skills.join("review.md"),
            "# Claude Review\nUse claude guidance.",
        )
        .expect("claude skill should write");

        let skills =
            ExtensionRegistry::load_skill_instructions_for_roots(&ExtensionRoots::new(vec![
                root.join("plugins").join("builtin"),
                root.join(".ccodex"),
                root.join(".claude"),
            ]))
            .expect("skills should load");

        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].manifest.name, "review");
        assert!(skills[0].content.contains("Use claude guidance."));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn loads_hook_definitions_from_project_and_claude_layers() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock should be monotonic enough")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("ccodex-hook-load-{unique}"));
        let project_hooks = root.join(".ccodex").join("hooks");
        let claude_hooks = root.join(".claude").join("hooks");
        let codex_hooks = root.join(".codex").join("hooks");
        let plugin_hooks = root
            .join(".ccodex")
            .join("plugins")
            .join("review-pack")
            .join("hooks");

        std::fs::create_dir_all(&project_hooks).expect("project hooks dir should exist");
        std::fs::create_dir_all(&claude_hooks).expect("claude hooks dir should exist");
        std::fs::create_dir_all(&codex_hooks).expect("codex hooks dir should exist");
        std::fs::create_dir_all(&plugin_hooks).expect("plugin hooks dir should exist");
        std::fs::write(
            project_hooks.join("prepare.toml"),
            "event = \"pre_turn\"\ncommand = \"echo project-pre\"",
        )
        .expect("project hook should write");
        std::fs::write(
            plugin_hooks.join("plugin.toml"),
            "event = \"post_tool\"\ncommand = \"echo plugin-post-tool\"",
        )
        .expect("plugin hook should write");
        std::fs::write(
            claude_hooks.join("finish.toml"),
            "event = \"post_turn\"\ncommand = \"echo claude-post\"",
        )
        .expect("claude hook should write");
        std::fs::write(
            codex_hooks.join("codex.toml"),
            "event = \"pre_tool\"\ncommand = \"echo codex-pre-tool\"",
        )
        .expect("codex hook should write");

        let hooks = ExtensionRegistry::load_hooks_for_roots(&external_roots(&root))
            .expect("hooks should load");
        assert_eq!(hooks.len(), 4);
        assert_eq!(hooks[0].event, HookEvent::PreTurn);
        assert_eq!(hooks[0].command, "echo project-pre");
        assert_eq!(hooks[0].timeout_ms, 5_000);
        assert_eq!(hooks[1].event, HookEvent::PostTool);
        assert_eq!(hooks[1].command, "echo plugin-post-tool");
        assert_eq!(hooks[2].event, HookEvent::PostTurn);
        assert_eq!(hooks[2].command, "echo claude-post");
        assert_eq!(hooks[3].event, HookEvent::PreTool);
        assert_eq!(hooks[3].command, "echo codex-pre-tool");

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn hook_layers_override_lower_precedence_entries_by_name() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock should be monotonic enough")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("ccodex-hook-override-{unique}"));
        let project_hooks = root.join(".ccodex").join("hooks");
        let claude_hooks = root.join(".claude").join("hooks");

        std::fs::create_dir_all(&project_hooks).expect("project hooks dir should exist");
        std::fs::create_dir_all(&claude_hooks).expect("claude hooks dir should exist");
        std::fs::write(
            project_hooks.join("prepare.toml"),
            "event = \"pre_turn\"\ncommand = \"echo project-pre\"",
        )
        .expect("project hook should write");
        std::fs::write(
            claude_hooks.join("prepare.toml"),
            "event = \"post_turn\"\ncommand = \"echo claude-post\"",
        )
        .expect("claude hook should write");

        let hooks = ExtensionRegistry::load_hooks_for_roots(&ExtensionRoots::new(vec![
            root.join(".ccodex"),
            root.join(".claude"),
        ]))
        .expect("hooks should load");
        let prepare = hooks
            .into_iter()
            .find(|hook| hook.manifest.name == "prepare")
            .expect("prepare hook should exist");
        assert_eq!(prepare.event, HookEvent::PostTurn);
        assert_eq!(prepare.command, "echo claude-post");

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn loads_agent_specs_from_project_and_claude_layers() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock should be monotonic enough")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("ccodex-agents-{unique}"));
        std::fs::create_dir_all(root.join(".ccodex").join("agents"))
            .expect("project agents dir should exist");
        std::fs::create_dir_all(
            root.join(".ccodex")
                .join("plugins")
                .join("review-pack")
                .join("agents"),
        )
        .expect("plugin agents dir should exist");
        std::fs::create_dir_all(root.join(".claude").join("agents"))
            .expect("claude agents dir should exist");
        std::fs::create_dir_all(root.join(".codex").join("agents"))
            .expect("codex agents dir should exist");

        std::fs::write(
            root.join(".ccodex").join("agents").join("reviewer.md"),
            "---\nname = \"Reviewer\"\n---\n# Reviewer\nReview carefully.",
        )
        .expect("project agent should write");
        std::fs::write(
            root.join(".ccodex")
                .join("plugins")
                .join("review-pack")
                .join("agents")
                .join("plugin-reviewer.md"),
            "# Plugin Reviewer\nReview from plugin package.",
        )
        .expect("plugin agent should write");
        std::fs::write(
            root.join(".claude").join("agents").join("implementer.md"),
            "# Implementer\nShip the change.",
        )
        .expect("claude agent should write");
        std::fs::write(
            root.join(".codex").join("agents").join("planner.md"),
            "# Planner\nBuild a concrete execution plan.",
        )
        .expect("codex agent should write");

        let agents = ExtensionRegistry::load_agents_for_roots(&external_roots(&root))
            .expect("agents should load");

        assert_eq!(agents.len(), 4);
        assert_eq!(agents[0].name, "reviewer");
        assert!(agents[0].instructions.contains("Review carefully."));
        assert_eq!(agents[1].name, "plugin-reviewer");
        assert!(agents[1].instructions.contains("plugin package"));
        assert_eq!(agents[2].name, "implementer");
        assert!(agents[2].instructions.contains("Ship the change."));
        assert_eq!(agents[3].name, "planner");
        assert!(agents[3].instructions.contains("concrete execution plan"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn agent_layers_override_lower_precedence_entries_by_name() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock should be monotonic enough")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("ccodex-agent-override-{unique}"));
        let builtin_agents = root.join("plugins").join("builtin").join("agents");
        let project_agents = root.join(".ccodex").join("agents");

        std::fs::create_dir_all(&builtin_agents).expect("builtin agents dir should exist");
        std::fs::create_dir_all(&project_agents).expect("project agents dir should exist");

        std::fs::write(
            builtin_agents.join("implementer.md"),
            "# Builtin Implementer\nShip the builtin path.",
        )
        .expect("builtin agent should write");
        std::fs::write(
            project_agents.join("implementer.md"),
            "# Project Implementer\nShip the project path.",
        )
        .expect("project agent should write");

        let agents =
            ExtensionRegistry::load_agents_for_workspace(&root).expect("agents should load");
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].name, "implementer");
        assert!(agents[0].instructions.contains("project path"));

        let _ = std::fs::remove_dir_all(root);
    }
}
