# ChooBro architecture

Status: initial native prototype implemented on October 28, 2026, following the design below. Unit tests and a native callback/layout smoke check pass on Apple Silicon with macOS 27.0.1; warm Launch Services delivery to the built bundle was also observed. Real browser handoff, accessibility interaction, default-handler consent edge cases, older OS versions, distribution signing, and performance budgets are not yet verified.

## Decision: Rust with native AppKit

Use Rust for application logic and UI construction, with `objc2` bindings to Apple's AppKit and Foundation frameworks. The chooser and settings use native macOS controls.

This satisfies the native-components option in the project requirements. AppKit participates in the system's accelerated window composition and can use layer-backed views, but not every control or drawing operation runs on the GPU. ChooBro does not need a custom Metal renderer for a short browser list. Actual startup and input latency must be measured.

### Alternatives researched

| Option | Rendering and controls | Decision |
| --- | --- | --- |
| AppKit through `objc2` | Native macOS controls and system-managed rendering | Recommended: closest platform fit, existing accessibility and appearance behavior, direct access to required OS APIs. Cost: explicit Objective-C ownership and callback handling from Rust. |
| GPUI | GPU-accelerated Rust UI; Metal backend on macOS | Good for substantial custom interfaces, but not AppKit controls. Unnecessary rendering and styling responsibility for this app. |
| Iced | Rust UI with a `wgpu` renderer supporting Metal, plus a software renderer | Useful for cross-platform apps; widgets are not native AppKit controls. Cross-platform support is not required. |
| SwiftUI with a Rust core | Native Apple UI with a separate Swift layer | Viable technically, but adds a second application language and a bridge that this small, Rust-first app does not need. |
| Tauri or Electron | Web-based UI | Rejected: conflicts with the no-HTML/WebView requirement. |

Rust is a project requirement, not proof of performance. For this utility, process startup, app discovery, icon loading, and OS event handling matter more than sustained rendering throughput.

## Intended stack

- Stable Rust, Cargo, one binary crate, and the standard library.
- `objc2` for Objective-C interop; `objc2-app-kit` and `objc2-foundation` for native APIs.
- `block2` for APIs using completion blocks, when needed.
- `NSUserDefaults` for a small set of browser preferences; no serialization library or database initially.
- A conventional `.app` bundle containing the executable, `Info.plist`, and icon resources.
- Proposed deployment target: macOS 12.0+, because the selected public default-handler API starts at macOS 12. Apple Silicon is the first verification target; Intel is not promised until tested.

The initial `Cargo.lock` resolves `objc2` 0.6.5 and `objc2-app-kit`/`objc2-foundation` 0.3.2, with `block2` 0.6.2. Rust is pinned to 1.91.1. Application profiles abort on panic so Rust unwinding cannot cross native callbacks; the panic hook omits payloads to avoid exposing URL data.

No async runtime, GPU crate, browser engine, helper daemon, or additional UI framework is needed initially.

## Application lifecycle and link flow

1. Launch Services delivers HTTP/HTTPS URLs to `NSApplicationDelegate` via `application:openURLs:`. Do not assume links arrive as command-line arguments.
2. Retain requests even if they arrive before UI initialization completes. Create the UI after the app has finished launching.
3. Validate schemes and hosts without rewriting the incoming URLs. Retain valid requests in memory only.
4. Discover eligible browser applications and apply saved exclusions.
5. Present one compact chooser. A browser selection forwards that request through the explicit-application `NSWorkspace` API.
6. On successful handoff, remove the request and show the next pending request, if any. A launch failure keeps the request available and displays an actionable error.
7. Close the chooser when nothing is pending. Keep the process available through AppKit's event loop until the user quits; this avoids repeated process startup without a separate daemon or login item. A direct app launch with no pending link opens settings.

One delegate delivery containing several URLs is one request, opened together in the selected browser. Later deliveries queue in arrival order, including deliveries received while another launch is in flight. Prevent duplicate submission of an in-flight request. Escape cancels only the displayed request; it must not silently discard unrelated queued requests.

Use accessory-style activation to avoid a persistent Dock presence, with a native menu-bar item exposing Settings and Quit. Validate keyboard focus and foreground activation in the first native prototype; a chooser must become usable immediately without stealing focus again after dispatch.

## Browser discovery and forwarding

Use `NSWorkspace.URLsForApplicationsToOpenURL` with fixed HTTP and HTTPS probe URLs to ask Launch Services for registered handlers. This is local discovery, not a network request. Filter candidates for the actual request's schemes.

- Do not scan the entire disk or maintain a fixed Safari/Chrome/Firefox list.
- Treat returned applications as registered web-link handlers, not proof they are full browsers. Other browser choosers may appear too.
- Remove ChooBro by its own bundle identifier and bundle location. Reject stale or unavailable bundles.
- Deduplicate application locations, show the native display name and app icon, and use a stable name-based order. Distinguish duplicate names when necessary.
- Save exclusions by bundle identifier so app updates or moves do not reset preferences. Excluding an identifier excludes every copy with that identifier. If a candidate has no usable identifier, omit it initially rather than store a fragile identity.
- Query on presentation and when settings refreshes. Retain icons with the current browser list; rely on native icon lookup rather than adding a persistent browser cache. Later queued deliveries do not rescan or reset an already-presented choice.

Forward using `NSWorkspace.openURLs(_:withApplicationAt:configuration:completionHandler:)` and the selected bundle URL. This bypasses the default handler and prevents self-recursion. Check completion errors and marshal UI updates back to the main thread. Successful handoff does not prove the browser loaded the page.

If all browsers are excluded or none are available, show a clear empty state with access to settings. Do not silently pick a hidden browser. Settings must remain reachable without receiving a link.

## Default-browser integration and packaging

The bundle declares HTTP and HTTPS under `CFBundleURLTypes`/`CFBundleURLSchemes`, with bundle identifier `io.github.miguelcaravantes.choobro`, based on the project's GitHub owner.

Offer an explicit Make Default Browser action using `NSWorkspace.setDefaultApplication(at:toOpenURLsWithScheme:completion:)` for both schemes. macOS requests consent where required. Cancellation is normal, and partial success must be reported: verify each scheme's current handler instead of assuming one successful callback configured both.

Being installed, declaring URL handlers, and being the default browser are separate states. Also provide manual System Settings guidance if the request fails or policy prevents a change. Do not edit Launch Services databases or rely on deprecated command-line registration hacks.

`sh scripts/build-app.sh` builds a locked release binary, assembles `dist/ChooBro.app`, validates its plist, and applies an ad-hoc signature for local development. It replaces the executable atomically rather than overwriting an in-use binary. A bare Cargo executable is not the release artifact.

For distribution outside the App Store, plan Developer ID signing, hardened runtime, notarization, and stapling. Local development can use a development bundle; it must not be described as ready for general distribution. App Store sandboxing and approval are not part of the initial distribution plan.

## Native UI and accessibility

- A compact native window with browser icons, readable names, and standard controls; no imitation web-style macOS theme.
- System fonts, semantic colors, Auto Layout, light/dark appearance, and native focus indication.
- Arrow-key navigation, Return to choose, Escape to cancel, and standard Settings/Quit shortcuts where applicable.
- Native accessibility labels and a tested VoiceOver reading/focus order. Native controls help but do not make accessibility testing optional.
- A fixed-width chooser with character-wrapped URL preview, capped at three lines, and the complete URL in its tooltip. Wrapping changes presentation only, not the forwarded URL. Never load a page preview or fetch a favicon. URLs remain untrusted text.
- A simple settings list with inclusion toggles and a refresh action. Never require hand-editing a preference file.

## Privacy, safety, and boundaries

Accept only absolute HTTP/HTTPS URLs with a host. Do not register custom schemes or HTML file associations in the first release. Preserve query strings, fragments, and percent encoding when forwarding; if Foundation normalizes a representation, test that its meaning is preserved.

Do not persist pending URLs, browsing history, or source-app identity. Do not log full URLs, including in error paths. Error messages should identify the failed browser without leaking URL credentials or tokens into diagnostics. Cancellation and Quit intentionally abandon affected in-memory requests; warn before Quit if requests remain.

ChooBro does not read messages, monitor the clipboard, inspect browser profiles, or require privileged interception. Links opened internally by Teams, WhatsApp, or another app will not reach it. Managed macOS or application policies may also prevent default-browser changes or bypass system defaults.

## Rendering and capture investigation

During initial testing, an unbroken Teams-style URL expanded the chooser to 20,708 points wide. The shared window layout now constrains its content width, and the URL preview uses a wrapping text field rather than a single-line label. Both windows use native layer backing. Long-URL layout is covered by the native smoke check; moving-window corruption and Shottr area capture still require user confirmation after restarting the rebuilt app.

No matching `objc2` Shottr/rendering issue was found in the public tracker during this investigation. [Shottr's changelog](https://shottr.cc/newversion.html) lists macOS 27 compatibility changes in 1.9.2/1.9.3, and its [custom area grabber](https://shottr.cc/kb/customareacapture) is documented as work in progress. [AltTab issue 5757](https://github.com/lwouis/alt-tab-macos/issues/5757) documents a macOS 27 glass-panel corner artifact, but ChooBro does not use that custom glass panel; this is related evidence, not a confirmed cause or a reason to copy its clipping workaround.

## Performance validation

Use an event-driven AppKit loop with no idle polling or rendering timer. Avoid subprocesses, network work, eager large assets, and background indexing on the URL-to-chooser path. Do not move AppKit work to a worker thread to hide latency.

Measure separately:

- Cold launch: OS launch request to chooser ready for input, plus process-entry-to-ready instrumentation.
- Warm delivery: URL callback to chooser ready for input.
- Dispatch: browser selection to `NSWorkspace` completion, separately for running and stopped browsers.
- Idle CPU and resident memory after the chooser is dismissed.

Capture release-build median and p95 across repeated trials, documenting hardware, macOS version, browser count, and measurement method. Start with a provisional warm callback-to-ready p95 target below 100 ms; it is a target, not a measured claim. Establish a cold-launch baseline before setting a defensible budget. Browser startup and page loading are separate from chooser latency.

## Implementation sequence

1. Create a pinned Rust project and minimal `.app` bundle. Prove cold/warm URL event delivery and explicit browser dispatch without changing the system default automatically.
2. Add browser discovery and an accessible chooser, including queued requests, cancellation, missing-browser handling, and launch errors.
3. Add native settings, saved exclusions, menu-bar access, and user-approved default-browser setup with status verification.
4. Measure performance, verify native behavior across supported macOS versions, and prepare signed/notarized distribution.

Keep modules small and concrete: entry/delegate, chooser/settings, browser integration, and preferences. Split files as the code grows; do not create a multi-crate workspace or generic platform abstraction upfront.

Unit tests should cover scheme/host validation, self-exclusion and filtering, preference behavior, and request queue transitions. Native bundle tests must cover pre-launch URL delivery, bursts during an in-flight launch, browser removal, all-excluded state, focus, VoiceOver, appearance, user-denied default changes, and full URL preservation. Perform real Teams/WhatsApp checks rather than promising every source app behaves alike.

## Primary sources

- [`objc2`: interop, framework crates, and a Rust AppKit example](https://docs.rs/objc2/latest/objc2/).
- [`objc2-app-kit`: typed `NSWorkspace` APIs](https://docs.rs/objc2-app-kit/latest/objc2_app_kit/struct.NSWorkspace.html).
- [Apple: `NSApplicationDelegate.application(_:open:)`](https://developer.apple.com/documentation/appkit/nsapplicationdelegate/application(_:open:)).
- [Apple: `CFBundleURLTypes`](https://developer.apple.com/documentation/bundleresources/information-property-list/cfbundleurltypes).
- [Apple: `NSWorkspace` discovery and explicit application opening](https://developer.apple.com/documentation/appkit/nsworkspace).
- [Apple: setting a default scheme handler, availability, and user consent](https://developer.apple.com/documentation/appkit/nsworkspace/setdefaultapplication(at:toopenurlswithscheme:completion:)).
- [GPUI: GPU-accelerated Rust framework](https://www.gpui.rs/) and [macOS backend source](https://github.com/zed-industries/zed/tree/main/crates/gpui/src/platform/mac).
- [Iced: renderers and platform support](https://github.com/iced-rs/iced).

These sources establish API capabilities and architectural tradeoffs, not ChooBro's performance or production compatibility. The prototype proves basic native lifecycle and layout; consent behavior, deployment-target compatibility, actual browser dispatch, and latency still require end-to-end validation.
