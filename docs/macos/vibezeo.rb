# Homebrew cask for Vibezeo (D-2107). A template: it belongs in a tap
# (`NamanJain-Engineer/homebrew-tap`, Casks/vibezeo.rb) and is valid only
# for a release whose dmg is Developer ID signed, notarized and stapled
# (`spctl -a -vv` says "Notarized Developer ID"; see HUMAN_TODO "macOS
# first run"). Homebrew's own cask repository wants the same, plus notability.
# Per release: set `version`, and `sha256` from the release's SHA256SUMS line
# for Vibezeo_<version>_universal.dmg. Never `sha256 :no_check`.
cask "vibezeo" do
  version "0.0.0"
  sha256 "0000000000000000000000000000000000000000000000000000000000000000"

  url "https://vibezeo.com/downloads/Vibezeo_#{version}_universal.dmg"
  name "Vibezeo"
  desc "Desktop workspace for coding agents"
  homepage "https://vibezeo.com/"

  livecheck do
    url "https://vibezeo.com/downloads/SHA256SUMS"
    regex(/Vibezeo[._-]v?(\d+(?:\.\d+)+)[._-]universal\.dmg/i)
  end

  # The app updates itself (tauri-plugin-updater, F-SH-06).
  auto_updates true
  # macOS 14 or later (tauri.macos.conf.json, DECISIONS).
  depends_on macos: ">= :sonoma"

  app "Vibezeo.app"

  # The background-routines LaunchAgent (F-RT-03; tandem-routines LAUNCHD_LABEL).
  uninstall launchctl: "app.tandem.routines"

  # `brew uninstall --zap` only: ~/.tandem holds the user's workspaces,
  # threads and settings.
  zap trash: [
    "~/.tandem",
    "~/Library/Application Support/com.vibezeo.tandem",
    "~/Library/Caches/com.vibezeo.tandem",
    "~/Library/LaunchAgents/app.tandem.routines.plist",
    "~/Library/Preferences/com.vibezeo.tandem.plist",
    "~/Library/Saved Application State/com.vibezeo.tandem.savedState",
    "~/Library/WebKit/com.vibezeo.tandem",
  ]
end
