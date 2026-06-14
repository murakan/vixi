// Copyright (c) 2026 Kan Murata
// This software is released under the MIT License, see LICENSE.

use std::env;
use std::path::PathBuf;

pub fn data_dir() -> PathBuf {
    if let Some(value) = env::var_os("XDG_DATA_HOME") {
        return PathBuf::from(value).join("vixi");
    }
    home_dir()
        .map(|home| home.join(".local").join("share").join("vixi"))
        .unwrap_or_else(|| PathBuf::from(".vixi"))
}

pub fn plugin_dir() -> PathBuf {
    data_dir().join("plugins")
}

fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME").map(PathBuf::from)
}
