// SPDX-License-Identifier: GPL-3.0-or-later OR AGPL-3.0-or-later
// Copyright (C) 2025  Red Hat, Inc.

#[derive(Parser, Debug)]
#[command(version)]
struct Args {
    /// Path to configuration file
    #[arg(
        short = 'c',
        long = "config",
	default_value = concat!("~/.config/", env!("CARGO_PKG_NAME"), ".yaml")
    )]
    config_path: String,

    /// Number of context lines to include around conflict markers
    #[arg(long = "code-context-lines", default_value = "3", value_parser = clap::value_parser!(u32).range(0..))]
    code_context_lines: u32,

    /// Number of context lines of the git_diff provided as context
    #[arg(long = "diff-context-lines", default_value = "3", value_parser = clap::value_parser!(u32).range(0..))]
    diff_context_lines: u32,

    /// Number of context lines of the patch
    #[arg(long = "patch-context-lines", default_value = "3", value_parser = clap::value_parser!(u32).range(0..))]
    patch_context_lines: u32,

    /// Automatically resolve conflicts and update the git index.
    #[arg(long = "vibe", default_value = "false")]
    vibe: bool,

    /// Continue the current cherry-pick, rebase, revert, or merge operation after resolving conflicts
    #[arg(long = "continue", requires = "vibe", default_value = "false")]
    continue_op: bool,
}

// Local Variables:
// rust-format-on-save: t
// End:
