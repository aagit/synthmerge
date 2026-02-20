#!/usr/bin/env python3
#
# SPDX-License-Identifier: GPL-3.0-or-later OR AGPL-3.0-or-later
# Copyright (C) 2026  Red Hat, Inc.

"""
synthmerge-stable-test.py - Test synthmerge on stable kernel trees

This script automates testing of synthmerge on stable kernel trees by:
1. Parsing git log output to find commits with conflicts
2. For each conflict commit, extracting the commit hash and upstream commit hash
3. Testing synthmerge by:
   - Checking out the parent of the conflict commit
   - Cherry-picking the upstream commit
   - Running synthmerge to resolve conflicts
   - Verifying the resolution matches the original commit
4. Collecting statistics on successful conflict resolutions

The script is designed to test synthmerge's ability to resolve conflicts
that occur during stable kernel maintenance, where patches from upstream
need to be applied to stable branches.
"""

import re
import subprocess
import sys
from typing import List, Tuple, Optional


def parse_git_log(git_log_output: str) -> List[Tuple[str, str]]:
    """
    Parse git log output to find commits with conflicts and extract:
    - commit_hash: The hash of the conflict commit
    - upstream_commit_hash: The hash of the upstream commit being cherry-picked

    Returns a list of tuples (commit_hash, upstream_commit_hash)
    """
    commit_pattern = re.compile(r"^commit ([a-f0-9]{40})$")
    upstream_patterns = (
        re.compile(r"^commit ([a-f0-9]{40}) upstream.$"),
        re.compile(r"^\[ Upstream commit ([a-f0-9]{40}) \]$"),
    )

    commits = []
    current_commit = None
    upstream_commit = None

    for line in git_log_output.split("\n"):
        line = line.strip()

        # Check for commit hash (starts with commit)
        if commit_pattern.search(line):
            if current_commit and upstream_commit:
                commits.append((current_commit, upstream_commit))
            current_commit = line.split()[1]
            upstream_commit = None
            continue

        # Check for upstream commit info
        if upstream_commit is None:
            for upstream_pattern in upstream_patterns:
                m = upstream_pattern.search(line)
                if m:
                    upstream_commit = m.group(1)
                    break

    # Add the last commit if it exists
    if upstream_commit and upstream_pattern:
        commits.append((current_commit, upstream_commit))

    return commits


def run_command(
    command: str, check: bool = True, capture: bool = True
) -> Optional[str]:
    """
    Run a shell command and return its output.

    Args:
        command: The command to run
        check: Whether to raise an exception if the command fails
        expect_failure: Whether to expect the command to fail (when check=True)

    Returns:
        The command output as a string, or None if check=False and command fails
    """
    try:
        result = subprocess.run(
            command, shell=True, check=check, text=True, capture_output=capture
        )
        return result.stdout
    except subprocess.CalledProcessError:
        if check:
            raise
        return None


def test_synthmerge(
    commit_hash: str, upstream_commit_hash: str, config_file: str
) -> int:
    """
    Test synthmerge on a specific conflict scenario.

    Args:
        commit_hash: The hash of the conflict commit
        upstream_commit_hash: The hash of the upstream commit being cherry-picked
        config_file: Path to synthmerge config file

    Returns:
        1 if synthmerge successfully resolved the conflict,
        0 if there were no conflicts to resolve,
        -1 if synthmerge failed to resolve the conflict
    """
    print(f"\nTesting commit {commit_hash} with upstream {upstream_commit_hash}")

    try:
        # Checkout the parent of the conflict commit
        run_command(f"git checkout -f {commit_hash}~")

        # Cherry-pick the upstream commit
        run_command(f"git cherry-pick -x {upstream_commit_hash}", check=False)

        # Check if there are any files in UU state (both deleted)
        status_output = run_command("git status --short")
        if not status_output or "UU " not in status_output:
            print(f"✗ No conflicts found for {commit_hash} - skipping")
            return 0

        # Run synthmerge
        run_command(f"synthmerge -c {config_file}", capture=False)

        # Open conflicts in editor
        uu_files = [
            line.split()[1]
            for line in status_output.split("\n")
            if line.startswith("UU")
        ]
        if uu_files:
            run_command(f"emacsclient {uu_files[0]}", check=False, capture=False)

        # Verify the resolution matches the original commit
        diff_output = run_command(f"git diff {commit_hash}")
        success = diff_output.strip() == ""

        if success:
            print(f"✓ Successfully resolved conflict for {commit_hash}")
            return 1
        else:
            print(f"✗ Failed to resolve conflict for {commit_hash}")
            print("Difference from original commit:")
            print(diff_output)
            return -1

    except subprocess.CalledProcessError as e:
        print(f"✗ Error testing commit {commit_hash}: {e}")
        return -1


def main():
    if len(sys.argv) < 2:
        print("Usage: python synthmerge-stable-test.py <config-file>")
        print(
            "Example: git log stable/linux-5.15.y -100 | python backtest_stable.py synthmerge-all.yaml"
        )
        sys.exit(1)

    git_log_output = sys.stdin.read()
    config_file = sys.argv[1]

    # Parse git log to find conflict commits
    commits = parse_git_log(git_log_output)

    if not commits:
        print("No conflict commits found in git log output")
        return

    print(f"Found {len(commits)} conflict commits to test")
    print("=" * 60)

    # Test each conflict commit
    success_count = 0
    no_conflict_count = 0
    total_count = len(commits)

    for commit_hash, upstream_commit_hash in commits:
        if upstream_commit_hash is None:
            print(f"Skipping {commit_hash} - no upstream commit found")
            continue

        result = test_synthmerge(commit_hash, upstream_commit_hash, config_file)
        if result == 1:  # Only count as success if True
            success_count += 1
        elif result == 0:  # Count as no conflict
            no_conflict_count += 1

    # Print statistics
    print("\n" + "=" * 60)
    print("Test Results:")
    print(f"Total tests: {total_count}")
    print(f"No conflicts found: {no_conflict_count}")
    print(f"Successful resolutions: {success_count}")
    print(f"Failed resolutions: {total_count - success_count - no_conflict_count}")
    print(
        f"Success rate: {success_count / (total_count - no_conflict_count) * 100:.1f}%"
        if (total_count - no_conflict_count) > 0
        else "N/A"
    )


if __name__ == "__main__":
    main()
