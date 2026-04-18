use std::path::Path;

use crate::{CompatError, ImportedInstruction, load_instruction_file};

const AGENTS_INSTRUCTIONS_FILE: &str = "AGENTS.md";

pub(crate) fn collect_workspace_instructions(
    workspace_root: &Path,
    target: &mut Vec<ImportedInstruction>,
) -> Result<(), CompatError> {
    load_instruction_file(&workspace_root.join(AGENTS_INSTRUCTIONS_FILE), target)?;
    load_instruction_file(
        &workspace_root
            .join(".hermes")
            .join(AGENTS_INSTRUCTIONS_FILE),
        target,
    )?;
    Ok(())
}
