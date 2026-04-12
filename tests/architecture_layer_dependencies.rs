//! レイヤー間の依存方向を検査する簡易アーキテクチャテストです。
//!
//! このテストは `src/domain/**/*.rs`、`src/ports/**/*.rs`、`src/application/**/*.rs`
//! を走査し、それぞれが禁止された外側レイヤーへ直接依存していないことを確認します。
//! 該当パターンを含む行が見つかった場合は、ファイルパスと行番号を添えて失敗します。

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const DOMAIN_DIRECTORY: &str = "src/domain";
const PORTS_DIRECTORY: &str = "src/ports";
const APPLICATION_DIRECTORY: &str = "src/application";
const DOMAIN_FORBIDDEN_MODULES: [&str; 5] = ["adapters", "application", "cli", "config", "ports"];
const PORTS_FORBIDDEN_MODULES: [&str; 4] = ["adapters", "application", "cli", "config"];
const APPLICATION_FORBIDDEN_MODULES: [&str; 3] = ["adapters", "cli", "config"];
const CRATE_PATH_PREFIX: &str = "crate::";
const PARENT_PATH_PREFIX: &str = "super::super::";

const LAYER_RULES: [LayerDependencyRule<'_>; 3] = [
    LayerDependencyRule {
        layer_name: "domain",
        directory: DOMAIN_DIRECTORY,
        forbidden_modules: &DOMAIN_FORBIDDEN_MODULES,
    },
    LayerDependencyRule {
        layer_name: "ports",
        directory: PORTS_DIRECTORY,
        forbidden_modules: &PORTS_FORBIDDEN_MODULES,
    },
    LayerDependencyRule {
        layer_name: "application",
        directory: APPLICATION_DIRECTORY,
        forbidden_modules: &APPLICATION_FORBIDDEN_MODULES,
    },
];

/// `domain` 配下のコードが外側レイヤーに依存していないことを保証する。
#[test]
fn domain_layer_depends_only_on_domain_layer() {
    assert_layer_has_no_forbidden_dependencies(&LAYER_RULES[0]).unwrap();
}

/// `ports` 配下のコードが上位レイヤーに依存していないことを保証する。
#[test]
fn ports_layer_depends_only_on_ports_layer() {
    assert_layer_has_no_forbidden_dependencies(&LAYER_RULES[1]).unwrap();
}

/// `application` 配下のコードが `domain` と `ports` 以外の外側レイヤーへ依存していないことを保証する。
#[test]
fn application_layer_depends_only_on_domain_and_ports_layers() {
    assert_layer_has_no_forbidden_dependencies(&LAYER_RULES[2]).unwrap();
}

fn assert_layer_has_no_forbidden_dependencies(
    rule: &LayerDependencyRule<'_>,
) -> Result<(), LayerDependencyError> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let target_dir = manifest_dir.join(rule.directory);
    let mut rust_files = Vec::new();
    collect_rust_files(&target_dir, &mut rust_files)?;

    let violations = rust_files
        .iter()
        .map(|path| find_forbidden_dependencies(manifest_dir, path, rule))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();

    if violations.is_empty() {
        Ok(())
    } else {
        Err(LayerDependencyError::ForbiddenDependency {
            layer_name: rule.layer_name,
            violations,
        })
    }
}

#[derive(Debug, Clone, Copy)]
struct LayerDependencyRule<'a> {
    layer_name: &'static str,
    directory: &'static str,
    forbidden_modules: &'a [&'a str],
}

#[derive(Debug)]
enum LayerDependencyError {
    ReadDirectory {
        path: PathBuf,
        source: io::Error,
    },
    ReadFile {
        path: PathBuf,
        source: io::Error,
    },
    ForbiddenDependency {
        layer_name: &'static str,
        violations: Vec<DependencyViolation>,
    },
}

impl fmt::Display for LayerDependencyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReadDirectory { path, source } => {
                write!(f, "failed to read directory {}: {source}", path.display())
            }
            Self::ReadFile { path, source } => {
                write!(f, "failed to read file {}: {source}", path.display())
            }
            Self::ForbiddenDependency {
                layer_name,
                violations,
            } => {
                let messages = violations
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("; ");
                write!(
                    f,
                    "{layer_name} layer has forbidden dependencies: {messages}"
                )
            }
        }
    }
}

impl std::error::Error for LayerDependencyError {}

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
            "{}:{} contains forbidden layer dependency: {}",
            self.path.display(),
            self.line_number,
            self.line.trim()
        )
    }
}

