cask "choobro" do
  version "0.1.3"
  sha256 "eb7c3546f6453803b803e3540440b2536ee49cb53a61efd8188e3feb3a732944"

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
