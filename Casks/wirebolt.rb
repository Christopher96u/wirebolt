cask "wirebolt" do
  version "1.0.0-rc.1"
  sha256 "e3447e8816ea1e11eb6e008b368d054cf80ed1b2fe160c9e3ce2a2d8917cf738"

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
end
