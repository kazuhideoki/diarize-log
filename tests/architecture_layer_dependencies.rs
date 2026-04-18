//! レイヤー間の依存方向を検査する簡易アーキテクチャテストです。
//!
//! このテストは `src/domain/**/*.rs`、`src/application/ports/**/*.rs`、
//! `src/application/usecase/**/*.rs` に加えて、
//! `src/bootstrap/**/*.rs`、`src/cli.rs`、`src/lib.rs`、`src/main.rs`
//! も走査し、それぞれが禁止された外側レイヤーや binary 固有都合へ
//! 直接依存していないことを確認します。
//! 該当パターンを含む行が見つかった場合は、ファイルパスと行番号を添えて失敗します。

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const DOMAIN_DIRECTORY: &str = "src/domain";
const APPLICATION_PORTS_DIRECTORY: &str = "src/application/ports";
const APPLICATION_USECASE_DIRECTORY: &str = "src/application/usecase";
const BOOTSTRAP_DIRECTORY: &str = "src/bootstrap";
const CLI_FILE: &str = "src/cli.rs";
const LIB_FILE: &str = "src/lib.rs";
const MAIN_FILE: &str = "src/main.rs";
const DOMAIN_FORBIDDEN_PATHS: [&str; 8] = [
    "crate::adapters",
    "crate::application",
    "crate::cli",
    "crate::config",
    "super::super::adapters",
    "super::super::application",
    "super::super::cli",
    "super::super::config",
];
const PORTS_FORBIDDEN_PATHS: [&str; 8] = [
    "crate::adapters",
    "crate::application::usecase",
    "crate::cli",
    "crate::config",
    "super::super::usecase",
    "super::super::super::adapters",
    "super::super::super::cli",
    "super::super::super::config",
];
const APPLICATION_FORBIDDEN_PATHS: [&str; 6] = [
    "crate::adapters",
    "crate::cli",
    "crate::config",
    "super::super::super::adapters",
    "super::super::super::cli",
    "super::super::super::config",
];
const BOOTSTRAP_FORBIDDEN_PATHS: [&str; 10] = [
    "crate::adapters",
    "crate::application",
    "crate::cli",
    "crate::config",
    "crate::domain",
    "super::super::adapters",
    "super::super::application",
    "super::super::cli",
    "super::super::config",
    "super::super::domain",
];
const CLI_FORBIDDEN_PATHS: [&str; 10] = [
    "crate::adapters",
    "crate::application",
    "crate::config",
    "crate::domain",
    "crate::bootstrap",
    "super::adapters",
    "super::application",
    "super::config",
    "super::domain",
    "super::bootstrap",
];
const LIB_FORBIDDEN_TEXTS: [&str; 4] = [
    "mod bootstrap;",
    "pub mod bootstrap;",
    "mod main;",
    "pub mod main;",
];
const MAIN_FORBIDDEN_TEXTS: [&str; 9] = [
    "mod adapters;",
    "mod application;",
    "mod cli;",
    "mod config;",
    "mod domain;",
    "use diarize_log::",
    "diarize_log::",
    "crate::",
    "super::",
];

const LAYER_RULES: [LayerDependencyRule<'_>; 7] = [
    LayerDependencyRule {
        layer_name: "domain",
        target_path: DOMAIN_DIRECTORY,
        forbidden_paths: &DOMAIN_FORBIDDEN_PATHS,
        forbidden_texts: &[],
    },
    LayerDependencyRule {
        layer_name: "ports",
        target_path: APPLICATION_PORTS_DIRECTORY,
        forbidden_paths: &PORTS_FORBIDDEN_PATHS,
        forbidden_texts: &[],
    },
    LayerDependencyRule {
        layer_name: "application",
        target_path: APPLICATION_USECASE_DIRECTORY,
        forbidden_paths: &APPLICATION_FORBIDDEN_PATHS,
        forbidden_texts: &[],
    },
    LayerDependencyRule {
        layer_name: "bootstrap",
        target_path: BOOTSTRAP_DIRECTORY,
        forbidden_paths: &BOOTSTRAP_FORBIDDEN_PATHS,
        forbidden_texts: &[],
    },
    LayerDependencyRule {
        layer_name: "cli",
        target_path: CLI_FILE,
        forbidden_paths: &CLI_FORBIDDEN_PATHS,
        forbidden_texts: &[],
    },
    LayerDependencyRule {
        layer_name: "lib",
        target_path: LIB_FILE,
        forbidden_paths: &[],
        forbidden_texts: &LIB_FORBIDDEN_TEXTS,
    },
    LayerDependencyRule {
        layer_name: "main",
        target_path: MAIN_FILE,
        forbidden_paths: &[],
        forbidden_texts: &MAIN_FORBIDDEN_TEXTS,
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

/// `bootstrap` 配下のコードが binary crate の外側都合に閉じ、library の内部モジュールを直接生やさないことを保証する。
#[test]
fn bootstrap_layer_depends_only_on_bootstrap_and_library_crate_boundary() {
    assert_layer_has_no_forbidden_dependencies(&LAYER_RULES[3]).unwrap();
}

/// `cli.rs` が内側レイヤーへ直接依存せず CLI 入力境界に閉じることを保証する。
#[test]
fn cli_file_does_not_depend_on_application_or_other_inner_layers() {
    assert_layer_has_no_forbidden_dependencies(&LAYER_RULES[4]).unwrap();
}

/// `lib.rs` が binary 側の `bootstrap` や `main` を取り込まないことを保証する。
#[test]
fn lib_root_does_not_depend_on_binary_entrypoints() {
    assert_layer_has_no_forbidden_dependencies(&LAYER_RULES[5]).unwrap();
}

/// `main.rs` が thin entrypoint として `bootstrap` 以外へ直接依存しないことを保証する。
#[test]
fn main_file_depends_only_on_bootstrap() {
    assert_layer_has_no_forbidden_dependencies(&LAYER_RULES[6]).unwrap();
}

fn assert_layer_has_no_forbidden_dependencies(
    rule: &LayerDependencyRule<'_>,
) -> Result<(), LayerDependencyError> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut rust_files = Vec::new();
    collect_target_rust_files(&manifest_dir.join(rule.target_path), &mut rust_files)?;

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
    target_path: &'static str,
    forbidden_paths: &'a [&'a str],
    forbidden_texts: &'a [&'a str],
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

