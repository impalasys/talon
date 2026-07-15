// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use std::error::Error;
use std::path::PathBuf;
use std::process::Command;

fn main() -> Result<(), Box<dyn Error>> {
    let repo_root = git_output(["rev-parse", "--show-toplevel"])?;
    let repo_root = PathBuf::from(repo_root.trim());
    let hook = repo_root.join(".githooks").join("pre-push");

    if !hook.is_file() {
        return Err(format!("missing pre-push hook at {}", hook.display()).into());
    }

    make_executable(&hook)?;

    let hooks_path = repo_root.join(".githooks");
    let status = Command::new("git")
        .current_dir(&repo_root)
        .args(["config", "core.hooksPath"])
        .arg(&hooks_path)
        .status()?;

    if !status.success() {
        return Err("failed to set git core.hooksPath".into());
    }

    println!("Installed Git hooks from .githooks");
    Ok(())
}

fn git_output<const N: usize>(args: [&str; N]) -> Result<String, Box<dyn Error>> {
    let output = Command::new("git").args(args).output()?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().into());
    }
    Ok(String::from_utf8(output.stdout)?)
}

#[cfg(unix)]
fn make_executable(path: &std::path::Path) -> Result<(), Box<dyn Error>> {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(permissions.mode() | 0o755);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(not(unix))]
fn make_executable(_path: &std::path::Path) -> Result<(), Box<dyn Error>> {
    Ok(())
}
