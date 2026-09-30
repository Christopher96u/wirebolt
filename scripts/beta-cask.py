import hashlib
from pathlib import Path
import re
import sys

version, archive = sys.argv[1:]
if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+-beta\.[1-9][0-9]*", version):
    raise SystemExit("Invalid beta version")
digest = hashlib.sha256()
with Path(archive).open("rb") as stream:
    for chunk in iter(lambda: stream.read(1024 * 1024), b""):
        digest.update(chunk)
print('''cask "wirebolt" do
  version "%s"
  sha256 "%s"

  url "https://github.com/Christopher96u/homebrew-tap/releases/download/v#{version}/Wirebolt-#{version}-arm64.zip"
  name "Wirebolt"
  homepage "https://github.com/Christopher96u/homebrew-tap"

  depends_on arch: :arm64
  depends_on macos: :sequoia

  app "Wirebolt.app"

  # Workspaces under Application Support/Wirebolt/Workspaces are user documents and are kept.
  zap trash: [
    "~/Library/Application Support/Wirebolt/Cookies",
    "~/Library/Application Support/Wirebolt/History",
    "~/Library/Preferences/io.github.christopher96u.wirebolt.plist",
    "~/Library/Saved Application State/io.github.christopher96u.wirebolt.savedState",
  ]
end''' % (version, digest.hexdigest()))
