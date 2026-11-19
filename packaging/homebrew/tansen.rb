# Homebrew cask template for Tansen.
#
# NOT INSTALLABLE YET: the sha256 must be filled from a real signed and
# notarized GitHub release DMG (see README.md in this directory). Do not
# publish a cask that points at an unsigned or nonexistent artifact.
cask "tansen" do
  version "0.3.0"
  sha256 "0000000000000000000000000000000000000000000000000000000000000000"

  url "https://github.com/shiwng/tansen/releases/download/v#{version}/Tansen_#{version}_aarch64.dmg"
  name "Tansen"
  desc "Local-first desktop music player for macOS"
  homepage "https://github.com/shiwng/tansen"

  livecheck do
    url :homepage
    strategy :github_latest
  end

  depends_on arch: :arm64
  depends_on macos: ">= :catalina"

  app "Tansen.app"

  zap trash: "~/Library/Application Support/tansen"
end
