#!/usr/bin/env python3
"""Reproducible desktop-app benchmark: cold start, idle RSS, teardown.

Usage:
    # 1) build first (custom-protocol embeds the frontend; without it the
    #    binary needs a dev server and shows a blank window)
    cargo build --release -p kvn-desktop --features custom-protocol
    # 2) run
    python3 scripts/benchmark-app.py [--runs 3] [--keep-state]

Measures (unix only; macOS/Linux):
  * cold start: launch → app data dir created (backend setup finished);
  * UI start: launch → "ui boot finished" appears in logs/kvn.log (webview
    loaded, the frontend bundle ran and IPC works end to end);
  * idle RSS: main process + all descendants (webview helpers);
  * teardown: SIGTERM → all processes gone.

The data dir is reset between runs unless --keep-state is given, so the
startup number is a true cold start (no caches on disk).

Honest reporting: numbers are medians of the runs; the environment is
printed alongside (machine, OS, binary size) because no two Macs are equal.
"""

import argparse
import os
import platform
import shutil
import signal
import subprocess
import sys
import time

APP_ID = "dev.zeroscrypt.kvn"
BINARY = "target/release/kvn-desktop"


def data_dir() -> str:
    if sys.platform == "darwin":
        return os.path.expanduser(f"~/Library/Application Support/{APP_ID}")
    return os.path.expanduser(f"~/.local/share/{APP_ID}")


def descendants_rss_kb(pid: int) -> tuple[int, int]:
    """Return (total_rss_kb, process_count) for pid + descendants."""
    out = subprocess.run(
        ["ps", "-axo", "pid=,ppid=,rss="], capture_output=True, text=True
    ).stdout
    procs: dict[int, tuple[int, int]] = {}
    for line in out.splitlines():
        parts = line.split()
        if len(parts) != 3:
            continue
        p, pp, rss = int(parts[0]), int(parts[1]), int(parts[2])
        procs[p] = (pp, rss)

    def children_of(p: int) -> list[int]:
        return [k for k, (pp, _) in procs.items() if pp == p]

    total = 0
    count = 0
    stack = [pid]
    seen = set()
    while stack:
        current = stack.pop()
        if current in seen:
            continue
        seen.add(current)
        if current in procs:
            total += procs[current][1]
            count += 1
        stack.extend(children_of(current))
    return total, count


def one_run(run_index: int) -> dict:
    reset = data_dir()
    if os.path.isdir(reset):
        shutil.rmtree(reset)

    started = time.monotonic()
    proc = subprocess.Popen(
        [BINARY],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        cwd=os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
    )

    dir_created_ms = None
    ui_ready_ms = None
    deadline = started + 15
    log_path = os.path.join(reset, "logs", "kvn.log")
    while time.monotonic() < deadline:
        if dir_created_ms is None and os.path.isdir(reset):
            dir_created_ms = (time.monotonic() - started) * 1000
        if ui_ready_ms is None:
            try:
                with open(log_path, "r", encoding="utf-8", errors="replace") as fh:
                    if "ui boot finished" in fh.read():
                        ui_ready_ms = (time.monotonic() - started) * 1000
            except OSError:
                pass
        if dir_created_ms is not None and ui_ready_ms is not None:
            break
        if proc.poll() is not None:
            raise RuntimeError(f"app exited during startup with code {proc.returncode}")
        time.sleep(0.005)

    time.sleep(4.0)  # let the webview settle
    rss_kb, count = descendants_rss_kb(proc.pid)

    proc.send_signal(signal.SIGTERM)
    try:
        proc.wait(timeout=8)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.wait(timeout=5)
        teardown = "forced (SIGKILL)"
    else:
        teardown = "clean (SIGTERM)"

    time.sleep(0.5)
    leftover = descendants_rss_kb(proc.pid)[1]  # main is gone; helpers too?

    layout = {}
    for sub in ("bin", "runtime", "logs"):
        layout[sub] = os.path.isdir(os.path.join(reset, sub))

    return {
        "run": run_index + 1,
        "dir_created_ms": dir_created_ms,
        "ui_ready_ms": ui_ready_ms,
        "idle_rss_mb": round(rss_kb / 1024, 1),
        "processes": count,
        "teardown": teardown,
        "layout": layout,
        "leftover_processes": leftover,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--keep-state", action="store_true")
    args = parser.parse_args()

    if not os.path.exists(BINARY):
        print(f"error: {BINARY} not found — run: cargo build --release -p kvn-desktop")
        return 1

    size_mb = os.path.getsize(BINARY) / (1024 * 1024)
    print(f"host:   {platform.machine()} / {platform.platform()}")
    print(f"binary: {BINARY} — {size_mb:.1f} MiB")
    print(f"state:  {data_dir()}")
    print("-" * 64)

    results = []
    for i in range(args.runs):
        result = one_run(i)
        results.append(result)
        ui = result["ui_ready_ms"]
        ui_text = f"{ui:.0f} ms" if ui is not None else "NOT REACHED"
        print(
            f"run {result['run']}: backend {result['dir_created_ms']:.0f} ms, "
            f"ui ready {ui_text}, "
            f"idle RSS {result['idle_rss_mb']} MiB across "
            f"{result['processes']} processes, {result['teardown']}, "
            f"leftover {result['leftover_processes']}"
        )

    starts = sorted(r["dir_created_ms"] for r in results)
    uis = sorted(r["ui_ready_ms"] for r in results if r["ui_ready_ms"] is not None)
    rss = sorted(r["idle_rss_mb"] for r in results)
    median_start = starts[len(starts) // 2]
    median_rss = rss[len(rss) // 2]
    print("-" * 64)
    print(f"median backend ready: {median_start:.0f} ms")
    if len(uis) == len(results):
        print(f"median ui ready:      {uis[len(uis) // 2]:.0f} ms")
    else:
        print(f"median ui ready:      WARNING — only {len(uis)}/{len(results)} runs reached a booted UI")
    print(f"median idle RSS:      {median_rss} MiB")

    if not all(r["layout"].get("bin") and r["layout"].get("runtime") and r["layout"].get("logs") for r in results):
        print("WARNING: expected data layout (bin/runtime/logs) missing!")
        return 1
    if any(r["leftover_processes"] > 0 for r in results):
        print("WARNING: processes survived teardown!")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
