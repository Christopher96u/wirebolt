#!/usr/bin/env python3
"""Five fresh production-search processes; never confuse allocator reuse with a pair's peak."""
import json
import statistics
import subprocess
import sys
import tempfile
from pathlib import Path

binary, size, output = sys.argv[1:4]
budgets = json.loads(Path("performance/budgets.json").read_text())
results = []
with tempfile.TemporaryDirectory(prefix="wirebolt-search-samples-") as directory:
    for sample in range(5):
        path = Path(directory) / f"{sample}.json"
        subprocess.run([binary, size, str(path)], check=True, timeout=60)
        results.append(json.loads(path.read_text()))
measurements = []
for index, measurement in enumerate(results[0]["measurements"]):
    values = [run["measurements"][index]["samples_ms"][0] for run in results]
    measurements.append(dict(name=measurement["name"], samples_ms=values,
                             median_ms=statistics.median(values), min_ms=min(values),
                             max_ms=max(values)))
peaks = [run["two_query_peak_rss_bytes"] / 1_048_576 for run in results]
failed = int(size) == 100 and (
    measurements[0]["max_ms"] > budgets["indexedSearchAbsentP95Milliseconds"]
    or measurements[1]["median_ms"] > budgets["indexedSearchCappedMedianMilliseconds"]
    or measurements[1]["max_ms"] > budgets["indexedSearchCappedP95Milliseconds"]
    or max(peaks) > budgets["indexedSearchTwoQueryPeakMebibytes"])
summary = dict(size_mib=int(size), measurements=measurements, two_query_peak_rss_mib=peaks,
               peak_rss_median_mib=statistics.median(peaks), peak_rss_min_mib=min(peaks),
               peak_rss_max_mib=max(peaks), over_budget=failed)
Path(output).write_text(json.dumps(summary, indent=2) + "\n")
print(json.dumps(summary))
if "--check" in sys.argv and failed:
    sys.exit(1)
