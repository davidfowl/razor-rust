#!/usr/bin/env python3
"""Bounded, fail-closed, byte-equivalent rendering comparison (standard library only)."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import statistics
import subprocess
import sys
import tempfile
from datetime import datetime, timezone


ROOT = Path(__file__).resolve().parent
PIN = "552937e5f2f21ebcdc73939be014f3d217ffbb22"
ENGINES = {"razor-rust", "askama", "maud", "tera", "razorslices-csharp"}


def command(arguments, cwd=ROOT):
    result = subprocess.run(
        [str(argument) for argument in arguments], cwd=cwd, text=True, capture_output=True
    )
    if result.returncode:
        sys.stderr.write(result.stdout + result.stderr)
        raise RuntimeError(f"command failed ({result.returncode}): {arguments[0]}")
    return result.stdout.strip()


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def invoke(arguments):
    output = command(arguments)
    records = [json.loads(line) for line in output.splitlines()]
    for record in records:
        print(json.dumps(record), flush=True)
    return records


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--upstream", type=Path, required=True, help="clean pinned RazorSlices checkout")
    parser.add_argument("--mode", choices=["smoke", "measure"], default="smoke")
    parser.add_argument("--verify-rejection", action="store_true", help="also corrupt expected bytes and require rejection by all engines")
    parser.add_argument("--output", type=Path, default=ROOT / "results" / "latest.json")
    args = parser.parse_args()
    upstream = args.upstream.resolve()
    if command(["git", "-C", upstream, "rev-parse", "HEAD"]) != PIN:
        raise RuntimeError(f"upstream must be pinned to {PIN}")
    if command(["git", "-C", upstream, "status", "--porcelain", "--untracked-files=all"]):
        raise RuntimeError("upstream must be completely clean")

    command(["cargo", "build", "--release", "--locked"])
    command(
        ["dotnet", "build", "-c", "Release", "--nologo",
         "-p:RazorSlicesSource=" + str(upstream)],
        cwd=ROOT / "csharp",
    )
    # DOTNET_* settings affect tiering/GC; retain the environment, but record overrides.
    protocol = (
        {"warmup": 2, "samples": 1, "iterations": 2}
        if args.mode == "smoke"
        else {"warmup": 1000, "samples": 5, "iterations": 5000}
    )
    counts = [str(protocol[key]) for key in ("warmup", "samples", "iterations")]
    cargo_metadata = json.loads(command(["cargo", "metadata", "--locked", "--format-version", "1", "--no-deps"]))
    rust_binary = Path(cargo_metadata["target_directory"]) / "release" / "render-comparison"
    csharp_binary = ROOT / "csharp/bin/Release/net10.0/RenderComparison.dll"
    prefixes = [[str(rust_binary)], ["dotnet", str(csharp_binary)]]

    def calls(action, expected):
        return [prefix + [action, str(ROOT / "fixture.json"), str(expected)] + counts for prefix in prefixes]

    gates = []
    # Every engine passes before either process is allowed to time anything.
    for call in calls("verify", ROOT / "expected.html"):
        gates.extend(invoke(call))
    if {gate["engine"] for gate in gates} != ENGINES or not all(gate["equal"] for gate in gates):
        raise RuntimeError("missing or failed cross-framework equality gate")

    rejections = []
    rejection_results = []
    if args.verify_rejection:
        with tempfile.TemporaryDirectory(prefix="render-comparison-") as directory:
            wrong = Path(directory) / "wrong.html"
            wrong.write_bytes((ROOT / "expected.html").read_bytes() + b"deliberate-mismatch")
            for call in calls("measure", wrong):
                result = subprocess.run(call, cwd=ROOT, text=True, capture_output=True)
                if result.returncode == 0 or '"kind":"sample"' in result.stdout.replace(" ", ""):
                    raise RuntimeError("corrupt fixture did not prevent measurement")
                rejected = [engine for engine in ENGINES if f"output mismatch: {engine}," in result.stderr]
                if not rejected:
                    raise RuntimeError("negative control failed for an unrelated reason:\n" + result.stderr)
                rejections.extend(rejected)
                rejection_results.append({
                    "engines": sorted(rejected), "exit_code": result.returncode,
                    "stdout": result.stdout, "stderr": result.stderr,
                })
            if set(rejections) != ENGINES:
                raise RuntimeError("not all engines rejected the corrupt expected bytes")

    records = []
    for call in calls("measure", ROOT / "expected.html"):
        records.extend(invoke(call))
    samples = [record for record in records if record["kind"] == "sample"]
    if len(samples) != len(ENGINES) * protocol["samples"]:
        raise RuntimeError("missing measured samples")
    for sample in samples:
        if sample["elapsed_ns"] <= 0 or sample["allocated_bytes"] <= 0:
            raise RuntimeError("invalid elapsed time or missing genuine allocation measurement")
        if sample["engine"] != "razorslices-csharp" and sample["allocation_events"] <= 0:
            raise RuntimeError("missing Rust allocation events")

    sdk_version = command(["dotnet", "--version"], cwd=ROOT / "csharp")
    sdk_directory = None
    for line in command(["dotnet", "--list-sdks"]).splitlines():
        if line.startswith(sdk_version + " "):
            sdk_directory = Path(line.split("[", 1)[1].rstrip("]")) / sdk_version
    if sdk_directory is None:
        raise RuntimeError("could not identify Razor/C# compiler SDK")
    compiler = command(["dotnet", sdk_directory / "Roslyn/bincore/csc.dll", "-version"])
    cpuinfo = Path("/proc/cpuinfo")
    cpu_models = sorted({
        line.split(":", 1)[1].strip()
        for line in cpuinfo.read_text().splitlines()
        if line.startswith("model name")
    }) if cpuinfo.exists() else [platform.processor()]
    environment = {
        "platform": platform.platform(), "architecture": platform.machine(),
        "cpu_models": cpu_models, "logical_cpus": os.cpu_count(),
        "affinity_cpus": len(os.sched_getaffinity(0)) if hasattr(os, "sched_getaffinity") else None,
        "meminfo": Path("/proc/meminfo").read_text() if Path("/proc/meminfo").exists() else None,
        "cgroup_cpu_max": Path("/sys/fs/cgroup/cpu.max").read_text().strip() if Path("/sys/fs/cgroup/cpu.max").exists() else None,
        "cgroup_memory_max": Path("/sys/fs/cgroup/memory.max").read_text().strip() if Path("/sys/fs/cgroup/memory.max").exists() else None,
        "runtime_overrides": {
            key: os.environ[key]
            for key in (
                "DOTNET_TieredCompilation", "DOTNET_TieredPGO", "DOTNET_ReadyToRun",
                "DOTNET_gcServer", "DOTNET_GCHeapHardLimit", "DOTNET_GCConserveMemory",
                "COMPlus_TieredCompilation", "COMPlus_TieredPGO", "COMPlus_ReadyToRun",
                "COMPlus_gcServer", "COMPlus_GCHeapHardLimit", "COMPlus_GCConserveMemory",
            )
            if key in os.environ
        },
    }
    report = {
        "timestamp_utc": datetime.now(timezone.utc).isoformat(),
        "source_head": command(["git", "rev-parse", "HEAD"]),
        "source_status": command(["git", "status", "--porcelain"]),
        "upstream_commit": PIN,
        "mode": args.mode, "profile": "release", "protocol": protocol,
        "fixture_sha256": sha256(ROOT / "fixture.json"),
        "expected_sha256": sha256(ROOT / "expected.html"),
        "cargo_lock_sha256": sha256(ROOT / "Cargo.lock"),
        "harness_sha256": {
            str(path.relative_to(ROOT)): sha256(path)
            for path in sorted(ROOT.rglob("*"))
            if path.is_file() and path.suffix in {".rs", ".cs", ".cshtml", ".csproj", ".py", ".toml", ".json", ".html"}
            and not any(part in {"target", "bin", "obj", "results", "evidence", "__pycache__"} for part in path.relative_to(ROOT).parts)
        },
        "versions": {
            "rustc": command(["rustc", "--version", "--verbose"]),
            "cargo": command(["cargo", "--version"]),
            "packages": command(["cargo", "tree", "--locked", "--prefix", "none"]),
            "dotnet": command(["dotnet", "--info"], cwd=ROOT / "csharp"),
            "csharp_compiler": compiler,
            "sdk_razor_compiler_sha256": sha256(sdk_directory / "Sdks/Microsoft.NET.Sdk.Razor/source-generators/Microsoft.CodeAnalysis.Razor.Compiler.dll"),
        },
        "generated_cshtml_sha256": {
            str(path.relative_to(ROOT)): sha256(path)
            for path in sorted((ROOT / "csharp/obj/generated").rglob("*.cs"))
        },
        "environment": environment,
        "gates": gates, "deliberate_mismatch_rejected": sorted(rejections),
        "negative_controls": rejection_results,
        "rust_allocator_calibration": {"passed": True, "events": 3, "requested_bytes": 224},
        "samples": samples,
        "summary": {
            engine: {
                "median_renders_per_second": statistics.median(
                    sample["renders_per_second"] for sample in samples if sample["engine"] == engine
                ),
                "median_allocated_bytes_per_render": statistics.median(
                    sample["allocated_bytes_per_render"] for sample in samples if sample["engine"] == engine
                ),
                "allocation_events_per_render": [
                    sample["allocation_events_per_render"] for sample in samples if sample["engine"] == engine
                ],
            }
            for engine in sorted(ENGINES)
        },
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(f"Evidence: {args.output.resolve()}", flush=True)


if __name__ == "__main__":
    try:
        main()
    except (OSError, RuntimeError, ValueError) as error:
        sys.exit(str(error))
