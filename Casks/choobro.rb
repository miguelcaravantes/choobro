cask "choobro" do
  version "0.1.2"
  sha256 "c62689ba82469310240b3c6e98fbab97f18a0198eb85b7143e936ee08ea9884c"

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
end
