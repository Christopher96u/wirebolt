import hashlib
from pathlib import Path
import re
import sys

version, archive = sys.argv[1:]
if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+(-(rc|beta)\.[1-9][0-9]*)?", version):
    raise SystemExit("Invalid release version")
digest = hashlib.sha256()
with Path(archive).open("rb") as stream:
    for chunk in iter(lambda: stream.read(1024 * 1024), b""):
        digest.update(chunk)
print('''cask "wirebolt" do
  version "%s"
  sha256 "%s"

  url "https://github.com/Christopher96u/wirebolt/releases/download/v#{version}/Wirebolt-#{version}-arm64.zip"
  name "Wirebolt"
  desc "Native HTTP and WebSocket API client"
  homepage "https://github.com/Christopher96u/wirebolt"

  depends_on arch: :arm64
  depends_on macos: :sequoia

  app "Wirebolt.app"

  # The app is ad-hoc signed and not notarized, so Gatekeeper would block the first launch.
  postflight_steps do
    run "/usr/bin/xattr", args: ["-dr", "com.apple.quarantine", "{{appdir}}/Wirebolt.app"]
  end

  # Workspaces under Application Support/Wirebolt/Workspaces are user documents and are kept.
  zap trash: [
    "~/Library/Application Support/Wirebolt/Cookies",
    "~/Library/Application Support/Wirebolt/History",
    "~/Library/Preferences/io.github.christopher96u.wirebolt.plist",
    "~/Library/Saved Application State/io.github.christopher96u.wirebolt.savedState",
  ]

  caveats <<~EOS
    Wirebolt is ad-hoc signed and not notarized by Apple.
    This cask removes the quarantine attribute from Wirebolt.app so it opens normally.
  EOS
end''' % (version, digest.hexdigest()))