fn collect_rust_files(
    directory: &Path,
    files: &mut Vec<PathBuf>,
) -> Result<(), LayerDependencyError> {
    let entries =
        fs::read_dir(directory).map_err(|source| LayerDependencyError::ReadDirectory {
            path: directory.to_path_buf(),
            source,
        })?;

    for entry in entries {
        let entry = entry.map_err(|source| LayerDependencyError::ReadDirectory {
            path: directory.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        let file_type =
            entry
                .file_type()
                .map_err(|source| LayerDependencyError::ReadDirectory {
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
    rule: &LayerDependencyRule<'_>,
) -> Result<Vec<DependencyViolation>, LayerDependencyError> {
    let source = fs::read_to_string(path).map_err(|error| LayerDependencyError::ReadFile {
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
        .filter(|(_, line)| contains_forbidden_dependency(line, rule.forbidden_modules))
        .map(|(line_index, line)| DependencyViolation {
            path: relative_path.clone(),
            line_number: line_index + 1,
            line: line.to_owned(),
        })
        .collect();

    Ok(violations)
}

fn contains_forbidden_dependency(line: &str, forbidden_modules: &[&str]) -> bool {
    let normalized_line = line.replace(' ', "");

    forbidden_modules.iter().any(|module| {
        contains_forbidden_module_path(&normalized_line, CRATE_PATH_PREFIX, module)
            || contains_forbidden_grouped_crate_import(&normalized_line, module)
            || contains_forbidden_module_path(&normalized_line, PARENT_PATH_PREFIX, module)
    })
}

fn contains_forbidden_module_path(line: &str, prefix: &str, module: &str) -> bool {
    let mut offset = 0;
    let needle = format!("{prefix}{module}");

    while let Some(index) = line[offset..].find(&needle) {
        let matched_index = offset + index;
        let boundary_index = matched_index + needle.len();
        let next_char = line[boundary_index..].chars().next();

        if matches!(next_char, Some(':' | ',' | '}' | ';')) {
            return true;
        }

        offset = boundary_index;
    }

    false
}

fn contains_forbidden_grouped_crate_import(line: &str, module: &str) -> bool {
    let mut search_start = 0;

    while let Some(relative_index) = line[search_start..].find("crate::{") {
        let content_start = search_start + relative_index + "crate::{".len();

        if let Some(content_end) = find_grouped_import_end(&line[content_start..]) {
            let grouped_content = &line[content_start..content_start + content_end];

            if grouped_content
                .split_top_level(',')
                .into_iter()
                .any(|segment| segment_starts_with_module(segment, module))
            {
                return true;
            }

            search_start = content_start + content_end;
        } else {
            return false;
        }
    }

    false
}

fn find_grouped_import_end(line: &str) -> Option<usize> {
    let mut nested_group_depth = 0;

    for (index, character) in line.char_indices() {
        match character {
            '{' => nested_group_depth += 1,
            '}' if nested_group_depth == 0 => return Some(index),
            '}' => nested_group_depth -= 1,
            _ => {}
        }
    }

    None
}

fn segment_starts_with_module(segment: &str, module: &str) -> bool {
    segment.strip_prefix(module).is_some_and(|suffix| {
        suffix.is_empty() || matches!(suffix.chars().next(), Some(':' | ',' | '}'))
    })
}

trait SplitTopLevel {
    fn split_top_level(&self, separator: char) -> Vec<&str>;
}

impl SplitTopLevel for str {
    fn split_top_level(&self, separator: char) -> Vec<&str> {
        let mut nested_group_depth = 0;
        let mut segment_start = 0;
        let mut segments = Vec::new();

        for (index, character) in self.char_indices() {
            match character {
                '{' => nested_group_depth += 1,
                '}' => nested_group_depth -= 1,
                _ if character == separator && nested_group_depth == 0 => {
                    segments.push(&self[segment_start..index]);
                    segment_start = index + character.len_utf8();
                }
                _ => {}
            }
        }

        segments.push(&self[segment_start..]);
        segments
    }
}

#[cfg(test)]
mod tests {
    use super::{
        APPLICATION_FORBIDDEN_MODULES, PORTS_FORBIDDEN_MODULES, contains_forbidden_dependency,
    };

    #[test]
    /// `application` 層の grouped import でも禁止依存を検出する。
    fn detects_grouped_imports_for_application_layer() {
        assert!(contains_forbidden_dependency(
            "use crate::{ ports::Recorder, adapters::CpalRecorder };",
            &APPLICATION_FORBIDDEN_MODULES,
        ));
    }

    #[test]
    /// `ports` 層の自己参照は違反として扱わない。
    fn ignores_allowed_ports_self_reference() {
        assert!(!contains_forbidden_dependency(
            "use crate::ports::Recorder;",
            &PORTS_FORBIDDEN_MODULES,
        ));
    }

    #[test]
    /// `use crate::cli;` のような末尾モジュール参照も禁止依存として検出する。
    fn detects_plain_module_import_without_nested_item() {
        assert!(contains_forbidden_dependency(
            "use crate::cli;",
            &APPLICATION_FORBIDDEN_MODULES,
        ));
    }
}
