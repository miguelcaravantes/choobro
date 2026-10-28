cask "choobro" do
  version "0.1.1"
  sha256 "7883c9aea97f95928df64b427cd33fd5c330a93d380768cccad8115b890de10b"

  url "https://github.com/miguelcaravantes/choobro/releases/download/v#{version}/ChooBro-#{version}-macos-arm64.dmg"
  name "ChooBro"
  desc "Choose which browser opens each link"
  homepage "https://github.com/miguelcaravantes/choobro"

  depends_on arch: :arm64
  depends_on macos: :monterey

  app "ChooBro.app"

  postflight_steps do
    run "/usr/bin/xattr", args: ["-dr", "com.apple.quarantine", "{{appdir}}/ChooBro.app"]
  end

  caveats <<~EOS
    ChooBro is an early preview and is not notarized by Apple.
    This cask removes the quarantine attribute only from ChooBro.app,
    bypassing its quarantine-triggered first-launch warning.
    Install only if you trust this project. Gatekeeper is not disabled globally.
  EOS
end
