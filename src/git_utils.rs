// SPDX-License-Identifier: GPL-3.0-or-later OR AGPL-3.0-or-later
// Copyright (C) 2025  Red Hat, Inc.

use crate::conflict_resolver::{Conflict, ResolvedConflict};
use crate::prob;
use anyhow::{Context, Result};
use regex::Regex;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone)]
pub struct ContextLines {
    pub code_context_lines: u32,
    pub diff_context_lines: u32,
    pub patch_context_lines: u32,
}

// Wrapper around Command to allow inheritance-like behavior
pub struct GitCommand {
    command: Command,
}

/// Remove conflict markers from content
#[derive(Debug, Clone, Copy, PartialEq)]
enum ConflictMarkerMode {
    Local,
    Base,
    Remote,
}

impl GitCommand {
    pub fn new(program: &str) -> Self {
        let cmd = Command::new(program);
        GitCommand { command: cmd }
    }

    pub fn args<I, S>(&mut self, args: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        self.command.args(args);
        self
    }

    pub fn output(&mut self) -> Result<std::process::Output> {
        let program = self.command.get_program().to_string_lossy().into_owned();
        let args: Vec<_> = self.command.get_args().collect();
        let args_str = args
            .iter()
            .map(|arg| arg.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ");
        let output = self.command.output().context("Failed to execute command")?;
        log::debug!("GitCommand: {program} {args_str} {{{}}}", output.status);
        if !output.status.success() {
            log::debug!("stdout: {}", String::from_utf8_lossy(&output.stdout));
            log::debug!("stderr: {}", String::from_utf8_lossy(&output.stderr));
        }
        Ok(output)
    }
}

pub struct GitUtils {
    context_lines: ContextLines,
    in_rebase: bool,
    git_root: Option<String>,
    git_dir: Option<String>,
}

impl GitUtils {
    const ASSISTED_BY_LINE: &str = concat!("Assisted-by: ", env!("CARGO_PKG_NAME"));
    const REBASE_MESSAGE_FILE: &str = "rebase-merge/message";
    const MERGE_MSG_FILE: &str = "MERGE_MSG";

    const DEFAULT_MARKER_SIZE: usize = 7;

    pub fn new(context_lines: ContextLines, init_git: bool) -> Self {
        let git_root = if init_git {
            Self::get_git_root_uncached().ok()
        } else {
            None
        };
        let git_dir = if init_git {
            Self::get_git_dir_uncached().ok()
        } else {
            None
        };
        GitUtils {
            context_lines,
            in_rebase: false,
            git_root,
            git_dir,
        }
    }

    /// Check that git cherry-pick default is diff3 for merge.conflictStyle
    pub fn check_diff3(&self) -> Result<()> {
        let output = GitCommand::new("git")
            .args(["config", "--get", "merge.conflictStyle"])
            .output()
            .context("Failed to get git config")?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "Failed to get merge.conflictStyle: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        let config_value = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if config_value != "diff3" {
            return Err(anyhow::anyhow!(
                "merge.conflictStyle is not set to 'diff3', it is set to '{}'",
                config_value
            ));
        }

        Ok(())
    }

    /// Find all conflict markers in the repository
    pub fn find_conflicts(&self) -> Result<Vec<Conflict>> {
        let mut conflicts = Vec::new();

        // Find all files that might contain conflicts
        let output = GitCommand::new("git")
            .args(["diff", "--name-only", "--diff-filter=U"])
            .output()
            .context("Failed to execute git diff")?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "Git diff failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        let diff_output = String::from_utf8_lossy(&output.stdout);
        for line in diff_output.lines() {
            let file_path = line.trim();
            let conflict = self.parse_conflict_from_file(file_path)?;
            conflicts.extend(conflict);
        }

        Ok(conflicts)
    }

    /// Parse conflicts from a specific file
    fn parse_conflict_from_file(&self, file_path: &str) -> Result<Vec<Conflict>> {
        let path = Path::new(self.git_root.as_ref().unwrap()).join(file_path);
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read file: {}", file_path))?;

        // Get the marker size for this file from gitattributes
        let marker_size = self.get_marker_size_for_file(file_path)?;

        let mut conflicts = Vec::new();
        let re = Regex::new(&format!(
            r"(?ms)(^{} .*?^{} .*?^{}\n.*?^{}.*?\n)",
            Self::create_local_marker(marker_size),
            Self::create_base_marker(marker_size)
                .chars()
                .map(|c| format!(r"\{}", c))
                .collect::<String>(),
            Self::create_remote_marker(marker_size),
            Self::create_end_marker(marker_size),
        ))
        .unwrap();

        for cap in re.captures_iter(&content) {
            let this_cap = cap.get(0).unwrap();
            let conflict_text = this_cap.as_str();
            let start_line = content[..this_cap.start()]
                .chars()
                .filter(|&c| c == '\n')
                .count()
                + 1;
            let conflict = self.parse_conflict_text(
                conflict_text,
                &content,
                start_line,
                file_path,
                marker_size,
            )?;
            conflicts.push(conflict);
        }

        Ok(conflicts)
    }

    fn gen_context(
        &self,
        conflict_lines: &[&str],
        content_lines: &[&str],
        start_line: usize,
        marker_size: usize,
        mode: ConflictMarkerMode,
    ) -> Result<(usize, usize, String, String)> {
        let head_context_end = (start_line.saturating_sub(1)).max(0);
        let head_content_lines = content_lines[..start_line].to_vec();

        const CONTEXT_BEYOND_MARKER: bool = false;
        let head_content_lines = if CONTEXT_BEYOND_MARKER {
            Self::remove_conflict_markers(
                head_content_lines[..head_context_end].to_vec(),
                marker_size,
                mode,
            )
        } else {
            Ok(head_content_lines[..head_context_end]
                .iter()
                .rev()
                .take_while(|&&x| !x.starts_with(&Self::create_end_marker(marker_size)))
                .cloned()
                .collect::<Vec<_>>()
                .iter()
                .rev()
                .cloned()
                .collect::<Vec<_>>())
        }?;
        let head_context_lines = head_content_lines[head_content_lines
            .len()
            .saturating_sub(self.context_lines.code_context_lines as usize)
            .max(0)..]
            .to_vec();
        let nr_head_context_lines = head_context_lines.len();

        let tail_content_lines = content_lines[start_line + conflict_lines.len() - 1..].to_vec();
        let tail_content_lines = if CONTEXT_BEYOND_MARKER {
            Self::remove_conflict_markers(tail_content_lines, marker_size, mode)
        } else {
            Ok(tail_content_lines
                .iter()
                .take_while(|&&x| {
                    !x.starts_with(&format!("{} ", Self::create_local_marker(marker_size)))
                })
                .cloned()
                .collect::<Vec<_>>())
        }?;
        let tail_context_lines = tail_content_lines[..tail_content_lines
            .len()
            .min(self.context_lines.code_context_lines as usize)]
            .to_vec();
        let nr_tail_context_lines = tail_context_lines.len();

        Ok((
            nr_head_context_lines,
            nr_tail_context_lines,
            head_context_lines.join(""),
            tail_context_lines.join(""),
        ))
    }

    /// Parse a conflict block into structured data
    fn parse_conflict_text(
        &self,
        conflict_text: &str,
        content: &str,
        start_line: usize,
        file_path: &str,
        marker_size: usize,
    ) -> Result<Conflict> {
        let conflict_lines: Vec<&str> = conflict_text.split_inclusive('\n').collect();

        let local_start = conflict_lines
            .iter()
            .position(|&line| {
                line.starts_with(&format!("{} ", Self::create_local_marker(marker_size)))
            })
            .context("Failed to find head marker")?;

        let base_start = conflict_lines
            .iter()
            .position(|&line| {
                line.starts_with(&format!("{} ", Self::create_base_marker(marker_size)))
            })
            .context("Failed to find base marker")?;

        let remote_start = conflict_lines
            .iter()
            .position(|&line| line == format!("{}\n", Self::create_remote_marker(marker_size)))
            .context("Failed to find conflict marker")?;

        let remote_end = conflict_lines
            .iter()
            .position(|&line| line.starts_with(&Self::create_end_marker(marker_size)))
            .context("Failed to find conflict end marker")?;

        let ai_start = conflict_lines
            .iter()
            .position(|&line| {
                line.starts_with(&format!("{} ", Self::create_ai_marker(marker_size)))
            })
            .unwrap_or(remote_end);

        if remote_end < ai_start
            || remote_end <= remote_start
            || remote_start <= base_start
            || base_start <= local_start
        {
            return Err(anyhow::anyhow!(
                "Invalid conflict markers: ai_start={}, remote_end={}, remote_start={}, base_start={}, local_start={}",
                ai_start,
                remote_end,
                remote_start,
                base_start,
                local_start
            ));
        }

        let local_lines: Vec<&str> = conflict_lines[local_start + 1..base_start].to_vec();
        let base_lines: Vec<&str> = conflict_lines[base_start + 1..remote_start].to_vec();
        let remote_lines: Vec<&str> = conflict_lines[remote_start + 1..ai_start].to_vec();

        let content_lines: Vec<&str> = content.split_inclusive('\n').collect();

        let (nr_head_context_lines, nr_tail_context_lines, head_context, tail_context) = self
            .gen_context(
                &conflict_lines,
                &content_lines,
                start_line,
                marker_size,
                ConflictMarkerMode::Local,
            )?;

        Ok(Conflict {
            file_path: file_path.to_string(),
            local: local_lines.join(""),
            base: base_lines.join(""),
            remote: remote_lines.join(""),
            head_context,
            tail_context,
            start_line,
            remote_end, // append new AI results at the end
            nr_head_context_lines,
            nr_tail_context_lines,
            marker_size,
        })
    }

    /// Get the marker size for a specific file from gitattributes
    fn get_marker_size_for_file(&self, file_path: &str) -> Result<usize> {
        // Check if we can find the marker size in gitattributes for this file
        let output = GitCommand::new("git")
            .args([
                "-C",
                self.git_root.as_ref().unwrap(),
                "check-attr",
                "conflict-marker-size",
                "--",
                file_path,
            ])
            .output()
            .with_context(|| format!("Failed to execute git check-attr for file: {}", file_path))?;

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if let Some(size_str) = line
                    .strip_prefix(format!("{}:", file_path).as_str())
                    .and_then(|s| s.trim().strip_prefix("conflict-marker-size: "))
                    && let Ok(size) = size_str.parse::<usize>()
                {
                    return Ok(size);
                }
            }
        }

        // Default to 7 if not found
        Ok(Self::DEFAULT_MARKER_SIZE)
    }

    /// Create a marker with specified size
    fn create_marker(marker_char: char, size: usize) -> String {
        marker_char.to_string().repeat(size)
    }

    /// Create a local marker with specified size
    fn create_local_marker(size: usize) -> String {
        Self::create_marker('<', size)
    }

    /// Create a base marker with specified size
    fn create_base_marker(size: usize) -> String {
        Self::create_marker('|', size)
    }

    /// Create a conflict marker with specified size
    fn create_remote_marker(size: usize) -> String {
        Self::create_marker('=', size)
    }

    /// Create a AI marker with specified size
    fn create_ai_marker(size: usize) -> String {
        Self::create_marker('&', size)
    }

    /// Create an end marker with specified size
    fn create_end_marker(size: usize) -> String {
        Self::create_marker('>', size)
    }

    fn remove_conflict_markers(
        content_lines: Vec<&str>,
        marker_size: usize,
        mode: ConflictMarkerMode,
    ) -> Result<Vec<&str>> {
        let mut skip_lines = false;
        let mut in_region = false;
        let result: Vec<&str> = content_lines
            .into_iter()
            .filter(|line| {
                if line.starts_with(&Self::create_local_marker(marker_size)) {
                    in_region = mode == ConflictMarkerMode::Local;
                    skip_lines = true;
                    return false;
                } else if line.starts_with(&Self::create_base_marker(marker_size)) {
                    in_region = mode == ConflictMarkerMode::Base;
                    return false;
                } else if line.starts_with(&Self::create_remote_marker(marker_size)) {
                    in_region = mode == ConflictMarkerMode::Remote;
                    return false;
                } else if line.starts_with(&Self::create_ai_marker(marker_size)) {
                    in_region = false;
                    return false;
                } else if line.starts_with(&Self::create_end_marker(marker_size)) {
                    skip_lines = false;
                    in_region = false;
                    return false;
                }
                !skip_lines || in_region
            })
            .collect();

        // Check for nested conflict markers
        let re = Regex::new(&format!(
            r"^(<|>|=|\||\&){{{},}}",
            Self::DEFAULT_MARKER_SIZE,
        ))
        .unwrap();
        let has_nested_markers = result.iter().any(|line| re.is_match(line));

        if has_nested_markers {
            Err(anyhow::anyhow!("Nested conflict markers found in file"))
        } else {
            Ok(result)
        }
    }

    /// Apply resolved conflicts back to the repository
    pub fn apply_resolved_conflicts(&self, conflicts: &[ResolvedConflict]) -> Result<()> {
        let conflicts = Self::deduplicate_conflicts(conflicts);

        for conflict in conflicts.iter().rev() {
            println!(
                "Applying resolved conflict for: {}:{} - {}",
                conflict.conflict.file_path, conflict.conflict.start_line, conflict.model
            );

            // Read the file
            let path =
                Path::new(self.git_root.as_ref().unwrap()).join(&conflict.conflict.file_path);
            let mut content = fs::read_to_string(&path)
                .with_context(|| format!("Failed to read file: {}", conflict.conflict.file_path))?;
            // Split content into lines
            let mut lines: Vec<String> = content
                .split_inclusive('\n')
                .map(|s| s.to_string())
                .collect();

            // Calculate the line where we want to insert the resolved content
            //print startline and remote start
            let insert_line = conflict.conflict.start_line + conflict.conflict.remote_end - 1;

            // Get the marker size for this file from gitattributes
            let marker_size = conflict.conflict.marker_size;

            // Insert the resolved content with markers
            let marker_raw = format!("{} ", Self::create_ai_marker(marker_size));
            let marker = format!(
                "{}{}: {}{}\n",
                marker_raw,
                env!("CARGO_PKG_NAME"),
                conflict.model,
                conflict
                    .logprob
                    .map(|p| format!(" {:.1}%", prob::logprob_to_prob(p)))
                    .unwrap_or_default(),
            );
            let current_line = &lines[insert_line];
            if !current_line.starts_with(&format!("{} ", Self::create_end_marker(marker_size)))
                && !current_line.starts_with(&marker_raw)
            {
                log::error!(
                    "Invalid conflict marker found at line {}\n{}",
                    insert_line,
                    current_line
                );
                continue;
            }
            lines.insert(insert_line, marker);
            let resolved_lines: Vec<String> = conflict
                .resolved_version
                .lines()
                .map(|s| s.to_string())
                .collect();
            for (i, line) in resolved_lines.iter().enumerate() {
                lines.insert(insert_line + 1 + i, format!("{}\n", line));
            }

            content = lines.join("");

            // Write back to file
            fs::write(&path, content).with_context(|| {
                format!("Failed to write file: {}", conflict.conflict.file_path)
            })?;
        }

        // Add Assisted-by line to merge message
        self.update_merge_message()?;

        Ok(())
    }

    /// Apply vibe resolution - fully resolve conflicts and update git index
    pub fn apply_vibe_resolution(
        &self,
        conflicts: &[Conflict],
        resolved_conflicts: &[ResolvedConflict],
    ) -> Result<()> {
        let resolved_conflicts = Self::deduplicate_conflicts(resolved_conflicts);

        // Group conflicts by file
        let mut conflicts_by_file = std::collections::HashMap::new();
        for conflict in conflicts {
            conflicts_by_file
                .entry(&conflict.file_path)
                .or_insert_with(Vec::new)
                .push(conflict);
        }

        let mut unresolved_files = false;
        // Process each file
        for (file_path, file_conflicts) in &conflicts_by_file {
            println!("Processing file: {}", file_path);

            // Read the file
            let path = Path::new(self.git_root.as_ref().unwrap()).join(file_path);
            let content = fs::read_to_string(&path)
                .with_context(|| format!("Failed to read file: {}", file_path))?;

            // Split content into lines
            let mut lines: Vec<String> = content
                .split_inclusive('\n')
                .map(|s| s.to_string())
                .collect();

            // Sort conflicts by start line (ascending)
            let mut sorted_conflicts: Vec<_> = file_conflicts.iter().collect();
            sorted_conflicts.sort_by_key(|c| c.start_line);

            // Process conflicts in reverse order to maintain correct line numbers
            for conflict in sorted_conflicts.iter().rev() {
                let resolved_conflict = resolved_conflicts
                    .iter()
                    .find(|rc| rc.conflict == ***conflict);
                if resolved_conflict.is_none() {
                    unresolved_files = true;
                    continue;
                }
                let conflict = resolved_conflict.unwrap();
                println!(
                    "Applying vibe resolution for: {}:{}",
                    conflict.conflict.file_path, conflict.conflict.start_line
                );

                // Find the conflict markers
                let end_marker = Self::create_end_marker(conflict.conflict.marker_size);

                // Find the start and end of the conflict
                let start_line = conflict.conflict.start_line - 1; // Convert to 0-based index
                let mut end_line = start_line;

                // Find the end marker
                for (i, line) in lines[start_line..].iter().enumerate() {
                    if line.starts_with(&end_marker) {
                        end_line = start_line + i;
                        break;
                    }
                }

                // Replace the entire conflict with the resolved version
                let resolved_lines: Vec<String> = conflict
                    .resolved_version
                    .lines()
                    .map(|s| s.to_string() + "\n")
                    .collect();

                // Replace the conflict
                lines.splice(start_line..=end_line, resolved_lines);
            }

            // Write back to file
            let updated_content = lines.join("");
            fs::write(&path, updated_content)
                .with_context(|| format!("Failed to write file: {}", file_path))?;
        }

        // Add Assisted-by line to merge message
        self.update_merge_message()?;

        // Update git index if all conflicts are resolved
        if !unresolved_files {
            let output = GitCommand::new("git")
                .args(["add", "-u"])
                .output()
                .context(format!("Failed to execute git add -u ."))?;

            if !output.status.success() {
                return Err(anyhow::anyhow!(
                    "Git add -u failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                ));
            }
            println!("Updated git index");
        }

        Ok(())
    }

    fn deduplicate_conflicts(conflicts: &[ResolvedConflict]) -> Vec<ResolvedConflict> {
        use std::collections::HashMap;
        let mut map: HashMap<(String, usize, &str), Vec<&ResolvedConflict>> = HashMap::new();

        // Group conflicts by resolved_version, start_line and file_path
        for conflict in conflicts {
            map.entry((
                conflict.resolved_version.clone(),
                conflict.conflict.start_line,
                &conflict.conflict.file_path,
            ))
            .or_default()
            .push(conflict);
        }

        // For each group, create a new conflict with combined model names
        let mut result = Vec::new();
        for ((resolved_version, _, _), group) in map {
            let model = Self::combine_model_names(group.as_slice());

            // Use the first conflict in the group as the base
            let base_conflict = &group[0].conflict;
            let total_tokens = if group.iter().any(|c| c.total_tokens.is_some()) {
                Some(
                    group.iter().filter_map(|c| c.total_tokens).sum::<u64>()
                        / group.iter().filter_map(|c| c.total_tokens).count() as u64,
                )
            } else {
                None
            };
            let logprob = if group.iter().any(|c| c.logprob.is_some()) {
                Some(
                    group.iter().filter_map(|c| c.logprob).sum::<f64>()
                        / group.iter().filter_map(|c| c.logprob).count() as f64,
                )
            } else {
                None
            };
            result.push(ResolvedConflict {
                conflict: base_conflict.clone(),
                resolved_version,
                model,
                duration: group
                    .iter()
                    .map(|c| c.duration)
                    .max_by(|a, b| a.partial_cmp(b).unwrap())
                    .unwrap_or(0.0),
                total_tokens,
                logprob,
                endpoint: group.iter().map(|c| c.endpoint).min().unwrap(),
                deduplicated_conflicts: group
                    .into_iter()
                    .filter(|x| {
                        assert!(&x.conflict == base_conflict);
                        true
                    })
                    .cloned()
                    .collect(),
            });
        }

        // Sort by number of models that agree on the resolved conflict (descending)
        // When equal, maintain original order
        let mut ordered_result = Vec::new();
        let mut seen = std::collections::HashSet::new();

        // First pass: collect all unique resolved conflicts with their original order positions
        let mut unique_conflicts: Vec<(String, &str, usize, usize, usize)> = Vec::new();
        for original in conflicts {
            let key = (
                &original.resolved_version,
                original.conflict.start_line,
                &original.conflict.file_path,
            );
            if seen.insert(key) {
                let pos = result
                    .iter()
                    .position(|r| {
                        (
                            &r.resolved_version,
                            r.conflict.start_line,
                            &r.conflict.file_path,
                        ) == key
                    })
                    .unwrap();
                let num_models = result[pos].deduplicated_conflicts.len();
                unique_conflicts.push((
                    result[pos].resolved_version.clone(),
                    &result[pos].conflict.file_path,
                    result[pos].conflict.start_line,
                    num_models,
                    result[pos].endpoint,
                ));
            }
        }

        // Sort by file, line, number of models (descending) and
        // finally with the original "endpoint" order
        unique_conflicts.sort_by(|a, b| {
            a.1.cmp(b.1)
                .then(a.2.cmp(&b.2))
                .then(b.3.cmp(&a.3))
                .then(a.4.cmp(&b.4))
        });

        // Build the final ordered result
        for (resolved_version, start_line, file_path, _, _) in unique_conflicts {
            let pos = result
                .iter()
                .position(|r| {
                    (
                        &r.resolved_version,
                        r.conflict.start_line,
                        r.conflict.file_path.as_str(),
                    ) == (&resolved_version, file_path, start_line)
                })
                .unwrap();
            ordered_result.push(result[pos].clone());
        }

        ordered_result
    }

    fn combine_model_names(group: &[&ResolvedConflict]) -> String {
        use std::collections::HashMap;
        let mut suffix_map: HashMap<String, Vec<String>> = HashMap::new();
        let mut prefixes = Vec::new();

        // Group models by their prefix (everything before the last '(')
        for conflict in group {
            let model_name = &conflict.model;
            if let Some(pos) = model_name.rfind('(') {
                let prefix = &model_name[..pos];
                let prefix = prefix.trim();
                let suffix_start = pos + 1;
                if let Some(suffix_end) = model_name[suffix_start..].find(')') {
                    let suffix = &model_name[suffix_start..suffix_start + suffix_end];
                    let entry = suffix_map.entry(prefix.to_string()).or_default();
                    assert!(
                        !entry.contains(&suffix.to_string()),
                        "Duplicate suffix found: {} for prefix {}",
                        suffix,
                        prefix
                    );
                    entry.push(suffix.to_string());
                    prefixes.push(prefix);
                } else {
                    // No closing parenthesis, treat as regular name
                    let entry = suffix_map.entry(model_name.clone()).or_default();
                    assert!(
                        entry.is_empty(),
                        "Duplicate empty suffix found for model: {}",
                        model_name
                    );
                    entry.push("".to_string());
                    prefixes.push(prefix);
                }
            } else {
                // No parentheses, treat as regular name
                let entry = suffix_map.entry(model_name.clone()).or_default();
                assert!(
                    entry.is_empty(),
                    "Duplicate empty suffix found for model: {}",
                    model_name
                );
                entry.push("".to_string());
                prefixes.push(model_name);
            }
        }

        let mut combined_names = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for prefix in prefixes {
            if !seen.insert(prefix) {
                continue;
            };
            let suffixes = suffix_map.get(prefix).unwrap();
            assert!(!suffixes.is_empty());
            assert!(suffixes.iter().filter(|x| x.is_empty()).count() <= 1);
            if suffixes.len() == 1 && suffixes[0].is_empty() {
                combined_names.push(prefix.to_string());
            } else {
                // Combine suffixes into a single string like "(suffix1|suffix2|suffix3)"
                let suffixes_str = suffixes.to_vec().join("|");
                combined_names.push(format!("{} ({})", prefix, suffixes_str));
            }
        }

        combined_names.join(", ")
    }

    /// Get the git root directory
    fn get_git_root_uncached() -> Result<String> {
        let output = GitCommand::new("git")
            .args(["rev-parse", "--show-toplevel"])
            .output()
            .context("Failed to execute git rev-parse")?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "Git rev-parse failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        let git_root = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(git_root)
    }

    /// Get the git directory
    fn get_git_dir_uncached() -> Result<String> {
        let output = GitCommand::new("git")
            .args(["rev-parse", "--git-dir"])
            .output()
            .context("Failed to execute git rev-parse")?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "Git rev-parse failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        let git_dir = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(git_dir)
    }

    /// Update the git merge message to include Assisted-by line
    fn update_merge_message(&self) -> Result<()> {
        let git_dir = self.git_dir.as_ref().unwrap();

        let merge_msg_path = if self.in_rebase {
            Path::new(git_dir).join(Self::REBASE_MESSAGE_FILE)
        } else {
            Path::new(git_dir).join(Self::MERGE_MSG_FILE)
        };
        let merge_msg_content = match fs::read_to_string(&merge_msg_path) {
            Ok(content) => content,
            Err(_) => {
                println!(
                    "If you use the AI generated code please add \"{}\"",
                    Self::ASSISTED_BY_LINE
                );
                return Ok(());
            }
        };

        if merge_msg_content.contains(Self::ASSISTED_BY_LINE) {
            return Ok(());
        }

        let mut lines: Vec<String> = merge_msg_content
            .split_inclusive('\n')
            .map(|s| s.to_string())
            .collect();

        // Find the line before "# Conflicts:" or end of file
        let mut insert_pos = lines.len();
        for (i, line) in lines.iter().enumerate() {
            if line.trim() == "# Conflicts:" {
                insert_pos = i;
                break;
            }
        }

        // Go backwards to find the last non-empty line
        while insert_pos > 0 {
            insert_pos -= 1;
            if !lines[insert_pos].trim().is_empty() {
                break;
            }
        }

        let mut i = insert_pos + 1;
        let mut prefix_newline = "\n";
        let regex = regex::Regex::new(r"^[A-Z][^\s]*-by:\s.*\n$").unwrap();
        while i > 0 {
            i -= 1;
            if regex.is_match(&lines[i]) {
                prefix_newline = "";
                break;
            }
        }

        // Insert the Assisted-by line after the last non-empty line
        let assisted_line = format!("{}{}\n", prefix_newline, Self::ASSISTED_BY_LINE);
        lines.insert(insert_pos + 1, assisted_line);

        let updated_content = lines.join("");
        fs::write(&merge_msg_path, updated_content).with_context(|| {
            format!(
                "Failed to write updated merge message: {}",
                merge_msg_path.display()
            )
        })?;

        println!("Added \"{}\"", Self::ASSISTED_BY_LINE);

        Ok(())
    }

    /// Check if we are currently in a cherry-pick, merge, or rebase state
    pub fn find_commit_hash(&mut self) -> Result<Option<String>> {
        let git_dir = self
            .git_dir
            .as_ref()
            .context("Not running in a git repository")?;

        // Check for cherry-pick, merge, and rebase HEAD files
        let mut head_files = Vec::new();
        for &prefix in &["CHERRY_PICK", "MERGE", "REBASE", "REVERT"] {
            head_files.push((
                prefix,
                Path::new(git_dir).join(format!("{}_{}", prefix, "HEAD")),
            ));
        }

        let mut content: Option<String> = None;
        let mut latest_path: Option<(&str, PathBuf)> = None;
        let mut latest_time = std::time::SystemTime::UNIX_EPOCH;

        for (name, path) in head_files {
            if Path::new(&path).exists() {
                let metadata = std::fs::metadata(&path)
                    .with_context(|| format!("Failed to get metadata for {}", name))?;
                let file_time = metadata
                    .modified()
                    .with_context(|| format!("Failed to get modification time for {}", name))?;

                if file_time > latest_time {
                    latest_time = file_time;
                    latest_path = Some((name, path));
                }
            }
        }

        if let Some((name, path)) = latest_path {
            content = Some(
                std::fs::read_to_string(&path)
                    .with_context(|| format!("Failed to read {}", name))?
                    .trim()
                    .to_string(),
            );
            // Check if it's a rebase
            if name == "REBASE" {
                // Also check if the rebase message file exists
                let rebase_msg_path = Path::new(git_dir).join(Self::REBASE_MESSAGE_FILE);
                if rebase_msg_path.exists() {
                    self.in_rebase = true;
                }
            }
        }

        Ok(content)
    }

    /// Extract the patch from a specific commit hash
    pub fn extract_diff(&self, commit_hash: &str) -> Result<Option<String>> {
        self.git_show_in_dir(commit_hash, None, None)
    }

    /// Extract the patch from a specific commit hash
    pub fn git_show_in_dir(
        &self,
        commit_hash: &str,
        dir: Option<&str>,
        filename: Option<&str>,
    ) -> Result<Option<String>> {
        let diff_context_lines = &format!("-U{}", self.context_lines.diff_context_lines);
        let dir = if let Some(directory) = dir {
            shellexpand::tilde(directory).to_string()
        } else {
            ".".to_string()
        };
        let output = if let Some(file) = filename {
            let filearg = &format!("{}:{}", commit_hash, file);
            let args = vec!["-C", &dir, "show", filearg];
            GitCommand::new("git")
                .args(&args)
                .output()
                .context("Failed to execute git show")?
        } else {
            let args = vec![
                "-C",
                &dir,
                "show",
                "--pretty=",
                "--no-color",
                "--histogram",
                diff_context_lines,
                commit_hash,
            ];
            GitCommand::new("git")
                .args(&args)
                .output()
                .context("Failed to execute git show")?
        };

        if !output.status.success() {
            return Ok(None);
        }

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        if filename.is_some() {
            Ok(Some(stdout))
        } else {
            let lines: Vec<&str> = stdout.split_inclusive('\n').collect();
            let mut result_lines = Vec::new();
            let mut include_line = true;

            for line in lines {
                if line.starts_with("diff --git") {
                    result_lines.push(line);
                    include_line = false;
                } else if line.starts_with("---") {
                    result_lines.push(line);
                    include_line = true;
                } else if include_line {
                    result_lines.push(line);
                }
            }

            Ok(Some(result_lines.join("")))
        }
    }
}

// Local Variables:
// rust-format-on-save: t
// End:
