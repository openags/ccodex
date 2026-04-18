use std::path::Path;

use crate::{CompatError, ImportedInstruction, load_instruction_file, load_markdown_dir};

const CODEX_INSTRUCTIONS_FILE: &str = "CODEX.md";
const AGENTS_INSTRUCTIONS_FILE: &str = "AGENTS.md";

pub(crate) fn collect_workspace_instructions(
    workspace_root: &Path,
    target: &mut Vec<ImportedInstruction>,
) -> Result<(), CompatError> {
    load_instruction_file(&workspace_root.join(CODEX_INSTRUCTIONS_FILE), target)?;
    load_instruction_file(
        &workspace_root.join(".codex").join(CODEX_INSTRUCTIONS_FILE),
        target,
    )?;
    load_instruction_file(
        &workspace_root.join(".codex").join(AGENTS_INSTRUCTIONS_FILE),
        target,
    )?;
    load_markdown_dir(&workspace_root.join(".codex").join("instructions"), target)?;
    Ok(())
}
