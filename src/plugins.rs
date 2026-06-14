// Copyright (c) 2026 Kan Murata
// This software is released under the MIT License, see LICENSE.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, bail, Context, Result};

use crate::config::plugin_dir;

pub fn install_plugin(path: &Path) -> Result<String> {
    if !path.is_file() {
        bail!("plugin file does not exist: {}", path.display());
    }

    let name = path
        .file_name()
        .ok_or_else(|| anyhow!("plugin path has no filename: {}", path.display()))?;
    let directory = plugin_dir();
    fs::create_dir_all(&directory)
        .with_context(|| format!("failed to create {}", directory.display()))?;
    let target = directory.join(name);
    fs::copy(path, &target)
        .with_context(|| format!("failed to copy plugin to {}", target.display()))?;

    Ok(format!("installed {}", target.display()))
}

pub fn remove_plugin(name: &str) -> Result<String> {
    let path = resolve_plugin(name)?;
    fs::remove_file(&path).with_context(|| format!("failed to remove {}", path.display()))?;
    Ok(format!("removed {}", path.display()))
}

pub fn list_plugins() -> Result<String> {
    let directory = plugin_dir();
    if !directory.exists() {
        return Ok("no plugins installed".to_owned());
    }

    let mut names = Vec::new();
    for entry in fs::read_dir(&directory)
        .with_context(|| format!("failed to read {}", directory.display()))?
    {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            names.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    names.sort();

    if names.is_empty() {
        Ok("no plugins installed".to_owned())
    } else {
        Ok(names.join("\n"))
    }
}

pub fn run_plugin(name: &str, args: &[String], image_path: &Path) -> Result<String> {
    let path = resolve_plugin(name)?;
    if path.extension().and_then(|value| value.to_str()) == Some("wasm") {
        bail!(
            "Wasm plugin runtime is not implemented yet: {}",
            path.display()
        );
    }

    let output = Command::new(&path)
        .args(args)
        .env("VIXI_IMAGE", image_path)
        .output()
        .with_context(|| format!("failed to run plugin {}", path.display()))?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();

    if !output.status.success() {
        let detail = if stderr.is_empty() {
            format!("exit status {}", output.status)
        } else {
            stderr
        };
        bail!("plugin failed: {detail}");
    }

    if stdout.is_empty() {
        Ok(format!("plugin {name} completed"))
    } else {
        Ok(stdout)
    }
}

pub fn installed_plugin_names() -> Vec<String> {
    let Ok(entries) = fs::read_dir(plugin_dir()) else {
        return Vec::new();
    };

    let mut names = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            entry
                .file_type()
                .ok()
                .filter(|kind| kind.is_file())
                .map(|_| entry)
        })
        .map(|entry| plugin_command_name(entry.file_name()))
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    names
}

fn resolve_plugin(name: &str) -> Result<PathBuf> {
    let directory = plugin_dir();
    let candidates = [
        directory.join(name),
        directory.join(format!("{name}.wasm")),
        directory.join(format!("{name}.sh")),
        directory.join(format!("{name}.exe")),
    ];

    candidates
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| anyhow!("plugin not installed: {name}"))
}

fn plugin_command_name(name: OsString) -> String {
    let path = PathBuf::from(name);
    path.file_stem()
        .and_then(|value| value.to_str())
        .map(str::to_owned)
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_plugin_extension_for_command_name() {
        assert_eq!(plugin_command_name("denoise.wasm".into()), "denoise");
        assert_eq!(plugin_command_name("measure".into()), "measure");
    }
}
