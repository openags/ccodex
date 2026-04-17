use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};

use ccodex_brand::{project_exports_dir, project_state_db_file, DISPLAY_NAME};
use ccodex_extensions::ExtensionRegistry;
use ccodex_kernel::Kernel;
use ccodex_protocol::SessionId;
use ccodex_runtime::Runtime;
use ccodex_store::{
    JsonlTranscriptExporter, ListSessionsParams, MarkdownTranscriptExporter, SessionStore,
    SQLiteSessionStore, TranscriptExporter,
};

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ExportFormat {
    Jsonl,
    Markdown,
}

#[derive(Debug, Subcommand)]
enum SessionsCommand {
    List,
}

#[derive(Debug, Subcommand)]
enum ExtensionsCommand {
    List,
}

#[derive(Debug, Subcommand)]
enum Command {
    Run {
        prompt: String,
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
    Export {
        session_id: String,
        #[arg(long, value_enum, default_value_t = ExportFormat::Markdown)]
        format: ExportFormat,
        #[arg(long)]
        output: Option<PathBuf>,
    },
}

#[derive(Debug, Parser)]
#[command(name = ccodex_brand::BINARY_NAME, version = ccodex_brand::VERSION)]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,
    /// Shorthand for `run <prompt>`.
    prompt: Option<String>,
}

fn open_store() -> Result<(Arc<SQLiteSessionStore>, PathBuf)> {
    let workspace_root = std::env::current_dir()?;
    let db_path = project_state_db_file(&workspace_root);
    Ok((Arc::new(SQLiteSessionStore::new(&db_path)?), workspace_root))
}

fn build_kernel(store: Arc<SQLiteSessionStore>) -> Kernel {
    let runtime = Runtime::bootstrap();
    Kernel::new(
        store,
        runtime.provider(),
        runtime.notifications(),
        runtime.tool_executor(),
        runtime.approval_engine(),
        runtime.tool_specs(),
    )
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
            let kernel = build_kernel(store.clone());
            let result = kernel.run_prompt(prompt, Some(workspace_root)).await?;
            println!("{}", result.assistant_text);
            println!("session_id={}", result.session.id);
        }
        Command::Resume { session_id, prompt } => {
            let kernel = build_kernel(store.clone());
            let session_id = SessionId(session_id);
            let result = kernel.resume_prompt(&session_id, prompt).await?;
            println!("{}", result.assistant_text);
            println!("session_id={}", result.session.id);
        }
        Command::Sessions {
            command: SessionsCommand::List,
        } => {
            let sessions = store.list_sessions(ListSessionsParams { limit: Some(20) }).await?;
            for session in sessions {
                println!(
                    "{}\t{}\t{}",
                    session.id,
                    session.title.unwrap_or_else(|| "Untitled".to_string()),
                    session.updated_at
                );
            }
        }
        Command::Extensions {
            command: ExtensionsCommand::List,
        } => {
            let registry = ExtensionRegistry::discover_for_workspace(&workspace_root)?;
            for manifest in registry.manifests() {
                println!(
                    "{:?}\t{}\t{}",
                    manifest.kind,
                    manifest.name,
                    manifest.source_path.display()
                );
            }
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
    }

    Ok(())
}
