# ChooBro

**Choose Browser.** Pick which browser opens each link on your Mac.

Keep work links in your work browser and personal links in your personal browser. When you click a link in an app that uses your default browser, ChooBro shows your installed browsers and lets you choose where it opens.

Built in Rust with a native macOS interface.

## Features

- Choose a browser for each link instead of sending everything to one default.
- Discover installed browsers automatically, with their names and icons.
- Hide browsers you don't want in the chooser.
- Navigate with the keyboard or mouse.
- Keep multiple incoming requests queued without losing the link you're choosing for.
- Read long links in a compact, wrapping preview.
- Follow your Mac's light or dark appearance.

## Get started

### Homebrew (preferred)

Homebrew is the preferred installation method for this early preview: it avoids the manual macOS approval normally required by the unnotarized DMG download and makes updates easier. This convenience uses the app-specific quarantine workaround explained below—not Apple notarization.

For Apple Silicon Macs with a current version of [Homebrew](https://brew.sh/) installed:

```sh
brew tap miguelcaravantes/choobro https://github.com/miguelcaravantes/choobro
brew trust --cask miguelcaravantes/choobro/choobro
brew install --cask miguelcaravantes/choobro/choobro
```

Homebrew 7 requires the explicit cask trust step. If `brew trust` isn't available, update Homebrew before continuing.

Open ChooBro from Applications, click **Make Default Browser…** in Settings, and approve the macOS prompts.

**Security tradeoff:** ChooBro is not notarized by Apple. This custom cask removes the `com.apple.quarantine` attribute only from ChooBro's app bundle to avoid its quarantine-triggered first-launch warning. It does not disable Gatekeeper globally or make the app Apple-verified. Install this way only if you trust the project; managed Macs may still block it.

If you already installed ChooBro manually, quit it before switching to Homebrew. Homebrew may ask you to remove the existing copy from Applications first; browser preferences are stored separately.

To update, quit ChooBro, then run:

```sh
brew update
brew upgrade --cask miguelcaravantes/choobro/choobro
```

### DMG download (manual installation)

Use this alternative if you don't use Homebrew. Because ChooBro isn't Developer ID signed or notarized yet, macOS may block the app and require you to approve it manually in **System Settings → Privacy & Security**.

**[Download the preview DMG](https://github.com/miguelcaravantes/choobro/releases)** for Apple Silicon Macs (M1 or later). No Rust installation or build tools needed.

1. Download `ChooBro-<version>-macos-arm64.dmg` from the release's **Assets** section and open it.
2. Drag **ChooBro** to **Applications**, then eject the disk image.
3. Open ChooBro from Applications and click **Make Default Browser…** in Settings.
4. Approve the macOS prompts, then click a web link in another app and choose a browser.

**Early preview:** the app is ad-hoc signed, not Developer ID signed or notarized by Apple. macOS may block it on first launch. If you trust the download, try opening it, then use **System Settings → Privacy & Security → Open Anyway** if macOS offers it. See [Apple's guidance](https://support.apple.com/en-us/102445). Don't disable your Mac's security protections globally.

Downloads currently support Apple Silicon only. The deployment target is macOS 12 or later, but older macOS versions haven't been fully tested.

To update, quit ChooBro before replacing the copy in Applications. Browser preferences are preserved.

## Choose a browser

- Select a browser and click **Open**, or double-click its row.
- Use **↑** and **↓** to choose, then **Return** to open.
- Press **Escape** or click **Cancel** to cancel the current request.

If an app sends several links together, they open in the same browser. Links arriving afterward wait for their own selection. Hover over the URL preview to see the full link; ChooBro forwards the original URL without rewriting it.

## Settings

Click **CB** in the menu bar to open **Settings**, show pending links, or quit ChooBro.

Checked browsers appear in the chooser. Uncheck any you want to hide; this doesn't uninstall them or change their settings. Click **Refresh** after installing or removing a browser. macOS may also list other apps that handle web links—you can hide those too.

## Privacy

ChooBro works locally. It doesn't collect telemetry, make network requests, or save your browsing history. Only browser preferences are stored. Pending links stay in memory until opened or canceled; quitting discards them after confirmation.

Your selected browser loads the destination and applies its own privacy settings.

## Limitations

- ChooBro handles HTTP and HTTPS links sent to the system default browser. It cannot intercept links an app opens in an embedded browser or forces into a specific browser.
- Browser profiles, private windows, and automatic routing rules aren't supported yet.
- Before moving or deleting ChooBro, choose another default browser in macOS System Settings so your links continue to open normally.

## License

[MIT](LICENSE)

## Build from source

On a Mac with [Rust](https://rustup.rs/), Python 3, and Xcode Command Line Tools installed:

```sh
sh scripts/build-app.sh
```

The app is created at `dist/ChooBro.app`. On Apple Silicon, `sh scripts/build-dmg.sh` also creates a DMG and SHA-256 checksum.
