# ChooBro

**Choose Browser.** A small, native macOS browser chooser written in Rust.

Open source under the [MIT license](LICENSE). Repository: [miguelcaravantes/choobro](https://github.com/miguelcaravantes/choobro).

Use one browser for development and another for personal browsing? ChooBro lets you choose where a link opens instead of sending every link to the same browser.

## How it works

1. Install `ChooBro.app` and approve making it the default browser.
2. Click an HTTP or HTTPS link in Teams, WhatsApp, or another app that uses the system default browser.
3. Choose an installed browser from a compact native window.
4. ChooBro forwards the link directly to that browser.

Settings let you exclude browsers from the chooser. Exclusion only affects ChooBro; it does not uninstall or change those browsers. A **CB** menu-bar item provides Settings, Show Pending Links, and Quit.

## Status

An early runnable prototype, built with **Rust + AppKit through `objc2`**: real macOS controls, no HTML or WebView. Not a production release; performance, supported-OS compatibility, and real browser handoff still need broader verification.

See [ARCHITECTURE.md](ARCHITECTURE.md) for the research, tradeoffs, and implementation sequence.

## Build and run

Requires macOS, Xcode or its Command Line Tools, and Rust installed through [rustup](https://rustup.rs/). The repository pins Rust 1.91.1 and targets the host CPU architecture.

```sh
sh scripts/build-app.sh
open dist/ChooBro.app
```

This creates an ad-hoc-signed development bundle at `dist/ChooBro.app`. It does not install the app or change your default browser. Quit and reopen ChooBro after rebuilding to load the new executable; rebuilding does not restart a running copy or discard its pending links.

To try a link without changing your default browser:

```sh
open -a "$PWD/dist/ChooBro.app" 'https://example.com'
```

Choose a row with the arrow keys and press Return, double-click a browser, or click Open. Escape or the close button cancels the current request. Links received together open in the same browser; later requests queue rather than replace the current selection.

Long URLs wrap across up to three preview lines without widening the chooser. The original URL is forwarded unchanged and remains available in the preview tooltip.

For regular use, quit the development copy, copy `ChooBro.app` to Applications, reopen that copy, then use **Make Default Browser…** in Settings. This explicitly requests macOS approval for HTTP and HTTPS and reports their current handlers separately. Do not delete or move the default app without first choosing another default browser.

## Development checks

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo run --example native-smoke
```

The native smoke check requires a logged-in graphical macOS session. It briefly shows its own test windows, checks pre-launch/warm URL callbacks, layout, queued requests, stable selection, failure/retry state, cancellation, and long-URL wrapping with bounded window geometry, then exits. It does not launch a browser, save preference changes, or change defaults. Native view snapshots are saved to `target/native-chooser.png`, `target/native-settings.png`, and `target/native-long-url.png`.

Actual browser handoff, Teams/WhatsApp behavior, VoiceOver, keyboard behavior, denied/partial default changes, cold-launch timing, and older macOS versions still need end-to-end checks. The development bundle is not Developer ID signed or notarized.

## Prototype capabilities

- Receive HTTP and HTTPS links as the user-approved default browser.
- Discover locally installed, registered web-link handlers rather than maintain a hard-coded browser list. macOS may also return non-browser apps; exclude those in Settings.
- Show browser names and icons, with mouse and keyboard selection.
- Remember browser exclusions in native macOS preferences.
- Use native controls, semantic colors, system fonts, and native accessibility semantics; VoiceOver verification is still pending.
- Stay lightweight and do no work while idle beyond the native event loop.

Deployment target: macOS 12 or later. Initial checks ran on Apple Silicon with macOS 27.0.1. Older macOS and Intel support require separate verification before being advertised as tested.

## Important limits

- Apps can bypass the default browser by opening an embedded browser, a named browser, or a proprietary URL scheme. ChooBro cannot intercept those without integration from the source app.
- Installing ChooBro does not silently make it the default browser. macOS controls approval, and the user can change the default later in System Settings.
- Browser profiles, private windows, domain rules, link rewriting, and cloud sync are not part of the first release.
- ChooBro is a dispatcher, not a browser engine. Browser launch and page-load time are outside its control.

## Privacy

ChooBro works locally: no telemetry, network requests, or browsing-history storage. Only browser preferences are saved. URLs remain in memory until dispatched or canceled; they are not logged or persisted. Quitting discards pending links after confirmation. The selected browser, not ChooBro, loads the destination.
