#!/usr/bin/env python3
"""Instrument temporary compiler inputs; never edit generated or shipping sources."""
import sys
from pathlib import Path

output = Path(sys.argv[1])
output.mkdir(parents=True, exist_ok=True)
for source, marker, probe in [
    ("apple/Generated/wirebolt_ffi.swift",
     "public static func write(_ value: DomainRequestDraft, into buf: inout [UInt8]) {",
     "DraftWorkProbe.encoded()"),
    ("apple/Sources/WireboltKit/Models.swift",
     "public static func == (lhs: Self, rhs: Self) -> Bool { lhs.domain == rhs.domain }",
     None),
]:
    text = Path(source).read_text()
    assert text.count(marker) == 1, f"Update the draft-work probe for {source}"
    replacement = (marker + "\n        " + probe if probe else
                   "public static func == (lhs: Self, rhs: Self) -> Bool { "
                   "DraftWorkProbe.compared(); return lhs.domain == rhs.domain }")
    (output / Path(source).name).write_text(text.replace(marker, replacement))
