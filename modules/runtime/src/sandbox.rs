use std::path::{Component, Path, PathBuf};

use ccodex_protocol::PortError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxMode {
    ReadOnly,
    WorkspaceWrite,
    DangerFullAccess,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileAccess {
    Read,
    Write,
}

#[derive(Debug, Clone)]
pub struct SandboxPolicy {
    workspace_root: PathBuf,
    mode: SandboxMode,
}

impl SandboxMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "read-only" | "readonly" => Some(Self::ReadOnly),
            "workspace-write" | "workspace" => Some(Self::WorkspaceWrite),
            "danger-full-access" | "danger" => Some(Self::DangerFullAccess),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ReadOnly => "read-only",
            Self::WorkspaceWrite => "workspace-write",
            Self::DangerFullAccess => "danger-full-access",
        }
    }
}

impl SandboxPolicy {
    pub fn new(workspace_root: PathBuf, mode: SandboxMode) -> Self {
        Self {
            workspace_root: normalize_path(workspace_root),
            mode,
        }
    }

    pub fn mode(&self) -> &SandboxMode {
        &self.mode
    }

    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    pub fn resolve_path(&self, path: &str, access: FileAccess) -> Result<PathBuf, PortError> {
        if matches!(
            (&self.mode, access),
            (SandboxMode::ReadOnly, FileAccess::Write)
        ) {
            return Err(PortError::Tool(
                "sandbox denied write in read-only mode".to_string(),
            ));
        }

        let candidate = PathBuf::from(path);
        let resolved = if candidate.is_absolute() {
            normalize_path(candidate)
        } else {
            normalize_path(self.workspace_root.join(candidate))
        };

        if !matches!(self.mode, SandboxMode::DangerFullAccess)
            && !resolved.starts_with(&self.workspace_root)
        {
            return Err(PortError::Tool(format!(
                "sandbox denied path outside workspace: {}",
                resolved.display()
            )));
        }

        Ok(resolved)
    }

    pub fn check_command(&self, command: &str) -> Result<(), PortError> {
        let normalized = command.trim().to_ascii_lowercase();

        if matches!(self.mode, SandboxMode::ReadOnly) && command_looks_mutating(&normalized) {
            return Err(PortError::Tool(
                "sandbox denied mutating shell command in read-only mode".to_string(),
            ));
        }

        if matches!(self.mode, SandboxMode::WorkspaceWrite)
            && (command_looks_dangerous(&normalized)
                || command_requests_network(&normalized)
                || command_targets_sensitive_locations(&normalized))
        {
            return Err(PortError::Tool(
                "sandbox denied dangerous shell command in workspace-write mode".to_string(),
            ));
        }

        Ok(())
    }
}

pub fn command_looks_mutating(command: &str) -> bool {
    let command = command.trim().to_ascii_lowercase();
    command_looks_mutating_impl(&command)
}

pub fn command_looks_dangerous(command: &str) -> bool {
    let command = command.trim().to_ascii_lowercase();
    command_looks_dangerous_impl(&command)
}

pub fn command_requests_network(command: &str) -> bool {
    let command = command.trim().to_ascii_lowercase();
    command_requests_network_impl(&command)
}

pub fn command_targets_sensitive_locations(command: &str) -> bool {
    let command = command.trim().to_ascii_lowercase();
    command_targets_sensitive_locations_impl(&command)
}

pub fn command_looks_scripted_execution(command: &str) -> bool {
    let command = command.trim().to_ascii_lowercase();
    command_looks_scripted_execution_impl(&command)
}

fn command_looks_mutating_impl(command: &str) -> bool {
    let patterns = [
        "rm ",
        "mv ",
        "cp ",
        "touch ",
        "mkdir ",
        "rmdir ",
        "chmod ",
        "chown ",
        "sed -i",
        "perl -pi",
        "git add",
        "git commit",
        "git clean",
        "git reset",
        "cargo install",
        "npm install",
        "pip install",
        ">",
        ">>",
    ];

    patterns.iter().any(|pattern| command.contains(pattern))
}

