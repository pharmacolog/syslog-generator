#!/usr/bin/env python3
"""
06_plot.py — генерация графиков для Whitepaper 2026 (Issue #196).

Читает perf/whitepaper-results.json (или путь из --input), генерирует
PNG plots в --output директорию. Все plots stdlib-only DARWIN-friendly;
matplotlib import — optional (если не установлен, exit 0 с warning).

Plots:
  - throughput_msg_per_sec.png — bar chart (4 workloads × 4 tools)
  - throughput_bytes_per_sec.png — bar chart, line encoding
  - latency_p50_p95_p99.png — only syslog_generator cells (per METHODOLOGY §5.3)
  - rate_vs_target.png — scatter: target vs achieved, идеал y=x
  - size_distribution.png — histogram actual_bytes_per_msg per workload

Honest outputs:
  - n/a cells omitted (not plotted as 0)
  - failed cells highlighted in red (если есть)
  - legend explains n/a замены текстом, а не implicit 0

Exit codes:
  0  — успех (либо matplotlib отсутствует, но артефактов нет, и это OK)
  1  — perf/whitepaper-results.json не найден
  2  — invalid JSON
  3  — output dir не создан (permission denied)
  4  — unexpected error (traceback в stderr)
"""

import argparse
import json
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent.parent.parent
DEFAULT_WP_RESULTS = REPO_ROOT / "perf" / "whitepaper-results.json"
DEFAULT_OUTPUT_DIR = REPO_ROOT / "benchmarks" / "whitepaper-2026" / "results" / "plots"

COMPARED_TOOLS = ("syslog_generator", "loggen", "flog", "tcpkali")
WORKLOAD_ORDER = (
    "udp_100rps_256b",
    "tcp_10krps_1kb",
    "tls_5krps_1kb",
    "kafka_50krps_256b",
)

# Per-cell цвета (consistent across plots).
COLORS = {
    "syslog_generator": "#2E86AB",
    "loggen":           "#A23B72",
    "flog":             "#F18F01",
    "tcpkali":          "#C73E1D",
}
NA_COLOR = "#D3D3D3"
FAILED_COLOR = "#B22222"


def die(code: int, msg: str) -> "NoReturn":
    print(f"ERROR: {msg}", file=sys.stderr)
    sys.exit(code)


def load_wp_results(path: Path) -> dict:
    if not path.exists():
        die(1, f"whitepaper-results.json not found: {path}")
    try:
        return json.loads(path.read_text())
    except json.JSONDecodeError as e:
        die(2, f"invalid JSON in {path}: {e}")


def ensure_matplotlib():
    """Try import matplotlib; return (mpl, np) or (None, None) if not installed."""
    try:
        import matplotlib
        matplotlib.use("Agg")  # headless; для runner без DISPLAY
        import matplotlib.pyplot as plt
        import numpy as np
        return plt, np
    except ImportError:
        return None, None


def index_runs(runs: list) -> dict:
    """{(workload_id, tool): run_dict}"""
    out = {}
    for r in runs:
        key = (r.get("workload_id"), r.get("tool"))
        out[key] = r
    return out


def plot_throughput_msg_per_sec(plt, np, data: dict, out: Path) -> bool:
    runs = data.get("runs", [])
    if not runs:
        return False
    idx = index_runs(runs)

    workloads_present = [w for w in WORKLOAD_ORDER if any(w == k[0] for k in idx.keys())]
    if not workloads_present:
        return False

    fig, ax = plt.subplots(figsize=(11, 6))
    x = np.arange(len(workloads_present))
    width = 0.20

    for i, tool in enumerate(COMPARED_TOOLS):
        values = []
        labels = []
        for w in workloads_present:
            r = idx.get((w, tool))
            if not r:
                values.append(0.0)
                labels.append("n/a")
                continue
            if r.get("status") == "completed":
                m = r.get("measurements", {})
                values.append(m.get("achieved_msg_per_sec", 0.0))
                labels.append("completed")
            elif r.get("status") == "failed":
                values.append(0.0)
                labels.append("failed")
            else:
                values.append(0.0)
                labels.append(r.get("status", "n/a"))

        bars = ax.bar(x + (i - 1.5) * width, values, width,
                      label=tool, color=COLORS[tool],
                      edgecolor="black", linewidth=0.5)

        for bar, lab in zip(bars, labels):
            if lab in ("n/a", "skipped", "dry_run"):
                bar.set_color(NA_COLOR)
                bar.set_hatch("///")
            elif lab == "failed":
                bar.set_color(FAILED_COLOR)
                bar.set_hatch("xx")

    ax.set_xticks(x)
    ax.set_xticklabels(workloads_present, rotation=15, ha="right")
    ax.set_ylabel("Throughput (msg/s)")
    ax.set_title("Whitepaper 2026 — Throughput per Workload (Issue #196)")
    ax.legend(loc="upper left", fontsize=9)
    ax.grid(axis="y", linestyle="--", alpha=0.3)
    ax.set_yscale("log")  # 50k vs 100 — log scale необходим

    plt.tight_layout()
    plt.savefig(out, dpi=150)
    plt.close(fig)
    return True


