use std::cell::{Cell, OnceCell, RefCell};

use block2::RcBlock;
use objc2::{
    DefinedClass, MainThreadOnly, define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, ProtocolObject},
    sel,
};
use objc2_app_kit::*;
use objc2_foundation::{
    MainThreadMarker, NSArray, NSInteger, NSNotification, NSObject, NSObjectProtocol,
    NSOperationQueue, NSURL,
};

use crate::{
    app::{App, Incoming, OpenRequest},
    browsers, ui,
};

thread_local! {
    static DELEGATE: OnceCell<Retained<Delegate>> = const { OnceCell::new() };
}

#[derive(Default)]
pub struct State {
    ready: Cell<bool>,
    app: App,
    chooser: OnceCell<ui::Chooser>,
    settings: RefCell<Option<ui::Settings>>,
    status_item: OnceCell<Retained<NSStatusItem>>,
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
            if self.ivars().app.has_pending() {
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
            match self.ivars().app.receive(urls) {
                Incoming::Show => self.refresh_chooser(true),
                Incoming::Queued => {},
                Incoming::Rejected if self.ivars().ready.get() => self.unsupported_link(),
                Incoming::Rejected => self.ivars().rejected_request.set(true),
            }
        }

        #[unsafe(method(applicationShouldHandleReopen:hasVisibleWindows:))]
        fn reopen(&self, _application: &NSApplication, _visible: bool) -> bool {
            if self.ivars().app.has_pending() {
                self.refresh_chooser(true);
            } else {
                self.show_settings(sel!(showSettings:), None);
            }
            true
        }

        #[unsafe(method(applicationShouldTerminate:))]
        fn should_terminate(&self, _sender: &NSApplication) -> NSApplicationTerminateReply {
            if self.ivars().app.has_pending() && !ui::confirm_quit(self.mtm()) {
                return NSApplicationTerminateReply::TerminateCancel;
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
            self.ivars().app.browser_count() as NSInteger
        }
    }

    unsafe impl NSControlTextEditingDelegate for Delegate {}

    unsafe impl NSTableViewDelegate for Delegate {
        #[unsafe(method_id(tableView:viewForTableColumn:row:))]
        fn row_view(&self, _table: &NSTableView, _column: Option<&NSTableColumn>, row: NSInteger) -> Option<Retained<NSView>> {
            usize::try_from(row).ok().and_then(|row| {
                self.ivars().app.browser(row).map(|browser| ui::browser_cell(&browser, self.mtm()))
            })
        }
    }

    impl Delegate {
        #[unsafe(method(copyLink:))]
        fn copy_link(&self, _sender: Option<&AnyObject>) {
            self.copy_link_to(&NSPasteboard::generalPasteboard());
        }

        #[unsafe(method(showSettings:))]
        fn show_settings(&self, _sender: Option<&AnyObject>) {
            let (browsers, excluded) = self.ivars().app.refresh_settings();
            let settings = ui::settings(self, &browsers, &excluded, self.ivars().app.configuring(), browsers::manual_order());
            let old = self.ivars().settings.borrow_mut().replace(settings);
            if let Some(old) = old {
                if let Some(settings) = &*self.ivars().settings.borrow() {
                    settings.window.setFrame_display(old.window.frame(), false);
                }
                old.window.orderOut(None);
            }
            if let Some(settings) = &*self.ivars().settings.borrow() {
                ui::present(&settings.window, self.mtm());
            }
        }

        #[unsafe(method(showChooser:))]
        fn show_chooser(&self, _sender: Option<&AnyObject>) {
            if self.ivars().app.has_pending() {
                self.refresh_chooser(true);
            } else {
                self.show_settings(sel!(showSettings:), None);
            }
        }

        #[unsafe(method(toggleBrowser:))]
        fn toggle_browser(&self, sender: &NSButton) {
            if let Ok(index) = usize::try_from(sender.tag()) {
                self.ivars().app.set_browser_enabled(index, sender.state() == NSControlStateValueOn);
            }
            self.refresh_chooser(false);
        }

        #[unsafe(method(changeBrowserSort:))]
        fn change_browser_sort(&self, sender: &NSButton) {
            browsers::set_manual_order(sender.state() == NSControlStateValueOn);
            self.show_settings(sel!(showSettings:), None);
            self.refresh_chooser(false);
        }

        #[unsafe(method(moveBrowserUp:))]
        fn move_browser_up(&self, sender: &NSButton) {
            self.move_settings_browser(sender, -1);
        }

        #[unsafe(method(moveBrowserDown:))]
        fn move_browser_down(&self, sender: &NSButton) {
            self.move_settings_browser(sender, 1);
        }

        #[unsafe(method(cancelRequest:))]
        fn cancel_request(&self, _sender: Option<&AnyObject>) {
            self.ivars().app.cancel();
            self.refresh_chooser(true);
        }

        #[unsafe(method(openBrowser:))]
        fn open_browser(&self, _sender: Option<&AnyObject>) {
            let Some(chooser) = self.ivars().chooser.get() else { return };
            let Ok(index) = usize::try_from(chooser.table.selectedRow()) else { return };
            match self.ivars().app.open(index) {
                OpenRequest::None => {},
                OpenRequest::Unavailable => {
                    self.refresh_chooser(false);
                    chooser.browser_unavailable();
                },
                OpenRequest::Ready(browser, urls) => {
                    self.set_busy(true);
                    let name = browser.name.clone();
                    browsers::open(&browser, &urls, move |success| {
                        let name = name.clone();
                        on_main(move || with_delegate(|delegate| delegate.dispatch_finished(success, &name)));
                    });
                },
            }
        }

        #[unsafe(method(makeDefault:))]
        fn make_default(&self, _sender: Option<&AnyObject>) {
            if self.ivars().app.configuring() {
                return;
            }
            if !browsers::is_app_bundle() {
                ui::alert("Launch the app bundle first", "Build and open ChooBro.app before requesting a default-browser change.", self.mtm());
                return;
            }
            if !self.ivars().app.begin_default_change() { return; }
            if let Some(settings) = &*self.ivars().settings.borrow() {
                settings.set_status(true, "Waiting for macOS approval…");
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
    fn move_settings_browser(&self, sender: &NSButton, offset: isize) {
        if let Ok(index) = usize::try_from(sender.tag())
            && self.ivars().app.move_browser(index, offset)
        {
            self.show_settings(sel!(showSettings:), None);
            self.refresh_chooser(false);
            if let Some(settings) = &*self.ivars().settings.borrow() {
                settings.focus_browser_move(index.checked_add_signed(offset).unwrap(), offset);
            }
        }
    }

    fn copy_link_to(&self, pasteboard: &NSPasteboard) {
        let url = self.ivars().app.preview();
        let Some(url) = url else { return };
        if let Some(chooser) = self.ivars().chooser.get() {
            chooser.copy_link(pasteboard, &url);
        }
    }

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
        if self.ivars().app.busy() {
            return;
        }
        let selected = usize::try_from(chooser.table.selectedRow()).ok();
        let content = self.ivars().app.refresh(selected);
        chooser.update(
            content.as_ref(),
            self.ivars().app.browser_count(),
            present,
            self.mtm(),
        );
    }

    fn set_busy(&self, busy: bool) {
        if let Some(chooser) = self.ivars().chooser.get() {
            chooser.set_busy(busy, self.ivars().app.browser_count());
        }
    }

    fn dispatch_finished(&self, success: bool, browser: &str) {
        let present = self.ivars().app.dispatch_finished(success);
        self.refresh_chooser(present);
        if !success && let Some(chooser) = self.ivars().chooser.get() {
            chooser.dispatch_failed(browser, self.mtm());
        }
    }

    fn set_default_scheme(&self, index: usize) {
        browsers::set_default_scheme(index, move |failed| {
            on_main(move || {
                with_delegate(|delegate| {
                    if let Some(status) =
                        delegate.ivars().app.default_scheme_finished(index, failed)
                    {
                        if let Some(settings) = &*delegate.ivars().settings.borrow() {
                            settings.set_status(false, &status);
                        }
                    } else {
                        delegate.set_default_scheme(1);
                    }
                })
            });
        });
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
