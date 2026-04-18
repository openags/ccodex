use std::fs;
use std::path::Path;

use ccodex_protocol::{ExtensionKind, ExtensionManifest};

use crate::paths::{ExtensionRoots, ordered_root_bases, plugin_dir_name};
use crate::util::read_dir_if_exists;
use crate::{ExtensionError, ExtensionRegistry, MarkdownExtension};

pub(crate) fn scan_markdown_layers(
    registry: &mut ExtensionRegistry,
    roots: &ExtensionRoots,
    dir_name: &str,
    kind: ExtensionKind,
) -> Result<(), ExtensionError> {
    for base in ordered_root_bases(roots) {
        crate::plugins::scan_markdown_dir(registry, &base.join(dir_name), kind.clone())?;
    }
    Ok(())
}

pub(crate) fn load_markdown_layers(
    roots: &ExtensionRoots,
    dir_name: &str,
    kind: ExtensionKind,
) -> Result<Vec<MarkdownExtension>, ExtensionError> {
    let mut items = Vec::new();
    for base in ordered_root_bases(roots) {
        merge_markdown_extensions(
            &mut items,
            load_markdown_extensions(&base.join(dir_name), kind.clone())?,
        );
        merge_markdown_extensions(
            &mut items,
            load_markdown_extensions_from_plugin_dirs(
                &base.join(plugin_dir_name()),
                dir_name,
                kind.clone(),
            )?,
        );
    }
    Ok(items)
}

pub(crate) fn load_markdown_extensions(
    dir: &Path,
    kind: ExtensionKind,
) -> Result<Vec<MarkdownExtension>, ExtensionError> {
    let mut loaded = Vec::new();

    for entry in read_dir_if_exists(dir)? {
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|ext| ext.to_str()) != Some("md") {
            continue;
        }

        let raw = fs::read_to_string(&path).map_err(|err| ExtensionError::InspectSource {
            path: path.display().to_string(),
            message: err.to_string(),
        })?;
        let content = normalize_markdown_prompt(&raw);

        loaded.push(MarkdownExtension {
            manifest: ExtensionManifest {
                name: path
                    .file_stem()
                    .and_then(|name| name.to_str())
                    .unwrap_or("unknown")
                    .to_string(),
                kind: kind.clone(),
                version: None,
                source_path: path,
                description: first_heading_or_line_from_content(&content),
            },
            content,
        });
    }

    Ok(loaded)
}

pub(crate) fn load_markdown_extensions_from_plugin_dirs(
    plugin_root: &Path,
    subdir: &str,
    kind: ExtensionKind,
) -> Result<Vec<MarkdownExtension>, ExtensionError> {
    let mut loaded = Vec::new();
    for entry in read_dir_if_exists(plugin_root)? {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        merge_markdown_extensions(
            &mut loaded,
            load_markdown_extensions(&path.join(subdir), kind.clone())?,
        );
    }
    Ok(loaded)
}

pub(crate) fn first_heading_or_line(path: &Path) -> Result<Option<String>, ExtensionError> {
    let content = fs::read_to_string(path).map_err(|err| ExtensionError::InspectSource {
        path: path.display().to_string(),
        message: err.to_string(),
    })?;

    Ok(first_heading_or_line_from_content(&content))
}

pub(crate) fn first_heading_or_line_from_content(content: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(heading) = trimmed.strip_prefix("# ") {
            return Some(heading.trim().to_string());
        }
        return Some(trimmed.to_string());
    }

    None
}

pub(crate) fn normalize_markdown_prompt(content: &str) -> String {
    let trimmed = content.trim();
    if !trimmed.starts_with("---") {
        return trimmed.to_string();
    }

    let mut lines = trimmed.lines();
    if lines.next() != Some("---") {
        return trimmed.to_string();
    }

    let mut body = Vec::new();
    let mut in_frontmatter = true;
    for line in lines {
        if in_frontmatter && line.trim() == "---" {
            in_frontmatter = false;
            continue;
        }
        if !in_frontmatter {
            body.push(line);
        }
    }

    let body = body.join("\n").trim().to_string();
    if body.is_empty() {
        trimmed.to_string()
    } else {
        body
    }
}

fn merge_markdown_extensions(target: &mut Vec<MarkdownExtension>, items: Vec<MarkdownExtension>) {
    for item in items {
        if let Some(existing) = target.iter_mut().find(|extension| {
            extension.manifest.kind == item.manifest.kind
                && extension.manifest.name == item.manifest.name
        }) {
            *existing = item;
        } else {
            target.push(item);
        }
    }
}