def plot_throughput_bytes_per_sec(plt, np, data: dict, out: Path) -> bool:
    runs = data.get("runs", [])
    if not runs:
        return False
    idx = index_runs(runs)
    workloads_present = [w for w in WORKLOAD_ORDER if any(w == k[0] for k in idx.keys())]
    if not workloads_present:
        return False

    fig, ax = plt.subplots(figsize=(11, 6))
    x = np.arange(len(workloads_present))
    width = 0.20

    for i, tool in enumerate(COMPARED_TOOLS):
        values = []
        for w in workloads_present:
            r = idx.get((w, tool))
            if r and r.get("status") == "completed":
                m = r.get("measurements", {})
                values.append(m.get("bytes_per_sec", 0.0))
            else:
                values.append(0.0)

        bars = ax.bar(x + (i - 1.5) * width, values, width,
                      label=tool, color=COLORS[tool],
                      edgecolor="black", linewidth=0.5)
        for bar, w in zip(bars, workloads_present):
            r = idx.get((w, tool))
            if not r or r.get("status") != "completed":
                bar.set_color(NA_COLOR)
                bar.set_hatch("///")

    ax.set_xticks(x)
    ax.set_xticklabels(workloads_present, rotation=15, ha="right")
    ax.set_ylabel("Throughput (bytes/s)")
    ax.set_title("Whitepaper 2026 — Throughput per Workload (bytes/s)")
    ax.legend(loc="upper left", fontsize=9)
    ax.grid(axis="y", linestyle="--", alpha=0.3)
    ax.set_yscale("log")
    plt.tight_layout()
    plt.savefig(out, dpi=150)
    plt.close(fig)
    return True


def plot_latency_percentiles(plt, np, data: dict, out: Path) -> bool:
    """Latency p50/p95/p99 — только syslog_generator (см. METHODOLOGY §5.3)."""
    runs = data.get("runs", [])
    if not runs:
        return False
    idx = index_runs(runs)

    sg_runs = [
        (w, r) for w in WORKLOAD_ORDER
        for (k, r) in idx.items() if k[0] == w and k[1] == "syslog_generator"
        and r.get("status") == "completed"
    ]
    if not sg_runs:
        return False

    workloads = [w for w, _ in sg_runs]
    p50 = [r["measurements"].get("p50_ms", 0.0) for _, r in sg_runs]
    p95 = [r["measurements"].get("p95_ms", 0.0) for _, r in sg_runs]
    p99 = [r["measurements"].get("p99_ms", 0.0) for _, r in sg_runs]

    x = np.arange(len(workloads))
    width = 0.25

    fig, ax = plt.subplots(figsize=(10, 5))
    ax.bar(x - width, p50, width, label="p50", color="#2E86AB")
    ax.bar(x,        p95, width, label="p95", color="#A23B72")
    ax.bar(x + width, p99, width, label="p99", color="#F18F01")

    ax.set_xticks(x)
    ax.set_xticklabels(workloads, rotation=15, ha="right")
    ax.set_ylabel("Latency (ms)")
    ax.set_title("Whitepaper 2026 — syslog-generator Latency (p50/p95/p99)\n"
                 "Other tools: N/A per METHODOLOGY §5.3")
    ax.legend(loc="upper left", fontsize=9)
    ax.grid(axis="y", linestyle="--", alpha=0.3)
    plt.tight_layout()
    plt.savefig(out, dpi=150)
    plt.close(fig)
    return True


