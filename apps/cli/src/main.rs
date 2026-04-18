use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use serde::Serialize;

use ccodex_brand::{
    DISPLAY_NAME, project_config_file, project_exports_dir, project_state_db_file, user_config_file,
};
use ccodex_compat::CompatLayer;
use ccodex_extensions::ExtensionRegistry;
use ccodex_kernel::Kernel;
use ccodex_protocol::{McpPort, SessionId};
use ccodex_runtime::{CommandBackedMcpPort, Runtime};
use ccodex_store::{
    JsonlTranscriptExporter, ListSessionsParams, MarkdownTranscriptExporter, SQLiteSessionStore,
    SessionStore, TranscriptExporter,
};

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ExportFormat {
    Jsonl,
    Markdown,
}

#[derive(Debug, Subcommand)]
enum SessionsCommand {
    List,
    Show { session_id: String },
    Fork { session_id: String },
}

#[derive(Debug, Subcommand)]
enum ExtensionsCommand {
    List,
}

#[derive(Debug, Subcommand)]
enum McpCommand {
    List,
    Call {
        server: String,
        tool: String,
        #[arg(default_value = "{}")]
        input: String,
    },
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    Show,
}

#[derive(Debug, Subcommand)]
enum Command {
    Run {
        prompt: String,
    },
    Exec {
        prompt: Option<String>,
        #[arg(long)]
        session_id: Option<String>,
        #[arg(long, default_value_t = true)]
        json: bool,
    },
    Resume {
        session_id: String,
        prompt: String,
    },
    Sessions {
        #[command(subcommand)]
        command: SessionsCommand,
    },
    Extensions {
        #[command(subcommand)]
        command: ExtensionsCommand,
    },
    Mcp {
        #[command(subcommand)]
        command: McpCommand,
    },
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    Export {
        session_id: String,
        #[arg(long, value_enum, default_value_t = ExportFormat::Markdown)]
        format: ExportFormat,
        #[arg(long)]
        output: Option<PathBuf>,
    },
    Doctor,
}

#[derive(Debug, Parser)]
#[command(name = ccodex_brand::BINARY_NAME, version = ccodex_brand::VERSION)]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,
    /// Shorthand for `run <prompt>`.
    prompt: Option<String>,
}

#[derive(Debug, Serialize)]
struct ExecOutput {
    session_id: String,
    turn_id: String,
    assistant_text: String,
    event_count: usize,
}

fn open_store() -> Result<(Arc<SQLiteSessionStore>, PathBuf)> {
    let workspace_root = std::env::current_dir()?;
    let db_path = project_state_db_file(&workspace_root);
    Ok((Arc::new(SQLiteSessionStore::new(&db_path)?), workspace_root))
}

fn build_kernel(
    store: Arc<SQLiteSessionStore>,
    workspace_root: &std::path::Path,
) -> Result<Kernel> {
    let runtime = Runtime::for_workspace(workspace_root.to_path_buf())?;
    Ok(Kernel::new(
        store,
        runtime.provider(),
        runtime.notifications(),
        runtime.tool_executor(),
        runtime.approval_engine(),
        runtime.tool_specs(),
    ))
}

fn read_prompt_from_stdin() -> Result<String> {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    Ok(input.trim().to_string())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let command = match (args.command, args.prompt) {
        (Some(command), _) => command,
        (None, Some(prompt)) => Command::Run { prompt },
        (None, None) => {
            println!("{DISPLAY_NAME}: pass a prompt or use a subcommand");
            return Ok(());
        }
    };

    let (store, workspace_root) = open_store()?;

    match command {
        Command::Run { prompt } => {
            let kernel = build_kernel(store.clone(), &workspace_root)?;
            let result = kernel
                .agent_runtime()
                .start_session(prompt, Some(workspace_root))
                .await?;
            println!("{}", result.assistant_text);
            println!("session_id={}", result.session.id);
        }
        Command::Exec {
            prompt,
            session_id,
            json,
        } => {
            let prompt = match prompt {
                Some(prompt) => prompt,
                None => read_prompt_from_stdin()?,
            };
            if prompt.is_empty() {
                anyhow::bail!("exec requires a prompt argument or stdin input");
            }

            let kernel = build_kernel(store.clone(), &workspace_root)?;
            let runtime = kernel.agent_runtime();
            let result = if let Some(session_id) = session_id {
                runtime
                    .continue_session(&SessionId(session_id), prompt)
                    .await?
            } else {
                runtime.start_session(prompt, Some(workspace_root)).await?
            };

            if json {
                println!(
                    "{}",
                    serde_json::to_string(&ExecOutput {
                        session_id: result.session.id.to_string(),
                        turn_id: result.turn.id.to_string(),
                        assistant_text: result.assistant_text,
                        event_count: result.events.len(),
                    })?
                );
            } else {
                println!("{}", result.assistant_text);
                println!("session_id={}", result.session.id);
                println!("turn_id={}", result.turn.id);
                println!("event_count={}", result.events.len());
            }
        }
        Command::Resume { session_id, prompt } => {
            let kernel = build_kernel(store.clone(), &workspace_root)?;
            let session_id = SessionId(session_id);
            let result = kernel
                .agent_runtime()
                .continue_session(&session_id, prompt)
                .await?;
            println!("{}", result.assistant_text);
            println!("session_id={}", result.session.id);
        }
        Command::Sessions {
            command: SessionsCommand::List,
        } => {
            let sessions = store
                .list_sessions(ListSessionsParams { limit: Some(20) })
                .await?;
            for session in sessions {
                println!(
                    "{}\t{}\t{}",
                    session.id,
                    session.title.unwrap_or_else(|| "Untitled".to_string()),
                    session.updated_at
                );
            }
        }
        Command::Sessions {
            command: SessionsCommand::Show { session_id },
        } => {
            let session_id = SessionId(session_id);
            let turns = store.list_turns(&session_id).await?;
            for stored in turns {
                println!("turn {}", stored.turn.id);
                for item in stored.items {
                    println!("{:?}", item.payload);
                }
                println!();
            }
        }
        Command::Sessions {
            command: SessionsCommand::Fork { session_id },
        } => {
            let kernel = build_kernel(store.clone(), &workspace_root)?;
            let forked = kernel
                .agent_runtime()
                .fork_session(&SessionId(session_id))
                .await?;
            println!("{}", forked.id);
            if let Some(title) = forked.title {
                println!("title={title}");
            }
        }
        Command::Extensions {
            command: ExtensionsCommand::List,
        } => {
            let compat = CompatLayer::new();
            let registry =
                ExtensionRegistry::discover_for_roots(&compat.extension_roots(&workspace_root))?;
            for manifest in registry.manifests() {
                println!(
                    "{:?}\t{}\t{}",
                    manifest.kind,
                    manifest.name,
                    manifest.source_path.display()
                );
            }
        }
        Command::Mcp {
            command: McpCommand::List,
        } => {
            let mcp = CommandBackedMcpPort::new(workspace_root.clone());
            for server in mcp.load_servers()? {
                println!(
                    "{}\t{}\t{}",
                    server.name,
                    server.command,
                    server.args.join(" ")
                );
            }
        }
        Command::Mcp {
            command:
                McpCommand::Call {
                    server,
                    tool,
                    input,
                },
        } => {
            let mcp = CommandBackedMcpPort::new(workspace_root.clone());
            let input: serde_json::Value = serde_json::from_str(&input)?;
            let output = mcp.call_tool(&server, &tool, input).await?;
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
        Command::Config {
            command: ConfigCommand::Show,
        } => {
            let runtime = Runtime::for_workspace(workspace_root.clone())?;
            let config = runtime.config();
            println!("workspace_root={}", workspace_root.display());
            println!("approval_policy={:?}", config.approval_policy);
            println!(
                "approval_allow_tools={}",
                config.approval_rules.allow_tools.join(",")
            );
            println!(
                "approval_deny_tools={}",
                config.approval_rules.deny_tools.join(",")
            );
            println!(
                "approval_allow_commands={}",
                config.approval_rules.allow_commands.join(",")
            );
            println!(
                "approval_deny_commands={}",
                config.approval_rules.deny_commands.join(",")
            );
            println!(
                "approval_allow_paths={}",
                config.approval_rules.allow_paths.join(",")
            );
            println!(
                "approval_deny_paths={}",
                config.approval_rules.deny_paths.join(",")
            );
            println!("sandbox_mode={}", config.sandbox_mode.as_str());
            println!("provider_kind={:?}", config.provider.kind);
            println!("provider_model={}", config.provider.model);
            println!(
                "provider_base_url={}",
                config.provider.base_url.as_deref().unwrap_or("<none>")
            );
            println!(
                "provider_api_key_present={}",
                if config
                    .provider
                    .api_key
                    .as_ref()
                    .map(|s| !s.is_empty())
                    .unwrap_or(false)
                {
                    "true"
                } else {
                    "false"
                }
            );
            println!("max_output_tokens={}", config.provider.max_output_tokens);
        }
        Command::Export {
            session_id,
            format,
            output,
        } => {
            let session_id = SessionId(session_id);
            let payload = match format {
                ExportFormat::Jsonl => {
                    JsonlTranscriptExporter::new((*store).clone())
                        .export_session(&session_id)
                        .await?
                }
                ExportFormat::Markdown => {
                    MarkdownTranscriptExporter::new((*store).clone())
                        .export_session(&session_id)
                        .await?
                }
            };

            if let Some(path) = output {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&path, payload)?;
                println!("{}", path.display());
            } else {
                let exports_dir = project_exports_dir(&workspace_root);
                std::fs::create_dir_all(&exports_dir)?;
                let extension = match format {
                    ExportFormat::Jsonl => "jsonl",
                    ExportFormat::Markdown => "md",
                };
                let path = exports_dir.join(format!("{}.{}", session_id, extension));
                std::fs::write(&path, payload)?;
                println!("{}", path.display());
            }
        }
        Command::Doctor => {
            let runtime = Runtime::for_workspace(workspace_root.clone())?;
            let compat = CompatLayer::new();
            let registry =
                ExtensionRegistry::discover_for_roots(&compat.extension_roots(&workspace_root))?;
            let compat_instructions = compat.load_workspace_instructions(&workspace_root)?;
            let mcp = CommandBackedMcpPort::new(workspace_root.clone());
            let mcp_servers = mcp.load_servers()?;
            let config = runtime.config();
            let user_config = user_config_file();
            let project_config = project_config_file(&workspace_root);
            let mut plugin_count = 0usize;
            let mut skill_count = 0usize;
            let mut agent_count = 0usize;
            let mut hook_count = 0usize;
            for manifest in registry.manifests() {
                match manifest.kind {
                    ccodex_protocol::ExtensionKind::Plugin => plugin_count += 1,
                    ccodex_protocol::ExtensionKind::Skill => skill_count += 1,
                    ccodex_protocol::ExtensionKind::Agent => agent_count += 1,
                    ccodex_protocol::ExtensionKind::Hook => hook_count += 1,
                }
            }

            println!("workspace_root={}", workspace_root.display());
            println!(
                "state_db={}",
                project_state_db_file(&workspace_root).display()
            );
            println!("user_config={}", user_config.display());
            println!(
                "user_config_exists={}",
                if user_config.exists() {
                    "true"
                } else {
                    "false"
                }
            );
            println!("project_config={}", project_config.display());
            println!(
                "project_config_exists={}",
                if project_config.exists() {
                    "true"
                } else {
                    "false"
                }
            );
            println!("approval_policy={:?}", config.approval_policy);
            println!(
                "approval_allow_tool_count={}",
                config.approval_rules.allow_tools.len()
            );
            println!(
                "approval_deny_tool_count={}",
                config.approval_rules.deny_tools.len()
            );
            println!(
                "approval_allow_command_count={}",
                config.approval_rules.allow_commands.len()
            );
            println!(
                "approval_deny_command_count={}",
                config.approval_rules.deny_commands.len()
            );
            println!(
                "approval_allow_path_count={}",
                config.approval_rules.allow_paths.len()
            );
            println!(
                "approval_deny_path_count={}",
                config.approval_rules.deny_paths.len()
            );
            println!("sandbox_mode={}", config.sandbox_mode.as_str());
            println!("provider_kind={:?}", config.provider.kind);
            println!("provider_model={}", config.provider.model);
            println!(
                "provider_base_url={}",
                config.provider.base_url.as_deref().unwrap_or("<none>")
            );
            println!(
                "provider_api_key_present={}",
                if config
                    .provider
                    .api_key
                    .as_ref()
                    .map(|s| !s.is_empty())
                    .unwrap_or(false)
                {
                    "true"
                } else {
                    "false"
                }
            );
            println!("max_output_tokens={}", config.provider.max_output_tokens);
            println!("tool_count={}", runtime.tool_specs().len());
            println!("extension_count={}", registry.manifests().len());
            println!("plugin_count={}", plugin_count);
            println!("skill_count={}", skill_count);
            println!("agent_count={}", agent_count);
            println!("hook_count={}", hook_count);
            println!(
                "compat_instruction_count={}",
                compat_instructions.instructions.len()
            );
            for imported in &compat_instructions.instructions {
                println!("compat_instruction={}", imported.source.display());
            }
            println!("mcp_server_count={}", mcp_servers.len());
            for server in &mcp_servers {
                println!("mcp_server={}\t{}", server.name, server.command);
            }
        }
    }

    Ok(())
}