fn command_looks_dangerous_impl(command: &str) -> bool {
    let patterns = [
        "sudo ",
        "rm -rf ",
        "rm -rf /",
        "git reset --hard",
        "git clean -fd",
        "mkfs",
        "dd if=",
        ">/dev/",
        "| sh",
        "| bash",
        "chmod -r 777",
        "chown -r",
        "launchctl",
        "systemctl",
        "diskutil erase",
        "shutdown ",
        "reboot",
        "kill -9 1",
    ];

    patterns.iter().any(|pattern| command.contains(pattern))
}

fn command_requests_network_impl(command: &str) -> bool {
    let patterns = [
        "curl ", "wget ", "ssh ", "scp ", "rsync ", "nc ", "ncat ", "telnet ", "http://",
        "https://", "ftp://",
    ];

    patterns.iter().any(|pattern| command.contains(pattern))
}

fn command_targets_sensitive_locations_impl(command: &str) -> bool {
    let patterns = [
        "/etc/",
        "/usr/",
        "/bin/",
        "/sbin/",
        "/var/",
        "/dev/",
        "/sys/",
        "/proc/",
        "~/.ssh/",
        "~/.config/",
    ];

    patterns.iter().any(|pattern| command.contains(pattern))
}

fn command_looks_scripted_execution_impl(command: &str) -> bool {
    let patterns = ["python -c", "python3 -c", "node -e", "perl -e", "ruby -e"];

    patterns.iter().any(|pattern| command.contains(pattern))
}

fn normalize_path(path: PathBuf) -> PathBuf {
    let mut normalized = PathBuf::new();

    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(part) => normalized.push(part),
        }
    }

    normalized
}

#[cfg(test)]
mod tests {
    use super::{
        FileAccess, SandboxMode, SandboxPolicy, command_looks_scripted_execution,
        command_requests_network, command_targets_sensitive_locations,
    };

    #[test]
    fn workspace_write_denies_paths_escaping_workspace() {
        let root = std::env::temp_dir().join("ccodex-sandbox-workspace");
        let policy = SandboxPolicy::new(root.clone(), SandboxMode::WorkspaceWrite);

        let denied = policy
            .resolve_path("../outside.txt", FileAccess::Read)
            .expect_err("path escape should fail");
        assert!(denied.to_string().contains("outside workspace"));

        let allowed = policy
            .resolve_path("src/main.rs", FileAccess::Read)
            .expect("workspace path should be allowed");
        assert!(allowed.starts_with(root));
    }

    #[test]
    fn read_only_denies_mutating_commands() {
        let root = std::env::temp_dir().join("ccodex-sandbox-readonly");
        let policy = SandboxPolicy::new(root, SandboxMode::ReadOnly);

        let error = policy
            .check_command("touch hello.txt")
            .expect_err("mutating command should fail");
        assert!(error.to_string().contains("read-only"));
    }

    #[test]
    fn workspace_write_denies_dangerous_commands() {
        let root = std::env::temp_dir().join("ccodex-sandbox-danger");
        let policy = SandboxPolicy::new(root, SandboxMode::WorkspaceWrite);

        let error = policy
            .check_command("sudo rm -rf /tmp/example")
            .expect_err("dangerous command should fail");
        assert!(error.to_string().contains("dangerous"));
    }

    #[test]
    fn detects_network_commands() {
        assert!(command_requests_network("curl https://example.com"));
        assert!(command_requests_network("ssh user@example.com"));
        assert!(!command_requests_network("cat Cargo.toml"));
    }

    #[test]
    fn detects_sensitive_locations() {
        assert!(command_targets_sensitive_locations("cat /etc/hosts"));
        assert!(command_targets_sensitive_locations("ls ~/.ssh/config"));
        assert!(!command_targets_sensitive_locations("cat src/main.rs"));
    }

    #[test]
    fn detects_scripted_execution() {
        assert!(command_looks_scripted_execution("python -c 'print(1)'"));
        assert!(command_looks_scripted_execution(
            "node -e \"console.log(1)\""
        ));
        assert!(!command_looks_scripted_execution("python script.py"));
    }
}
