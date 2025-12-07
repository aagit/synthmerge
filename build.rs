// SPDX-License-Identifier: GPL-3.0-or-later OR AGPL-3.0-or-later
// Copyright (C) 2025  Red Hat, Inc.

use std::env;

include!("src/bench_args.rs");
include!("src/main_args.rs");

fn main() -> std::io::Result<()> {
    if env::var("PROFILE").unwrap_or_default() == "release" {
        let out_dir = std::path::PathBuf::from(
            std::env::var_os("OUT_DIR").ok_or(std::io::ErrorKind::NotFound)?,
        )
        .parent()
        .ok_or(std::io::ErrorKind::NotFound)?
        .join("man");
        std::fs::create_dir_all(&out_dir)?;

        use clap::CommandFactory;
        for cmd in [(Args::command(), ""), (BenchArgs::command(), "_bench")] {
            use clap_mangen::Man;
            let man = Man::new(cmd.0);
            let mut buffer: Vec<u8> = Default::default();
            man.render(&mut buffer)?;

            std::fs::write(
                out_dir.join([env!("CARGO_PKG_NAME"), cmd.1, ".1"].join("")),
                buffer,
            )?;
        }
    }

    Ok(())
}

// Local Variables:
// rust-format-on-save: t
// End:
