use std::{
    cell::{Cell, OnceCell, RefCell},
    collections::BTreeSet,
    path::Path,
};

use block2::RcBlock;
use objc2::{
    DefinedClass, MainThreadOnly, define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, ProtocolObject},
    sel,
};
use objc2_app_kit::*;
use objc2_foundation::{
    MainThreadMarker, NSArray, NSBundle, NSError, NSIndexSet, NSInteger, NSNotification, NSObject,
    NSObjectProtocol, NSOperationQueue, NSURL, ns_string,
};

use crate::{
    browsers::{self, Browser},
    requests::Requests,
    ui,
};

thread_local! {
    static DELEGATE: OnceCell<Retained<Delegate>> = const { OnceCell::new() };
}

#[derive(Default)]
pub struct State {
    ready: Cell<bool>,
    requests: RefCell<Requests>,
    browsers: RefCell<Vec<Browser>>,
    settings_browsers: RefCell<Vec<Browser>>,
    chooser: OnceCell<ui::Chooser>,
    settings: RefCell<Option<ui::Settings>>,
    status_item: OnceCell<Retained<NSStatusItem>>,
    configuring: Cell<bool>,
    default_failed: Cell<bool>,
    rejected_request: Cell<bool>,
}

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = State]
    pub struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl NSApplicationDelegate for Delegate {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn did_finish_launching(&self, _notification: &NSNotification) {
            let app = NSApplication::sharedApplication(self.mtm());
            app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
            let _ = self.ivars().chooser.set(ui::chooser(self));
            let _ = self.ivars().status_item.set(ui::menus(self));
            self.ivars().ready.set(true);
            if self.ivars().requests.borrow().current().is_some() {
                self.refresh_chooser(true);
            } else {
                self.show_settings(sel!(showSettings:), None);
            }
            if self.ivars().rejected_request.replace(false) {
                self.unsupported_link();
            }
        }

        #[unsafe(method(application:openURLs:))]
        fn open_urls(&self, _application: &NSApplication, urls: &NSArray<NSURL>) {
            let (accepted, already_pending) = {
                let mut requests = self.ivars().requests.borrow_mut();
                let already_pending = requests.current().is_some();
                (requests.push(urls), already_pending)
            };
            if accepted {
                if !already_pending {
                    self.refresh_chooser(true);
                }
            } else if self.ivars().ready.get() {
                self.unsupported_link();
            } else {
                self.ivars().rejected_request.set(true);
            }
        }

        #[unsafe(method(applicationShouldHandleReopen:hasVisibleWindows:))]
        fn reopen(&self, _application: &NSApplication, _visible: bool) -> bool {
            if self.ivars().requests.borrow().current().is_some() {
                self.refresh_chooser(true);
            } else {
                self.show_settings(sel!(showSettings:), None);
            }
            true
        }

        #[unsafe(method(applicationShouldTerminate:))]
        fn should_terminate(&self, _sender: &NSApplication) -> NSApplicationTerminateReply {
            let pending = self.ivars().requests.borrow().current().is_some();
            if pending {
                let alert = NSAlert::new(self.mtm());
                alert.setMessageText(ns_string!("Quit with pending links?"));
                alert.setInformativeText(ns_string!("Pending links are kept only in memory and will be discarded."));
                alert.addButtonWithTitle(ns_string!("Keep ChooBro Open"));
                alert.addButtonWithTitle(ns_string!("Quit"));
                if alert.runModal() != NSAlertSecondButtonReturn {
                    return NSApplicationTerminateReply::TerminateCancel;
                }
            }
            NSApplicationTerminateReply::TerminateNow
        }
    }

    unsafe impl NSWindowDelegate for Delegate {
        #[unsafe(method(windowShouldClose:))]
        fn should_close(&self, sender: &NSWindow) -> bool {
            if self.ivars().chooser.get().is_some_and(|chooser| &*chooser.window == sender) {
                self.cancel_request(sel!(cancelRequest:), None);
                false
            } else {
                true
            }
        }
    }

    unsafe impl NSTableViewDataSource for Delegate {
        #[unsafe(method(numberOfRowsInTableView:))]
        fn number_of_rows(&self, _table: &NSTableView) -> NSInteger {
            self.ivars().browsers.borrow().len() as NSInteger
        }
    }

    unsafe impl NSControlTextEditingDelegate for Delegate {}

    unsafe impl NSTableViewDelegate for Delegate {
        #[unsafe(method_id(tableView:viewForTableColumn:row:))]
        fn row_view(&self, _table: &NSTableView, _column: Option<&NSTableColumn>, row: NSInteger) -> Option<Retained<NSView>> {
            usize::try_from(row).ok().and_then(|row| {
                self.ivars().browsers.borrow().get(row).map(|browser| ui::browser_cell(browser, self.mtm()))
            })
        }
    }

    impl Delegate {
        #[unsafe(method(showSettings:))]
        fn show_settings(&self, _sender: Option<&AnyObject>) {
            let browsers = browsers::discover(&BTreeSet::from(["http".into(), "https".into()]));
            let settings = ui::settings(self, &browsers, &browsers::exclusions(), self.ivars().configuring.get());
            *self.ivars().settings_browsers.borrow_mut() = browsers;
            if let Some(old) = self.ivars().settings.borrow_mut().replace(settings) {
                old.window.orderOut(None);
            }
            if let Some(settings) = &*self.ivars().settings.borrow() {
                ui::present(&settings.window, self.mtm());
            }
        }

        #[unsafe(method(showChooser:))]
        fn show_chooser(&self, _sender: Option<&AnyObject>) {
            if self.ivars().requests.borrow().current().is_some() {
                self.refresh_chooser(true);
            } else {
                self.show_settings(sel!(showSettings:), None);
            }
        }

        #[unsafe(method(toggleBrowser:))]
        fn toggle_browser(&self, sender: &NSButton) {
            if let Ok(index) = usize::try_from(sender.tag())
                && let Some(browser) = self.ivars().settings_browsers.borrow().get(index) {
                browsers::set_excluded(&browser.id, sender.state() != NSControlStateValueOn);
            }
            self.refresh_chooser(false);
        }

        #[unsafe(method(cancelRequest:))]
        fn cancel_request(&self, _sender: Option<&AnyObject>) {
            self.ivars().requests.borrow_mut().cancel();
            self.refresh_chooser(true);
        }

        #[unsafe(method(openBrowser:))]
        fn open_browser(&self, _sender: Option<&AnyObject>) {
            let Some(chooser) = self.ivars().chooser.get() else { return };
            let Ok(index) = usize::try_from(chooser.table.selectedRow()) else { return };
            let Some(browser) = self.ivars().browsers.borrow().get(index).cloned() else { return };
            let available = Path::new(&browser.path).is_dir()
                && NSBundle::bundleWithURL(&browser.url).and_then(|bundle| bundle.bundleIdentifier()).is_some_and(|id| id.to_string() == browser.id);
            if !available {
                self.refresh_chooser(false);
                chooser.message.setStringValue(ns_string!("That browser is no longer available. Choose another browser."));
                return;
            }
            let Some(urls) = self.ivars().requests.borrow_mut().begin() else { return };
            self.set_busy(true);
            let name = browser.name;
            let completion = RcBlock::new(move |_app: *mut NSRunningApplication, error: *mut NSError| {
                let success = error.is_null();
                let name = name.clone();
                on_main(move || with_delegate(|delegate| delegate.dispatch_finished(success, &name)));
            });
            let configuration = NSWorkspaceOpenConfiguration::new();
            configuration.setActivates(true);
            NSWorkspace::sharedWorkspace().openURLs_withApplicationAtURL_configuration_completionHandler(&urls, &browser.url, &configuration, Some(&completion));
        }

        #[unsafe(method(makeDefault:))]
        fn make_default(&self, _sender: Option<&AnyObject>) {
            if self.ivars().configuring.replace(true) {
                return;
            }
            let bundled = NSBundle::mainBundle().bundleIdentifier().is_some_and(|id| id.to_string() == browsers::BUNDLE_ID);
            if !bundled {
                self.ivars().configuring.set(false);
                ui::alert("Launch the app bundle first", "Build and open ChooBro.app before requesting a default-browser change.", self.mtm());
                return;
            }
            self.ivars().default_failed.set(false);
            if let Some(settings) = &*self.ivars().settings.borrow() {
                settings.make_default.setEnabled(false);
                settings.status.setStringValue(ns_string!("Waiting for macOS approval…"));
            }
            self.set_default_scheme(0);
        }

        #[unsafe(method(quit:))]
        fn quit(&self, _sender: Option<&AnyObject>) {
            NSApplication::sharedApplication(self.mtm()).terminate(None);
        }
    }
);

impl Delegate {
    fn unsupported_link(&self) {
        ui::alert(
            "Unsupported link",
            "ChooBro accepts only absolute HTTP and HTTPS links with a host. This request was not opened.",
            self.mtm(),
        );
    }

    fn refresh_chooser(&self, present: bool) {
        let Some(chooser) = self.ivars().chooser.get() else {
            return;
        };
        let (schemes, preview, count, busy) = {
            let requests = self.ivars().requests.borrow();
            let Some(urls) = requests.current() else {
                chooser.window.orderOut(None);
                chooser.preview.setStringValue(ns_string!(""));
                chooser.preview.setToolTip(None);
                return;
            };
            let schemes = urls
                .iter()
                .filter_map(|url| {
                    url.scheme()
                        .map(|value| value.to_string().to_ascii_lowercase())
                })
                .collect();
            (
                schemes,
                urls.objectAtIndex(0)
                    .absoluteString()
                    .unwrap_or_else(|| ns_string!("Web link").into()),
                urls.len(),
                requests.in_flight,
            )
        };
        if busy {
            return;
        }
        let selected_path = usize::try_from(chooser.table.selectedRow())
            .ok()
            .and_then(|index| {
                self.ivars()
                    .browsers
                    .borrow()
                    .get(index)
                    .map(|browser| browser.path.clone())
            });
        let excluded = browsers::exclusions();
        let browsers: Vec<_> = browsers::discover(&schemes)
            .into_iter()
            .filter(|browser| !excluded.contains(&browser.id))
            .collect();
        let empty = browsers.is_empty();
        let selected_row = browsers
            .iter()
            .position(|browser| Some(&browser.path) == selected_path.as_ref())
            .unwrap_or(0);
        *self.ivars().browsers.borrow_mut() = browsers;
        chooser.preview.setStringValue(&preview);
        chooser.preview.setToolTip(Some(&preview));
        chooser.table.reloadData();
        if !empty {
            chooser.table.selectRowIndexes_byExtendingSelection(
                &NSIndexSet::indexSetWithIndex(selected_row),
                false,
            );
        }
        chooser
            .message
            .setStringValue(&objc2_foundation::NSString::from_str(&if empty {
                "No enabled browsers. Open Settings to enable or refresh browsers.".into()
            } else if count > 1 {
                format!("Open all {count} links in the selected browser.")
            } else {
                "Use ↑ ↓ to choose, Return to open, or Escape to cancel.".into()
            }));
        self.set_busy(false);
        if present {
            ui::present(&chooser.window, self.mtm());
            chooser.window.makeFirstResponder(Some(&chooser.table));
        }
    }

    fn set_busy(&self, busy: bool) {
        if let Some(chooser) = self.ivars().chooser.get() {
            chooser
                .open
                .setEnabled(!busy && !self.ivars().browsers.borrow().is_empty());
            chooser.cancel.setEnabled(!busy);
            chooser.table.setEnabled(!busy);
            if busy {
                chooser
                    .message
                    .setStringValue(ns_string!("Opening in your browser…"));
            }
        }
    }

    fn dispatch_finished(&self, success: bool, browser: &str) {
        self.ivars().requests.borrow_mut().finish(success);
        let pending = self.ivars().requests.borrow().current().is_some();
        self.refresh_chooser(success && pending);
        if !success && let Some(chooser) = self.ivars().chooser.get() {
            chooser
                .message
                .setStringValue(&objc2_foundation::NSString::from_str(&format!(
                    "Could not open {browser}. Choose another browser or try again."
                )));
            ui::present(&chooser.window, self.mtm());
        }
    }

    fn set_default_scheme(&self, index: usize) {
        let scheme = [ns_string!("http"), ns_string!("https")][index];
        let completion = RcBlock::new(move |error: *mut NSError| {
            let failed = !error.is_null();
            on_main(move || {
                with_delegate(|delegate| {
                    if failed {
                        delegate.ivars().default_failed.set(true);
                    }
                    if index == 0 {
                        delegate.set_default_scheme(1);
                    } else {
                        delegate.ivars().configuring.set(false);
                        if let Some(settings) = &*delegate.ivars().settings.borrow() {
                            settings.make_default.setEnabled(true);
                            let mut status = browsers::default_status();
                            if delegate.ivars().default_failed.get() {
                                status.push_str("\nA change was declined or failed. You can use System Settings.");
                            }
                            settings
                                .status
                                .setStringValue(&objc2_foundation::NSString::from_str(&status));
                        }
                    }
                })
            });
        });
        NSWorkspace::sharedWorkspace()
            .setDefaultApplicationAtURL_toOpenURLsWithScheme_completionHandler(
                &NSBundle::mainBundle().bundleURL(),
                scheme,
                Some(&completion),
            );
    }
}

fn with_delegate(f: impl FnOnce(&Delegate)) {
    DELEGATE.with(|cell| {
        if let Some(delegate) = cell.get() {
            f(delegate);
        }
    });
}

fn on_main(f: impl Fn() + Send + 'static) {
    let block = RcBlock::new(f);
    unsafe { NSOperationQueue::mainQueue().addOperationWithBlock(&block) };
}

pub fn run() {
    std::panic::set_hook(Box::new(|info| {
        eprintln!(
            "ChooBro encountered an internal error at {:?} and must stop.",
            info.location()
        )
    }));
    let mtm = MainThreadMarker::new().expect("application starts on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    let delegate = Delegate::alloc(mtm).set_ivars(State::default());
    let delegate: Retained<Delegate> = unsafe { msg_send![super(delegate), init] };
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    DELEGATE.with(|cell| {
        let _ = cell.set(delegate);
    });
    app.run();
}
