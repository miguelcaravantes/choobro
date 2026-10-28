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

ChooBro is an early preview. For now, build it from source on a Mac with [Rust](https://rustup.rs/) and Xcode Command Line Tools installed:

```sh
sh scripts/build-app.sh
```

1. Copy `dist/ChooBro.app` to your **Applications** folder.
2. Open ChooBro, then click **Make Default Browser…** in Settings.
3. Approve the macOS prompts.
4. Click a web link in another app and choose a browser.

The build targets your Mac's processor and macOS 12 or later. Testing has focused on Apple Silicon; older macOS versions and Intel Macs haven't been verified. The app is locally signed, not notarized, so macOS may require you to approve opening it.

## Choose a browser

- Select a browser and click **Open**, or double-click its row.
- Use **↑** and **↓** to choose, then **Return** to open.
- Press **Escape** or close the chooser to cancel the current request.

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
