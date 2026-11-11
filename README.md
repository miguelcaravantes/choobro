# ChooBro

**Your links. Your browser. Your choice.**

Work in Chrome. Personal browsing in Firefox. A link arrives from Teams—why should your default browser decide?

ChooBro (**Choose Browser**) lets you pick where each link opens, right when you click it.

<img src="docs/images/chooser.png" alt="ChooBro’s native macOS chooser with its window frame, link preview, and installed browsers." width="592">

## Why ChooBro?

- **Free. Open source. Yours.** No subscription, no account. [MIT licensed](LICENSE).
- **Native, not a website in a window.** Real macOS controls, system fonts, and light/dark appearance.
- **Lean by design.** Written in Rust with AppKit. No Electron, WebView, bundled browser engine, or background polling.
- **Quick to use.** Arrow keys to choose, Return to open. No settings detour for every link.
- **Your browsers, your rules.** Finds installed browsers automatically. Hide the ones you don't use.
- **Private by default.** No telemetry, network requests, or browsing history. Browser preferences and domain rules are saved locally.

## Install

**Apple Silicon · targets macOS 12+ · early preview**

### Homebrew (recommended)

```sh
brew tap miguelcaravantes/tap
brew trust --cask miguelcaravantes/tap/choobro
brew install --cask miguelcaravantes/tap/choobro
```

Use current Homebrew; update it if `brew trust` is unavailable. The cask lives in [homebrew-tap](https://github.com/miguelcaravantes/homebrew-tap).

**Security:** ChooBro is ad-hoc signed, not Developer ID signed or Apple-notarized. The cask removes quarantine only from ChooBro.app to avoid its first-launch warning—not Gatekeeper globally. This does not make it Apple-verified. Managed Macs may block it.

### Prefer a download?

[Download the DMG](https://github.com/miguelcaravantes/choobro/releases), open it, drag ChooBro to Applications, and eject it. If macOS blocks launch, try opening it first, then use **System Settings → Privacy & Security → Open Anyway** if offered. [Apple's guidance](https://support.apple.com/en-us/102445). Never disable Gatekeeper globally.

Open ChooBro, click **Make Default Browser…** in Settings, and approve the macOS prompts. You're ready.

## Make it yours

- **Open:** choose a browser and press Return, click Open, or double-click its row.
- **Cancel:** press Escape or click Cancel.
- **Copy:** click Copy Link or press ⌘C. You get the full URL, even when the preview ends in an ellipsis; the chooser stays open.
- **Customize:** click **CB → Settings** in the menu bar to hide browsers or refresh the list. Browsers are alphabetical by default; choose **Sort → Manual**, then drag browsers or use the arrows beside the list to set the chooser order. Your manual order is saved when you switch back to alphabetical. New browsers appear at the end in manual mode.
- **Domain rules:** check **Always open `<domain>` in this browser** in the chooser, or add and remove rules in **Settings → Rules**. Exact domains match only that host; `*.example.com` matches `example.com` and its subdomains. Exact rules take priority over wildcard rules. Matching links open directly in the assigned browser; if it is unavailable or hidden, the chooser remains available.
- **Optional auto-open:** in Settings, enable **Automatically open a browser after waiting**, enter a positive delay in seconds (decimals work), and choose a browser. This is off by default, with an initial delay of 10 seconds; existing saved delays are preserved. The chooser shows a seconds-remaining countdown without moving its controls. Keyboard or mouse interaction in the chooser, copying a link, or opening Settings cancels automatic opening for that request; it does not restart. Each later queued request gets its own delay. If the chosen browser is excluded or unavailable, the link stays in the chooser.

Links arriving together open in one browser; later requests wait their turn. Pending links stay in memory, not on disk. Quitting asks before discarding them.

The queue holds up to 256 links and 8 MiB of URL text. A request that exceeds either limit is rejected as a whole with a message; already queued links stay intact.

## Update

Quit ChooBro, then:

```sh
brew update
brew upgrade --cask miguelcaravantes/tap/choobro
```

For DMG installs, replace the app in Applications. Preferences survive updates. Quit and remove a manually installed copy before switching to Homebrew.

## Good to know

- Only HTTP/HTTPS links sent to the system default browser are handled. Embedded or app-forced browsers bypass ChooBro.
- **Microsoft Safe Links:** Recognized Teams and Outlook wrappers are automatically replaced with their embedded destination for display, copying, routing, and opening. This bypasses Microsoft's Safe Links reputation check; only open links you trust.
- Browser profiles and private windows aren't supported.
- Older macOS compatibility, startup speed, and RAM usage haven't been fully verified or benchmarked.
- Before removing ChooBro, choose another default browser in macOS Settings.

## Build from source

With Rust, Python 3, and Xcode Command Line Tools installed:

```sh
sh scripts/build-app.sh
```

Output: `dist/ChooBro.app`. On Apple Silicon, `sh scripts/build-dmg.sh` also creates a DMG and SHA-256 checksum.
