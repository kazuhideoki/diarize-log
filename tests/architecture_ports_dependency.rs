//! `ports` 層から `adapters` 層への依存を禁止するアーキテクチャ検査です。
//!
//! このテストは `src/ports/**/*.rs` を走査し、`crate::adapters::...` など
//! `adapters` モジュールを直接参照するパターンが含まれていないことを確認します。
//! 該当パターンを含む行が見つかった場合は、ファイルパスと行番号を添えて失敗します。

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const PORTS_DIRECTORY: &str = "src/ports";
const FORBIDDEN_PATTERNS: [&str; 4] = [
    "crate::adapters::",
    "crate::{adapters::",
    "crate::{ adapters::",
    "super::super::adapters::",
];

/// `ports` 配下のコードが `adapters` に依存していないことを保証する。
#[test]
fn ports_layer_does_not_depend_on_adapters_layer() {
    assert_ports_do_not_depend_on_adapters().unwrap();
}

fn assert_ports_do_not_depend_on_adapters() -> Result<(), PortsAdapterDependencyError> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let ports_dir = manifest_dir.join(PORTS_DIRECTORY);
    let mut rust_files = Vec::new();
    collect_rust_files(&ports_dir, &mut rust_files)?;

    let violations = rust_files
        .iter()
        .map(|path| find_forbidden_dependencies(manifest_dir, path))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();

    if violations.is_empty() {
        Ok(())
    } else {
        Err(PortsAdapterDependencyError::ForbiddenDependency { violations })
    }
}

#[derive(Debug)]
enum PortsAdapterDependencyError {
    ReadDirectory {
        path: PathBuf,
        source: io::Error,
    },
    ReadFile {
        path: PathBuf,
        source: io::Error,
    },
    ForbiddenDependency {
        violations: Vec<DependencyViolation>,
    },
}

impl fmt::Display for PortsAdapterDependencyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReadDirectory { path, source } => {
                write!(f, "failed to read directory {}: {source}", path.display())
            }
            Self::ReadFile { path, source } => {
                write!(f, "failed to read file {}: {source}", path.display())
            }
            Self::ForbiddenDependency { violations } => {
                let messages = violations
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("; ");
                write!(
                    f,
                    "ports layer must not depend on adapters layer: {messages}"
                )
            }
        }
    }
}

impl std::error::Error for PortsAdapterDependencyError {}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DependencyViolation {
    path: PathBuf,
    line_number: usize,
    line: String,
}

impl fmt::Display for DependencyViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{} contains forbidden adapters dependency: {}",
            self.path.display(),
            self.line_number,
            self.line.trim()
        )
    }
}

fn collect_rust_files(
    directory: &Path,
    files: &mut Vec<PathBuf>,
) -> Result<(), PortsAdapterDependencyError> {
    let entries =
        fs::read_dir(directory).map_err(|source| PortsAdapterDependencyError::ReadDirectory {
            path: directory.to_path_buf(),
            source,
        })?;

    for entry in entries {
        let entry = entry.map_err(|source| PortsAdapterDependencyError::ReadDirectory {
            path: directory.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        let file_type =
            entry
                .file_type()
                .map_err(|source| PortsAdapterDependencyError::ReadDirectory {
                    path: path.clone(),
                    source,
                })?;

        if file_type.is_dir() {
            collect_rust_files(&path, files)?;
            continue;
        }

        if file_type.is_file() && path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }

    Ok(())
}

fn find_forbidden_dependencies(
    manifest_dir: &Path,
    path: &Path,
) -> Result<Vec<DependencyViolation>, PortsAdapterDependencyError> {
    let source =
        fs::read_to_string(path).map_err(|error| PortsAdapterDependencyError::ReadFile {
            path: path.to_path_buf(),
            source: error,
        })?;
    let relative_path = path
        .strip_prefix(manifest_dir)
        .unwrap_or(path)
        .to_path_buf();

    let violations = source
        .lines()
        .enumerate()
        .filter(|(_, line)| {
            FORBIDDEN_PATTERNS
                .iter()
                .any(|pattern| line.contains(pattern))
        })
        .map(|(line_index, line)| DependencyViolation {
            path: relative_path.clone(),
            line_number: line_index + 1,
            line: line.to_owned(),
        })
        .collect();

    Ok(violations)
}
