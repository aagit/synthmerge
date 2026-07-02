// SPDX-License-Identifier: GPL-3.0-or-later OR AGPL-3.0-or-later
// Copyright (C) 2025-2026  Red Hat, Inc.

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

    /// Artificially enlarge the conflict region
    #[arg(long = "extra-conflict-lines", default_value = "0", value_parser = clap::value_parser!(u32).range(0..))]
    extra_conflict_lines: u32,

    /// Maximum size of the diff provided as context in bytes
    #[arg(long = "max-context-size", default_value = "200000", value_parser = clap::value_parser!(u32).range(0..))]
    max_context_size: u32,

    /// Number of retries for conflict resolution (0 means no retries)
    #[arg(long = "retries", default_value = "10", value_parser = clap::value_parser!(u32).range(0..))]
    retries: u32,

    /// Path to LMDB cache file for API responses
    #[arg(long = "cache", default_value = concat!("~/.cache/", env!("CARGO_PKG_NAME"), "_cache"), conflicts_with = "no_cache")]
    cache_path: String,

    /// Disable cache
    #[arg(long = "no-cache")]
    no_cache: bool,

    /// Overwrite mode: disables cache lookup, only writes new entries
    #[arg(
        long = "cache-overwrite",
        default_value = "false",
        conflicts_with = "no_cache"
    )]
    cache_overwrite: bool,

    /// Import an LMDB cache into the current cache database
    #[arg(long = "import-cache", conflicts_with = "no_cache")]
    import_cache: Option<String>,

    /// Automatically resolve conflicts and update the git index.
    #[arg(long = "vibe", default_value = "false")]
    vibe: bool,

    /// Continue the current cherry-pick, rebase, revert, or merge operation after resolving conflicts
    #[arg(long = "continue", requires = "vibe", default_value = "false")]
    continue_op: bool,

    /// Use conflict markers instead of patch locator for vibe resolution.
    /// This restricts the vibe mode to the capabilities of the interactive mode.
    #[arg(long = "with-markers", default_value = "false")]
    with_markers: bool,
}

// Local Variables:
// rust-format-on-save: t
// End:
