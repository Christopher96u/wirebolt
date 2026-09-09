import json
from pathlib import Path
import sys

metadata = json.loads(Path(sys.argv[1]).read_text())
packages = {package["id"]: package for package in metadata["packages"]}
nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
pending = [package["id"] for package in packages.values() if package["name"] == "wirebolt-ffi"]
seen = set()
notices = []
while pending:
    package_id = pending.pop()
    if package_id in seen:
        continue
    seen.add(package_id)
    for dependency in nodes[package_id]["deps"]:
        if any(kind["kind"] is None for kind in dependency["dep_kinds"]):
            pending.append(dependency["pkg"])
    package = packages[package_id]
    if package["source"] is None:
        continue
    root = Path(package["manifest_path"]).parent
    files = set()
    for path in root.iterdir():
        if path.is_file() and path.name.upper().startswith(("LICENSE", "LICENCE", "COPYING", "NOTICE")):
            files.add(path)
        if path.is_dir() and path.name.lower() in ("license", "licenses"):
            files.update(item for item in path.rglob("*") if item.is_file())
    if package.get("license_file"):
        files.add(root / package["license_file"])
    if not files:
        family = "uniffi" if package["name"].startswith("uniffi") else package["name"]
        fallback = Path(__file__).parent / "licenses" / f"{family}.txt"
        if fallback.is_file():
            files.add(fallback)
    if not files:
        raise SystemExit(f"Missing license text: {package['name']} {package['version']}")
    texts = [path.read_text() for path in sorted(files)]
    source = f"https://crates.io/api/v1/crates/{package['name']}/{package['version']}/download"
    notices.append((package["name"], package["version"], source + "\n\n" + "\n\n".join(texts)))

Path(sys.argv[2]).write_text("\n\n".join(
    f"{name} {version}\n{'=' * (len(name) + len(version) + 1)}\n\n{text}"
    for name, version, text in sorted(notices)
) + "\n")
