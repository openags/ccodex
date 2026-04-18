use std::path::Path;

use crate::{CompatError, ImportedInstruction, load_instruction_file, load_markdown_dir};

const CLAUDE_INSTRUCTIONS_FILE: &str = "CLAUDE.md";
const AGENTS_INSTRUCTIONS_FILE: &str = "AGENTS.md";

pub(crate) fn collect_workspace_instructions(
    workspace_root: &Path,
    target: &mut Vec<ImportedInstruction>,
) -> Result<(), CompatError> {
    load_instruction_file(&workspace_root.join(CLAUDE_INSTRUCTIONS_FILE), target)?;
    load_instruction_file(
        &workspace_root
            .join(".claude")
            .join(CLAUDE_INSTRUCTIONS_FILE),
        target,
    )?;
    load_instruction_file(
        &workspace_root
            .join(".claude")
            .join(AGENTS_INSTRUCTIONS_FILE),
        target,
    )?;
    load_markdown_dir(&workspace_root.join(".claude").join("instructions"), target)?;
    Ok(())
}
