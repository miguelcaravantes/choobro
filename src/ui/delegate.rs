use std::cell::{Cell, OnceCell, RefCell};

use block2::RcBlock;
use objc2::{
    DefinedClass, MainThreadOnly, define_class, msg_send,
    rc::{Retained, autoreleasepool},
    runtime::{AnyObject, ProtocolObject},
    sel,
};
use objc2_app_kit::*;
use objc2_foundation::{
    MainThreadMarker, NSArray, NSInteger, NSNotification, NSObject, NSObjectProtocol,
    NSOperationQueue, NSString, NSTimer, NSURL, ns_string,
};

use crate::{
    app::{App, Incoming, OpenRequest},
    browsers,
    requests::Rejection,
    ui,
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
    rejected_request: Cell<Option<Rejection>>,
    showing_rejection: Cell<bool>,
    timeout: RefCell<Option<Retained<NSTimer>>>,
    timeout_monitor: RefCell<Option<Retained<AnyObject>>>,
    updating_chooser: Cell<bool>,
    timeout_browser: RefCell<Option<String>>,
}

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = State]
    pub struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl NSApplicationDelegate for Delegate {
        #[unsafe(method(applicationDidBecomeActive:))]
        fn did_become_active(&self, _notification: &NSNotification) {
            self.refresh_default_status();
        }

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
            if let Some(reason) = self.ivars().rejected_request.take() {
                self.rejected_link(reason);
            }
        }

        #[unsafe(method(application:openURLs:))]
        fn open_urls(&self, _application: &NSApplication, urls: &NSArray<NSURL>) {
            match self.ivars().app.receive(urls) {
                Incoming::Show => self.refresh_chooser(true),
                Incoming::Queued => {},
                Incoming::Rejected(reason) if self.ivars().ready.get() => self.rejected_link(reason),
                Incoming::Rejected(reason) => self.ivars().rejected_request.set(Some(reason)),
            }
        }

        #[unsafe(method(applicationShouldHandleReopen:hasVisibleWindows:))]
        fn reopen(&self, _application: &NSApplication, _visible: bool) -> bool {
            self.cancel_timeout();
            if self.ivars().app.has_pending() {
                self.refresh_chooser(true);
            } else {
                self.show_settings(sel!(showSettings:), None);
            }
            true
        }

        #[unsafe(method(applicationShouldTerminate:))]
        fn should_terminate(&self, _sender: &NSApplication) -> NSApplicationTerminateReply {
            self.cancel_timeout();
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
        fn number_of_rows(&self, table: &NSTableView) -> NSInteger {
            if is_settings_table(table) {
                self.ivars().app.settings_browser_count() as NSInteger
            } else {
                self.ivars().app.browser_count() as NSInteger
            }
        }

        #[unsafe(method_id(tableView:pasteboardWriterForRow:))]
        fn browser_drag_writer(&self, table: &NSTableView, row: NSInteger) -> Option<Retained<ProtocolObject<dyn NSPasteboardWriting>>> {
            self.browser_drag_item(table, row)
        }

        #[unsafe(method(tableView:validateDrop:proposedRow:proposedDropOperation:))]
        fn validate_browser_drop(&self, table: &NSTableView, info: &ProtocolObject<dyn NSDraggingInfo>, row: NSInteger, _operation: NSTableViewDropOperation) -> NSDragOperation {
            if self.browser_drop(table, info, row, NSTableViewDropOperation::Above).is_none() {
                return NSDragOperation::None;
            }
            table.setDropRow_dropOperation(row, NSTableViewDropOperation::Above);
            NSDragOperation::Move
        }

        #[unsafe(method(tableView:acceptDrop:row:dropOperation:))]
        fn accept_browser_drop(&self, table: &NSTableView, info: &ProtocolObject<dyn NSDraggingInfo>, row: NSInteger, operation: NSTableViewDropOperation) -> bool {
            self.perform_browser_drop(table, info, row, operation)
        }
    }

    unsafe impl NSControlTextEditingDelegate for Delegate {
        #[unsafe(method(controlTextDidEndEditing:))]
        fn text_editing_ended(&self, _notification: &NSNotification) {
            self.save_timeout();
        }
    }

    unsafe impl NSTextFieldDelegate for Delegate {}

    unsafe impl NSTableViewDelegate for Delegate {
        #[unsafe(method_id(tableView:viewForTableColumn:row:))]
        fn row_view(&self, table: &NSTableView, column: Option<&NSTableColumn>, row: NSInteger) -> Option<Retained<NSView>> {
            usize::try_from(row).ok().and_then(|row| {
                if is_settings_table(table) {
                    let browser = self.ivars().app.settings_browser(row)?;
                    if column.is_some_and(|column| &*column.identifier() == ns_string!("enabled")) {
                        let enabled = !browsers::exclusions().contains(&browser.id);
                        Some(ui::browser_toggle(&browser, row, enabled, table, self))
                    } else {
                        Some(ui::browser_cell(&browser, table, 0.0))
                    }
                } else {
                    self.ivars().app.browser(row).map(|browser| ui::browser_cell(&browser, table, 8.0))
                }
            })
        }

        #[unsafe(method(tableViewSelectionDidChange:))]
        fn selection_changed(&self, notification: &NSNotification) {
            if !self.ivars().updating_chooser.get()
                && notification.object().and_then(|object| object.downcast::<NSTableView>().ok())
                    .is_some_and(|table| !is_settings_table(&table))
            {
                self.cancel_timeout();
            }
            if notification.object().and_then(|object| object.downcast::<NSTableView>().ok())
                .is_some_and(|table| is_settings_table(&table))
                && let Some(settings) = &*self.ivars().settings.borrow()
            {
                settings.update_move_buttons();
            }
        }
    }

    impl Delegate {
        #[unsafe(method(changeTimeout:))]
        fn change_timeout(&self, _sender: Option<&AnyObject>) {
            self.save_timeout();
        }

        #[unsafe(method(timeoutFired:))]
        fn timeout_fired(&self, timer: &NSTimer) {
            if !self.ivars().timeout.borrow().as_ref().is_some_and(|current| &**current == timer) {
                return;
            }
            let id = self.ivars().timeout_browser.borrow().clone();
            self.cancel_timeout();
            let Some(chooser) = self.ivars().chooser.get() else { return };
            if !chooser.window.isVisible() || self.ivars().app.busy() { return; }
            let preference = browsers::AutoOpen::load();
            if !preference.enabled || id.as_deref() != Some(&preference.browser_id)
                || browsers::exclusions().contains(&preference.browser_id) { return; }
            if let Some(index) = id.and_then(|id| self.ivars().app.timeout_browser(&id)) {
                self.dispatch_browser(index);
            }
        }

        #[unsafe(method(copyLink:))]
        fn copy_link(&self, _sender: Option<&AnyObject>) {
            self.copy_link_to(&NSPasteboard::generalPasteboard());
        }

        #[unsafe(method(showSettings:))]
        fn show_settings(&self, _sender: Option<&AnyObject>) {
            self.cancel_timeout();
            let selected = self.settings_selection();
            self.refresh_settings(selected);
            self.refresh_chooser(false);
            if let Some(settings) = &*self.ivars().settings.borrow() {
                ui::present(&settings.window, self.mtm());
            }
        }

        #[unsafe(method(showChooser:))]
        fn show_chooser(&self, _sender: Option<&AnyObject>) {
            self.cancel_timeout();
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
                self.update_settings(index);
            }
            self.refresh_chooser(false);
        }

        #[unsafe(method(changeBrowserSort:))]
        fn change_browser_sort(&self, sender: &NSPopUpButton) {
            browsers::set_manual_order(sender.indexOfSelectedItem() == 1);
            self.refresh_settings(self.settings_selection());
            self.refresh_chooser(false);
        }

        #[unsafe(method(moveBrowserUp:))]
        fn move_browser_up(&self, _sender: &NSButton) {
            self.move_settings_browser(-1);
        }

        #[unsafe(method(moveBrowserDown:))]
        fn move_browser_down(&self, _sender: &NSButton) {
            self.move_settings_browser(1);
        }

        #[unsafe(method(cancelRequest:))]
        fn cancel_request(&self, _sender: Option<&AnyObject>) {
            self.cancel_timeout();
            self.ivars().app.cancel();
            self.refresh_chooser(true);
        }

        #[unsafe(method(openBrowser:))]
        fn open_browser(&self, _sender: Option<&AnyObject>) {
            let Some(chooser) = self.ivars().chooser.get() else { return };
            let Ok(index) = usize::try_from(chooser.table.selectedRow()) else { return };
            self.dispatch_browser(index);
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
    fn save_timeout(&self) {
        self.cancel_timeout();
        {
            let settings = self.ivars().settings.borrow();
            let Some(settings) = settings.as_ref() else {
                return;
            };
            let enabled = settings.auto_open.state() == NSControlStateValueOn;
            let seconds = settings
                .timeout_seconds
                .stringValue()
                .to_string()
                .trim()
                .parse::<f64>()
                .ok()
                .filter(|seconds| browsers::valid_timeout(*seconds))
                .or_else(|| (!enabled).then(|| browsers::AutoOpen::load().seconds));
            let Some(seconds) = seconds else {
                settings.timeout_error.setHidden(false);
                return;
            };
            let browser_id = settings
                .timeout_browser
                .selectedItem()
                .and_then(|item| item.representedObject())
                .and_then(|object| object.downcast::<NSString>().ok())
                .map(|id| id.to_string())
                .unwrap_or_else(|| browsers::AutoOpen::load().browser_id);
            browsers::AutoOpen {
                enabled,
                seconds,
                browser_id,
            }
            .save();
        }
        self.update_timeout_settings();
    }

    fn update_timeout_settings(&self) {
        if let Some(settings) = &*self.ivars().settings.borrow() {
            let browsers: Vec<_> = (0..self.ivars().app.settings_browser_count())
                .filter_map(|index| self.ivars().app.settings_browser(index))
                .collect();
            settings.update_timeout(&browsers);
        }
    }

    fn cancel_timeout(&self) {
        self.ivars().app.cancel_timeout();
        if let Some(timer) = self.ivars().timeout.borrow_mut().take() {
            timer.invalidate();
            if let Some(chooser) = self.ivars().chooser.get() {
                chooser.message.setStringValue(ns_string!(
                    "Automatic opening canceled. Choose a browser to open the link."
                ));
            }
        }
        if let Some(monitor) = self.ivars().timeout_monitor.borrow_mut().take() {
            unsafe {
                NSEvent::removeMonitor(&monitor);
            }
        }
        self.ivars().timeout_browser.borrow_mut().take();
    }

    fn start_timeout(&self) {
        if !self.ivars().app.claim_timeout() {
            return;
        }
        let preference = browsers::AutoOpen::load();
        if !preference.enabled
            || self
                .ivars()
                .app
                .timeout_browser(&preference.browser_id)
                .is_none()
        {
            return;
        }
        let monitor = RcBlock::new(|event: std::ptr::NonNull<NSEvent>| {
            let native_event = unsafe { event.as_ref() };
            with_delegate(|delegate| {
                if delegate.ivars().chooser.get().is_some_and(|chooser| {
                    native_event.window(delegate.mtm()).as_deref() == Some(&*chooser.window)
                }) {
                    delegate.cancel_timeout();
                }
            });
            event.as_ptr()
        });
        let token = unsafe {
            NSEvent::addLocalMonitorForEventsMatchingMask_handler(
                NSEventMask::KeyDown
                    | NSEventMask::LeftMouseDown
                    | NSEventMask::RightMouseDown
                    | NSEventMask::OtherMouseDown
                    | NSEventMask::ScrollWheel,
                &monitor,
            )
        };
        let Some(token) = token else { return };
        self.ivars().timeout_monitor.replace(Some(token));
        self.ivars()
            .timeout_browser
            .replace(Some(preference.browser_id.clone()));
        let timer = unsafe {
            NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                preference.seconds,
                self,
                sel!(timeoutFired:),
                None,
                false,
            )
        };
        self.ivars().timeout.replace(Some(timer));
        if let Some(chooser) = self.ivars().chooser.get()
            && let Some(browser) = self
                .ivars()
                .app
                .timeout_browser(&preference.browser_id)
                .and_then(|index| self.ivars().app.browser(index))
        {
            chooser.message.setStringValue(&NSString::from_str(&format!(
                "Opens in {} after {} seconds. Interact to cancel automatic opening.",
                browser.name, preference.seconds
            )));
        }
    }

    fn dispatch_browser(&self, index: usize) {
        self.cancel_timeout();
        match self.ivars().app.open(index) {
            OpenRequest::None => {}
            OpenRequest::Unavailable => {
                self.refresh_chooser(false);
                if let Some(chooser) = self.ivars().chooser.get() {
                    chooser.browser_unavailable();
                }
            }
            OpenRequest::Ready(browser, urls) => {
                self.set_busy(true);
                let name = browser.name.clone();
                browsers::open(&browser, &urls, move |success| {
                    let name = name.clone();
                    on_main(move || {
                        with_delegate(|delegate| delegate.dispatch_finished(success, &name))
                    });
                });
            }
        }
    }

    fn browser_drag_item(
        &self,
        table: &NSTableView,
        row: NSInteger,
    ) -> Option<Retained<ProtocolObject<dyn NSPasteboardWriting>>> {
        if !is_settings_table(table) || !browsers::manual_order() {
            return None;
        }
        let browser = self
            .ivars()
            .app
            .settings_browser(usize::try_from(row).ok()?)?;
        let item = NSPasteboardItem::new();
        if !item.setString_forType(&NSString::from_str(&browser.path), ui::browser_drag_type()) {
            return None;
        }
        Some(ProtocolObject::from_retained(item))
    }

    fn perform_browser_drop(
        &self,
        table: &NSTableView,
        info: &ProtocolObject<dyn NSDraggingInfo>,
        row: NSInteger,
        operation: NSTableViewDropOperation,
    ) -> bool {
        let Some((path, insertion)) = self.browser_drop(table, info, row, operation) else {
            return false;
        };
        let Some(destination) = self.ivars().app.drop_browser(&path, insertion) else {
            return false;
        };
        self.update_settings(destination);
        self.refresh_chooser(false);
        true
    }

    fn browser_drop(
        &self,
        table: &NSTableView,
        info: &ProtocolObject<dyn NSDraggingInfo>,
        row: NSInteger,
        operation: NSTableViewDropOperation,
    ) -> Option<(String, usize)> {
        if !is_settings_table(table) || operation != NSTableViewDropOperation::Above {
            return None;
        }
        let source = info.draggingSource()?.downcast::<NSTableView>().ok()?;
        if !std::ptr::eq(&*source, table) {
            return None;
        }
        let insertion = usize::try_from(row).ok()?;
        let path = info
            .draggingPasteboard()
            .stringForType(ui::browser_drag_type())?
            .to_string();
        self.ivars()
            .app
            .browser_drop_destination(&path, insertion)?;
        Some((path, insertion))
    }

    fn settings_selection(&self) -> Option<usize> {
        self.ivars()
            .settings
            .borrow()
            .as_ref()
            .and_then(|settings| usize::try_from(settings.table.selectedRow()).ok())
    }

    fn refresh_settings(&self, selected: Option<usize>) {
        let selected_row = self.ivars().app.refresh_settings(selected);
        if self.ivars().settings.borrow().is_none() {
            let settings = ui::settings(self, self.ivars().app.configuring());
            self.ivars().settings.borrow_mut().replace(settings);
        }
        self.update_settings(selected_row);
        self.refresh_default_status();
    }

    fn update_settings(&self, selected_row: usize) {
        if let Some(settings) = &*self.ivars().settings.borrow() {
            settings.update(selected_row, browsers::manual_order());
        }
        self.update_timeout_settings();
    }

    fn refresh_default_status(&self) {
        if !self.ivars().app.configuring()
            && let Some(settings) = &*self.ivars().settings.borrow()
        {
            settings.set_status(false, &browsers::default_status());
        }
    }

    fn move_settings_browser(&self, offset: isize) {
        if let Some(index) = self.settings_selection()
            && self.ivars().app.move_browser(index, offset)
        {
            self.update_settings(
                index
                    .checked_add_signed(offset)
                    .expect("valid browser move"),
            );
            self.refresh_chooser(false);
        }
    }

    fn copy_link_to(&self, pasteboard: &NSPasteboard) {
        self.cancel_timeout();
        let url = self.ivars().app.preview();
        let Some(url) = url else { return };
        if let Some(chooser) = self.ivars().chooser.get() {
            chooser.copy_link(pasteboard, &url);
        }
    }

    fn rejected_link(&self, reason: Rejection) {
        self.cancel_timeout();
        if self.ivars().showing_rejection.replace(true) {
            return;
        }
        let (title, detail) = reason.message();
        ui::alert(title, detail, self.mtm());
        self.ivars().showing_rejection.set(false);
    }

    fn refresh_chooser(&self, present: bool) {
        let Some(chooser) = self.ivars().chooser.get() else {
            return;
        };
        if self.ivars().app.busy() {
            return;
        }
        if !present {
            self.cancel_timeout();
        }
        let selected = usize::try_from(chooser.table.selectedRow()).ok();
        let content = self.ivars().app.refresh(selected);
        self.ivars().updating_chooser.set(true);
        chooser.update(
            content.as_ref(),
            self.ivars().app.browser_count(),
            present,
            self.mtm(),
        );
        self.ivars().updating_chooser.set(false);
        if present && content.is_some() {
            self.start_timeout();
        }
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

fn is_settings_table(table: &NSTableView) -> bool {
    table.identifier().as_deref() == Some(ns_string!("settingsBrowsers"))
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
    let app = autoreleasepool(|_| {
        let app = NSApplication::sharedApplication(mtm);
        let delegate = Delegate::alloc(mtm).set_ivars(State::default());
        let delegate: Retained<Delegate> = unsafe { msg_send![super(delegate), init] };
        app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        DELEGATE.with(|cell| {
            let _ = cell.set(delegate);
        });
        app
    });
    app.run();
}
