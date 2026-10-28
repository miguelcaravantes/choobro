cask "choobro" do
  version "0.1.0"
  sha256 "ebc278ba3285be0eea5a9883eb59e52c7ce8a417c6aaa831742fb15a78876d4d"

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
