import json
from pathlib import Path

arch = Path(__file__).resolve().parent
for mode in ("dev", "release"):
    d = json.loads((arch / f"run-map-{mode}.json").read_text(encoding="utf-8"))
    print("=" * 20, d["mode"])
    for lang, r in d["results"]["map"].items():
        print(
            f"{lang:8} build_med={r['build_median_ms']}  "
            f"run_med={r['run_median_ms']}  run_cpu={r['run_cpu_median_ms']}  "
            f"peak_mb={r['run_peak_median_mb']}  shipped={r['shipped_bytes']}"
        )
    print("output:", repr(d["results"]["map"]["ae"]["output"]))