fn collect_target_rust_files(
    target: &Path,
    files: &mut Vec<PathBuf>,
) -> Result<(), LayerDependencyError> {
    let metadata = fs::metadata(target).map_err(|source| LayerDependencyError::ReadDirectory {
        path: target.to_path_buf(),
        source,
    })?;

    if metadata.is_file() {
        if target
            .extension()
            .is_some_and(|extension| extension == "rs")
        {
            files.push(target.to_path_buf());
        }
        return Ok(());
    }

    collect_rust_files_in_directory(target, files)
}

fn collect_rust_files_in_directory(
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
            collect_rust_files_in_directory(&path, files)?;
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
        .filter(|(_, line)| {
            contains_forbidden_dependency(line, rule.forbidden_paths, rule.forbidden_texts)
        })
        .map(|(line_index, line)| DependencyViolation {
            path: relative_path.clone(),
            line_number: line_index + 1,
            line: line.to_owned(),
        })
        .collect();

    Ok(violations)
}

fn contains_forbidden_dependency(
    line: &str,
    forbidden_paths: &[&str],
    forbidden_texts: &[&str],
) -> bool {
    let normalized_line = line.replace(' ', "");

    forbidden_paths.iter().any(|forbidden_path| {
        contains_forbidden_path(&normalized_line, forbidden_path)
            || contains_forbidden_grouped_import(&normalized_line, forbidden_path)
    }) || forbidden_texts
        .iter()
        .any(|forbidden_text| normalized_line.contains(&forbidden_text.replace(' ', "")))
}

fn contains_forbidden_path(line: &str, forbidden_path: &str) -> bool {
    let mut offset = 0;
    let needle = forbidden_path;

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

fn contains_forbidden_grouped_import(line: &str, forbidden_path: &str) -> bool {
    let Some((root_path, grouped_path)) = forbidden_path.rsplit_once("::") else {
        return false;
    };
    let grouped_import_prefix = format!("{root_path}::{{");
    let mut search_start = 0;

    while let Some(relative_index) = line[search_start..].find(&grouped_import_prefix) {
        let content_start = search_start + relative_index + grouped_import_prefix.len();

        if let Some(content_end) = find_grouped_import_end(&line[content_start..]) {
            let grouped_content = &line[content_start..content_start + content_end];

            if grouped_content
                .split_top_level(',')
                .into_iter()
                .any(|segment| segment_starts_with_path(segment, grouped_path))
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

fn segment_starts_with_path(segment: &str, path: &str) -> bool {
    segment.strip_prefix(path).is_some_and(|suffix| {
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
        APPLICATION_FORBIDDEN_PATHS, MAIN_FORBIDDEN_TEXTS, PORTS_FORBIDDEN_PATHS,
        contains_forbidden_dependency,
    };

    #[test]
    /// `application` 層の grouped import でも禁止依存を検出する。
    fn detects_grouped_imports_for_application_layer() {
        assert!(contains_forbidden_dependency(
            "use crate::{ application::ports::Recorder, adapters::CpalRecorder };",
            &APPLICATION_FORBIDDEN_PATHS,
            &[],
        ));
    }

    #[test]
    /// `ports` 層の自己参照は違反として扱わない。
    fn ignores_allowed_ports_self_reference() {
        assert!(!contains_forbidden_dependency(
            "use crate::application::ports::Recorder;",
            &PORTS_FORBIDDEN_PATHS,
            &[],
        ));
    }

    #[test]
    /// `use crate::cli;` のような末尾モジュール参照も禁止依存として検出する。
    fn detects_plain_module_import_without_nested_item() {
        assert!(contains_forbidden_dependency(
            "use crate::cli;",
            &APPLICATION_FORBIDDEN_PATHS,
            &[],
        ));
    }

    #[test]
    /// 禁止文字列ルールでも thin entrypoint 違反を検出する。
    fn detects_forbidden_text_dependency() {
        assert!(contains_forbidden_dependency(
            "use diarize_log::parse_cli_args;",
            &[],
            &MAIN_FORBIDDEN_TEXTS,
        ));
    }
}
