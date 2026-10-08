"""Chart a bounded trace window without exporting PCM or authored names.

Native taps are supplied as NODE=DB=measured|inferred. They must describe
the same observation window; --note records any state mismatch explicitly.
"""
import argparse
import json
import math
from pathlib import Path


def summarize(trace, first, last):
    if not trace["complete"] or trace["dropped"]:
        raise ValueError("A complete, zero-loss trace is required")
    levels = {}
    for row in trace["records"]:
        if row["contribution"]:
            continue
        frames = max(0, min(last, row["at"] + row["frames"]) - max(first, row["at"]))
        if not frames:
            continue
        cell = levels.setdefault(row["node"], {"frames": 0, "input": 0., "output": 0.})
        cell["frames"] += frames
        for side in ("input", "output"):
            cell[side] += sum(v * v for v in row[side]["rms"][:row["identity"].get("output_channels") or 2]) / (row["identity"].get("output_channels") or 2) * frames
        cell["last"] = row
    db = lambda energy: 10 * math.log10(energy) if energy > 0 else None
    nodes = trace["graph"]["nodes"]
    result = []
    for node_id in trace["graph"]["order"]:
        if node_id not in levels:
            continue
        cell = levels[node_id]
        row = cell["last"]
        node = nodes[node_id]
        inp, out = (db(cell[s] / cell["frames"]) for s in ("input", "output"))
        result.append({"node": node_id, "kind": node["kind"], "processor": node["processor"],
                       "zone": node["zone"], "group": node["group"], "bus": node["bus"],
                       "frames": cell["frames"], "input_dbfs": inp, "output_dbfs": out,
                       "delta_db": out - inp if inp is not None and out is not None else None,
                       "latency_samples": row["latency_samples"], "enabled": row["enabled"],
                       "output_channels": row["identity"].get("output_channels"),
                       "gain_measurement": node.get("gain_measurement", "legacy_unspecified"),
                       "parameters": [{"name": p["name"], "value": row["values"][i],
                                       "native": row["normalized"][i]}
                                      for i, p in enumerate(node["parameters"][:16])]})
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("trace", type=Path)
    parser.add_argument("output", type=Path, help="Artifact stem in the run directory")
    parser.add_argument("--first-frame", type=int, required=True)
    parser.add_argument("--last-frame", type=int, required=True)
    parser.add_argument("--reference", action="append", default=[])
    parser.add_argument("--note", default="")
    args = parser.parse_args()
    if args.first_frame < 0 or args.last_frame <= args.first_frame:
        parser.error("Window must have positive length")
    trace = json.loads(args.trace.read_text())
    rows = summarize(trace, args.first_frame, args.last_frame)
    references = []
    for text in args.reference:
        node, value, method = text.split("=")
        if method not in ("measured", "inferred"):
            parser.error("Reference method must be measured or inferred")
        node, value = int(node), float(value)
        if not math.isfinite(value) or node not in {r["node"] for r in rows}:
            parser.error("Reference requires a finite value and observed node")
        references.append({"node": node, "dbfs": value, "method": method})
    receipt = {"complete": True, "dropped": 0, "window_frames": [args.first_frame, args.last_frame],
               "rows": rows, "reference": references, "note": args.note,
               "edge_block_metrics": "weighted by overlap; no PCM is retained"}
    args.output.with_suffix(".json").write_text(json.dumps(receipt, indent=2) + "\n")

    import matplotlib
    matplotlib.use("Agg")
    import matplotlib.pyplot as plt
    fig, ax = plt.subplots(figsize=(17, max(4, .37 * len(rows) + 2)))
    y = list(range(len(rows)))
    ax.barh(y, [max(-90., r["output_dbfs"] if r["output_dbfs"] is not None else -90.) + 90. for r in rows], left=-90.,
            color=["#207cb5" if r["enabled"] else "#82909a" for r in rows])
    labels = [f"#{r['node']} {r['kind']}/{r['processor']}  z{r['zone']} g{r['group']} b{r['bus']}"
              for r in rows]
    ax.set_yticks(y, labels, fontsize=8)
    for i, row in enumerate(rows):
        level = f"{row['output_dbfs']:.2f}" if row["output_dbfs"] is not None else "−inf"
        delta = f"{row['delta_db']:+.2f}" if row["delta_db"] is not None else "unknown"
        ax.text(3, i, f"{level} dBFS   Δ{delta} dB   latency {row['latency_samples']}", fontsize=8, va="center")
    for ref in references:
        i = next(i for i, r in enumerate(rows) if r["node"] == ref["node"])
        ax.scatter(ref["dbfs"], i, marker="x" if ref["method"] == "inferred" else "D", color="#b13c34", zorder=3)
        ax.annotate(f"native {ref['method']} {ref['dbfs']:.2f}", (ref["dbfs"], i),
                    xytext=(0, -13), textcoords="offset points", fontsize=8, color="#b13c34")
    ax.invert_yaxis()
    ax.set_xlim(-70, 35)
    ax.set_xlabel("Mean-channel RMS dBFS; coherent node sums (mono uses its one physical channel)")
    ax.set_title(f"Signal trace: frames {args.first_frame}–{args.last_frame}; zero drops\n{args.note}", fontsize=10)
    ax.grid(axis="x", alpha=.2)
    fig.tight_layout()
    for extension in ("svg", "png"):
        fig.savefig(args.output.with_suffix("." + extension), dpi=130)
    plt.close(fig)


if __name__ == "__main__":
    main()