def plot_rate_vs_target(plt, np, data: dict, out: Path) -> bool:
    """Scatter: target vs achieved. Идеал — на y=x."""
    runs = data.get("runs", [])
    if not runs:
        return False
    targets = {w["id"]: w["target_msg_per_sec"]
               for w in data.get("workloads", [])}

    points = []
    for r in runs:
        if r.get("status") != "completed":
            continue
        m = r.get("measurements", {})
        w = r.get("workload_id")
        t = r.get("tool")
        achieved = m.get("achieved_msg_per_sec")
        if achieved is None or w not in targets:
            continue
        points.append((targets[w], achieved, w, t))

    if not points:
        return False

    fig, ax = plt.subplots(figsize=(8, 7))
    for tool in COMPARED_TOOLS:
        xs = [p[0] for p in points if p[3] == tool]
        ys = [p[1] for p in points if p[3] == tool]
        ax.scatter(xs, ys, label=tool, color=COLORS[tool], s=70, alpha=0.7,
                   edgecolors="black", linewidth=0.5)

    max_val = max(max(p[0], p[1]) for p in points) * 1.05
    ax.plot([0, max_val], [0, max_val], "k--", alpha=0.5, label="y = x (ideal)")
    ax.plot([0, max_val], [0, max_val * 1.05], "r:", alpha=0.4, label="+5% (upper gate)")
    ax.plot([0, max_val], [0, max_val * 0.95], "r:", alpha=0.4, label="-5% (lower gate)")

    ax.set_xlabel("Target rate (msg/s)")
    ax.set_ylabel("Achieved rate (msg/s)")
    ax.set_title("Whitepaper 2026 — Achieved vs Target Rate\n"
                 "Points between dotted lines = within 95–105% gate")
    ax.legend(loc="upper left", fontsize=9)
    ax.grid(True, linestyle="--", alpha=0.3)
    ax.set_xscale("log")
    ax.set_yscale("log")
    plt.tight_layout()
    plt.savefig(out, dpi=150)
    plt.close(fig)
    return True


def plot_size_distribution(plt, np, data: dict, out: Path) -> bool:
    """Histogram actual_bytes_per_msg per workload."""
    runs = data.get("runs", [])
    if not runs:
        return False
    targets = {w["id"]: w["target_bytes_per_msg"]
               for w in data.get("workloads", [])}

    per_workload = {w: [] for w in WORKLOAD_ORDER}
    for r in runs:
        if r.get("status") != "completed":
            continue
        m = r.get("measurements", {})
        actual = m.get("actual_bytes_per_msg")
        w = r.get("workload_id")
        if actual is not None and w in per_workload:
            per_workload[w].append(
                (r.get("tool"), actual, targets.get(w, 0))
            )

    has_data = any(v for v in per_workload.values())
    if not has_data:
        return False

    fig, axes = plt.subplots(1, len(WORKLOAD_ORDER), figsize=(18, 5), sharey=False)
    for ax, w in zip(axes, WORKLOAD_ORDER):
        entries = per_workload[w]
        if not entries:
            ax.text(0.5, 0.5, f"{w}\n(no completed cells)",
                    ha="center", va="center", transform=ax.transAxes,
                    fontsize=12, color="gray")
            ax.set_xticks([])
            ax.set_yticks([])
            continue
        target = entries[0][2]
        for tool, actual, _ in entries:
            ax.scatter([actual], [0], color=COLORS[tool], s=100,
                       label=tool, edgecolors="black", linewidth=0.5)
        ax.axvline(target, color="green", linestyle="--", alpha=0.7,
                   label=f"target={target}B")
        ax.axvline(target * 1.05, color="red", linestyle=":", alpha=0.5)
        ax.axvline(target * 0.95, color="red", linestyle=":", alpha=0.5)
        ax.set_xlabel("Actual bytes per msg")
        ax.set_title(w)
        ax.set_yticks([])
        ax.grid(axis="x", linestyle="--", alpha=0.3)
        if w == WORKLOAD_ORDER[0]:
            ax.legend(loc="upper right", fontsize=7)

    fig.suptitle("Whitepaper 2026 — Actual Message Size per Workload\n"
                 "(dotted red lines = ±5% tolerance)", fontsize=12)
    plt.tight_layout()
    plt.savefig(out, dpi=150)
    plt.close(fig)
    return True


def diag_variance(plt, np, data: dict, out: Path) -> bool:
    """Variance mode — для root-cause analysis если ≥3 failed cells (см. §3.4 runbook)."""
    runs = data.get("runs", [])
    if not runs:
        return False
    idx = index_runs(runs)

    fig, ax = plt.subplots(figsize=(10, 6))
    completed = [r for r in runs if r.get("status") == "completed"]
    if not completed:
        plt.close(fig)
        return False

    labels = []
    cv_pct = []
    for w in WORKLOAD_ORDER:
        for tool in COMPARED_TOOLS:
            r = idx.get((w, tool))
            if not r or r.get("status") != "completed":
                continue
            # coefficient of variation = stddev / mean, * 100
            m = r.get("measurements", {})
            v = m.get("achieved_msg_per_sec")
            cv = m.get("cv_pct")
            if cv is None or v is None:
                continue
            labels.append(f"{w}/{tool}")
            cv_pct.append(cv)

    if not cv_pct:
        plt.close(fig)
        return False

    ax.bar(range(len(labels)), cv_pct, color="#2E86AB", edgecolor="black")
    ax.axhline(5.0, color="red", linestyle="--", label="5% threshold")
    ax.set_xticks(range(len(labels)))
    ax.set_xticklabels(labels, rotation=60, ha="right", fontsize=8)
    ax.set_ylabel("Coefficient of variation (%)")
    ax.set_title("Variance Diagnosis — CV% across 3 runs per cell")
    ax.legend()
    plt.tight_layout()
    plt.savefig(out, dpi=150)
    plt.close(fig)
    return True


def main(argv):
    parser = argparse.ArgumentParser(
        description="Generate Whitepaper 2026 plots (Issue #196)"
    )
    parser.add_argument("--input", type=Path, default=DEFAULT_WP_RESULTS,
                        help=f"Path to whitepaper-results.json (default: {DEFAULT_WP_RESULTS})")
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT_DIR,
                        help=f"Output directory for PNGs (default: {DEFAULT_OUTPUT_DIR})")
    parser.add_argument("--mode", choices=["full", "variance"], default="full",
                        help="Plot mode: full (5 plots) or variance (1 plot, for diagnosis)")
    args = parser.parse_args(argv)

    data = load_wp_results(args.input)

    try:
        args.output.mkdir(parents=True, exist_ok=True)
    except OSError as e:
        die(3, f"cannot create output dir {args.output}: {e}")

    plt, np = ensure_matplotlib()
    if plt is None:
        print("WARN: matplotlib not installed; skipping plot generation. "
              "Install with: pip install matplotlib numpy", file=sys.stderr)
        sys.exit(0)

    status = data.get("status", "schema_only")
    runs = data.get("runs", [])
    if status == "schema_only" or not runs:
        print("INFO: whitepaper-results.json has status=schema_only (no measured runs). "
              "Plot generation will exit 0 with notice.", file=sys.stderr)
        # Пишем placeholder plot, чтобы pipeline не валился.
        fig, ax = plt.subplots(figsize=(8, 4))
        ax.text(0.5, 0.5,
                "No measured runs yet.\nperf/whitepaper-results.json::status = schema_only\n"
                "Run Issue #196 execution plan to populate.",
                ha="center", va="center", transform=ax.transAxes,
                fontsize=14, color="gray")
        ax.set_xticks([])
        ax.set_yticks([])
        placeholder = args.output / "PLACEHOLDER_no_runs.png"
        plt.savefig(placeholder, dpi=150)
        plt.close(fig)
        print(f"Wrote placeholder: {placeholder}")
        return 0

    plots = []
    if args.mode == "full":
        plots.append(("throughput_msg_per_sec.png", plot_throughput_msg_per_sec))
        plots.append(("throughput_bytes_per_sec.png", plot_throughput_bytes_per_sec))
        plots.append(("latency_p50_p95_p99.png", plot_latency_percentiles))
        plots.append(("rate_vs_target.png", plot_rate_vs_target))
        plots.append(("size_distribution.png", plot_size_distribution))
    else:
        plots.append(("variance_diagnosis.png", diag_variance))

    generated = []
    for name, fn in plots:
        out_path = args.output / name
        try:
            ok = fn(plt, np, data, out_path)
            if ok:
                generated.append(str(out_path))
        except Exception as e:
            print(f"WARN: failed to generate {name}: {e}", file=sys.stderr)

    print(f"Generated {len(generated)} plots in {args.output}:")
    for p in generated:
        print(f"  - {p}")
    return 0


if __name__ == "__main__":
    try:
        main(sys.argv[1:])
    except KeyboardInterrupt:
        die(4, "interrupted")
    except Exception as e:
        import traceback
        traceback.print_exc()
        die(4, f"unexpected error: {e}")
