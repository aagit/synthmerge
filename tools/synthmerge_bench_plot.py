#!/usr/bin/env python3
#
# SPDX-License-Identifier: GPL-3.0-or-later OR AGPL-3.0-or-later
# Copyright (C) 2026  Red Hat, Inc.

import matplotlib.pyplot as plt
import re

# 1. Data Definition
# Format: (Display Name, Accuracy %, Accuracy Stripped %)
# Note: Percentages are stored as floats (e.g., 69.09)
data = [
    ("Claude Opus 4.6", 69.09, 76.17),
    ("Gemini 3.1 Pro (reasoning low)", 68.73, 75.29),
    ("Patchpal AI 7B (beam #0)", 67.1, 73.6),
    ("Claude Sonnet 4.0", 66.70, 73.34),
    ("Claude Sonnet 4.5", 65.10, 73.16),
    ("Qwen3.5-27B", 63.86, 71.48),
    ("Gemini 3 Flash (reasoning none)", 64.13, 73.43),
    ("Claude Sonnet 4.6", 60.67, 66.61),
    ("Devstral-Small-2-24B", 57.22, 67.32),
    ("Qwen3.5-35B-A3B", 56.86, 66.34),
    ("Gemini 2.5 Pro (reasoning high)", 55.18, 63.42),
    ("Qwen3-Coder-Next", 53.32, 61.74),
    ("Qwen3.5-9B", 53.06, 59.96),
    ("Gemini 2.5 Pro (reasoning low, userctx)", 52.44, 59.70),
    ("Qwen3-Coder-30B-A3B", 49.69, 56.78),
    ("Gemini 2.5 Flash (reasoning none, default context)", 49.60, 63.42),
    ("Gemini 2.5 Flash (reasoning low, user context)", 42.52, 55.98),
    # Secondary Beams
    ("Patchpal AI 7B (beam #1)", 10.54, 30.03),
    ("Gemini 2.5 Flash (reasoning low, default context)", 7.97, 10.27),
    ("Qwen3-Coder-30B (beam #1)", 7.71, 16.56),
    ("Patchpal AI 7B (beam #2)", 3.37, 23.83),
    ("Qwen3-Coder-30B (beam #2)", 1.95, 11.87),
]

# 2. Preprocessing: Sort by Accuracy (Stripped) descending
data.sort(key=lambda x: x[1], reverse=True)

# Extract lists for plotting
models = [d[0] for d in data]
accuracy = [d[1] for d in data]
accuracy_stripped = [d[2] for d in data]

# 3. Setup Plot
plt.style.use("seaborn-v0_8-dark")
fig, ax = plt.subplots(figsize=(16, 9))

# Define positions for grouped bars
x = range(len(models))
bar_width = 0.3

# Plot bars
bars1 = ax.bar(
    [i - bar_width / 2 for i in x],
    accuracy,
    width=bar_width,
    label="Accuracy",
    color="#3498db",
    alpha=0.9,
    edgecolor="black",
)
bars2 = ax.bar(
    [i + bar_width / 2 for i in x],
    accuracy_stripped,
    width=bar_width,
    label="Accuracy (Stripped)",
    color="#2ecc71",
    alpha=0.9,
    edgecolor="black",
)

# 4. Customization
ax.set_xlabel("Model", fontsize=12, fontweight="bold")
ax.set_ylabel("Percentage (%)", fontsize=12, fontweight="bold")
ax.set_title(
    'Model Performance: "Accuracy" vs. "Accuracy (Stripped)"\nSorted by "Accuracy"',
    fontsize=14,
    fontweight="bold",
    pad=20,
)
ax.set_xticks(x)
ax.set_xticklabels(models, rotation=90, fontsize=9)
ax.legend(loc="upper right", fontsize=11)
ax.set_ylim(0, 100)


# Add value labels on top of bars (optional, keeps it clean for many bars)
def add_labels(bars):
    for bar in bars:
        height = bar.get_height()
        ax.text(
            bar.get_x() + bar.get_width() / 3.0,
            height + 0.5,
            f"{height:.1f}",
            ha="center",
            va="bottom",
            fontsize=8,
            color="black",
        )


add_labels(bars1)
add_labels(bars2)

# Adjust layout to prevent label cutoff
plt.tight_layout()

# 5. Display
plt.show()
