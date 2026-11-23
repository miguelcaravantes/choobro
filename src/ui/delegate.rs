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
    NSOperationQueue, NSProcessInfo, NSString, NSTimer, NSURL, ns_string,
};

use crate::{
    app::{App, Incoming, OpenRequest, SavedTarget},
    browsers,
    requests::Rejection,
    ui,
};

fn ambiguous_browser_message() -> &'static NSString {
    ns_string!(
        "More than one app matches the saved browser or profile. Choose by installation path."
    )
}

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
    countdown: RefCell<Option<Retained<NSTimer>>>,
    timeout_monitor: RefCell<Option<Retained<AnyObject>>>,
    updating_chooser: Cell<bool>,
    returning_from_profile_privacy: Cell<bool>,
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
            if self.ivars().returning_from_profile_privacy.replace(false) {
                self.refresh_profile_access();
            }
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
            } else if is_rules_table(table) {
                self.ivars().app.rule_count() as NSInteger
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
        fn text_editing_ended(&self, notification: &NSNotification) {
            if let Some(settings) = &*self.ivars().settings.borrow()
                && notification.object().and_then(|o| o.downcast::<NSTextField>().ok())
                    .is_some_and(|field| *field == *settings.timeout_seconds)
            {
                self.save_timeout();
            }
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
                        let excluded = browsers::exclusions();
                        let enabled = !excluded.contains(&browser.target_key());
                        let parent_visible = browser.parent_is_visible(&excluded);
                        Some(ui::browser_toggle(&browser, row, enabled, parent_visible, table, self))
                    } else {
                        Some(ui::browser_cell(&browser, table, 0.0, false))
                    }
                } else if is_rules_table(table) {
                    let rule = self.ivars().app.rule(row)?;
                    if column.is_some_and(|column| &*column.identifier() == ns_string!("ruleDomain")) {
                        Some(ui::rule_domain_cell(&rule.pattern, table))
                    } else {
                        let targets = self.ivars().app.settings_rule_targets();
                        Some(ui::rule_browser_cell(&rule, &targets, table, self))
                    }
                } else {
                    self.ivars().app.browser(row).map(|browser| {
                        let show_path = self.ivars().app.show_browser_path(&browser);
                        ui::browser_cell(&browser, table, 8.0, show_path)
                    })
                }
            })
        }

        #[unsafe(method(tableViewSelectionDidChange:))]
        fn selection_changed(&self, notification: &NSNotification) {
            let Some(table) = notification.object().and_then(|object| object.downcast::<NSTableView>().ok()) else {
                return;
            };
            if is_settings_table(&table) {
                if let Some(settings) = &*self.ivars().settings.borrow() {
                    settings.update_move_buttons();
                }
            } else if is_rules_table(&table) {
                if let Some(settings) = &*self.ivars().settings.borrow() {
                    settings.update_rule_buttons();
                }
            } else if !self.ivars().updating_chooser.get() {
                self.cancel_timeout();
            }
        }
    }

    impl Delegate {
        #[unsafe(method(changeSettingsTab:))]
        fn change_settings_tab(&self, sender: &NSSegmentedControl) {
            if let Some(settings) = &*self.ivars().settings.borrow() {
                settings.select_tab(sender.selectedSegment());
            }
        }

        #[unsafe(method(toggleRemember:))]
        fn toggle_remember(&self, _sender: Option<&AnyObject>) {
            self.cancel_timeout();
        }

        #[unsafe(method(addRule:))]
        fn add_rule(&self, _sender: Option<&AnyObject>) {
            self.save_rule_from_settings();
        }

        #[unsafe(method(changeRuleTarget:))]
        fn change_rule_target(&self, sender: &NSPopUpButton) {
            let settings = self.ivars().settings.borrow();
            let Some(settings) = settings.as_ref() else { return };
            let Ok(row) = usize::try_from(settings.rules_table.rowForView(sender)) else { return };
            let Some(rule) = self.ivars().app.rule(row) else { return };
            let Some(key) = sender.selectedItem()
                .and_then(|item| item.representedObject())
                .and_then(|object| object.downcast::<NSString>().ok())
            else { return };
            let key = key.to_string();
            if key != rule.target_key && self.ivars().app.save_domain_rule(&rule.pattern, &key).is_some()
                && sender.itemAtIndex(0).is_some_and(|item| item.representedObject().is_none())
            {
                sender.removeItemAtIndex(0);
            }
        }

        #[unsafe(method(removeRule:))]
        fn remove_rule(&self, _sender: Option<&AnyObject>) {
            self.remove_selected_rule();
        }

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
            match self.ivars().app.timeout_browser(&preference.browser_id) {
                SavedTarget::Unique(index) => { let _ = self.dispatch_browser(index, false); }
                SavedTarget::Ambiguous => chooser.message.setStringValue(ambiguous_browser_message()),
                SavedTarget::Unavailable => {}
            }
        }

        #[unsafe(method(countdownTick:))]
        fn countdown_tick(&self, timer: &NSTimer) {
            if self.ivars().countdown.borrow().as_ref().is_some_and(|current| &**current == timer) {
                self.update_countdown();
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

        #[unsafe(method(openProfilePrivacy:))]
        fn open_profile_privacy(&self, _sender: Option<&AnyObject>) {
            self.cancel_timeout();
            let macos_version = NSProcessInfo::processInfo().operatingSystemVersion().majorVersion;
            let target = if macos_version >= 26 {
                ns_string!("x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?Privacy_FilesAndFolders")
            } else {
                ns_string!("x-apple.systempreferences:com.apple.preference.security?Privacy_FilesAndFolders")
            };
            if NSURL::URLWithString(target).is_some_and(|url| NSWorkspace::sharedWorkspace().openURL(&url)) {
                self.ivars().returning_from_profile_privacy.set(true);
            } else {
                let directions = if macos_version >= 13 {
                    "Open System Settings → Privacy & Security → Files & Folders, then expand ChooBro."
                } else {
                    "Open System Preferences → Security & Privacy → Privacy → Files and Folders."
                };
                ui::alert("Could not open Files & Folders", directions, self.mtm());
            }
        }

        #[unsafe(method(showProfileSettings:))]
        fn show_profile_settings(&self, sender: Option<&AnyObject>) {
            self.show_settings(sel!(showSettings:), sender);
            if let Some(settings) = &*self.ivars().settings.borrow() {
                settings.select_tab(2);
                settings.window.makeFirstResponder(Some(&*settings.profiles));
            }
        }

        #[unsafe(method(chooseChromeFolder:))]
        fn choose_chrome_folder(&self, _sender: Option<&AnyObject>) {
            self.pick_profile_folder("Chrome");
        }

        #[unsafe(method(chooseChromeBetaFolder:))]
        fn choose_chrome_beta_folder(&self, _sender: Option<&AnyObject>) {
            self.pick_profile_folder("Chrome Beta");
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

        #[unsafe(method(toggleBrowserProfiles:))]
        fn toggle_browser_profiles(&self, sender: &NSButton) {
            browsers::set_profiles_enabled(sender.state() == NSControlStateValueOn);
            self.refresh_profile_access();
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
            let remember = !chooser.remember.isHidden() && chooser.remember.state() == NSControlStateValueOn;
            let _ = self.dispatch_browser(index, remember);
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
            let browsers = self.ivars().app.settings_regular_browsers();
            settings.update_timeout(&browsers);
        }
    }

    fn cancel_timeout(&self) {
        self.ivars().app.cancel_timeout();
        if let Some(timer) = self.ivars().countdown.borrow_mut().take() {
            timer.invalidate();
        }
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
        if !preference.enabled {
            return;
        }
        match self.ivars().app.timeout_browser(&preference.browser_id) {
            SavedTarget::Unavailable => return,
            SavedTarget::Ambiguous => {
                if let Some(chooser) = self.ivars().chooser.get() {
                    chooser.message.setStringValue(ambiguous_browser_message());
                }
                return;
            }
            SavedTarget::Unique(_) => {}
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
        let countdown = unsafe {
            NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                1.0,
                self,
                sel!(countdownTick:),
                None,
                true,
            )
        };
        self.ivars().countdown.replace(Some(countdown));
        self.update_countdown();
    }

    fn update_countdown(&self) {
        let Some(seconds) = self
            .ivars()
            .timeout
            .borrow()
            .as_ref()
            .map(|timer| timer.fireDate().timeIntervalSinceNow().max(0.0).ceil())
        else {
            return;
        };
        if let Some(chooser) = self.ivars().chooser.get()
            && let Some(id) = self.ivars().timeout_browser.borrow().as_ref()
            && let SavedTarget::Unique(index) = self.ivars().app.timeout_browser(id)
            && let Some(browser) = self.ivars().app.browser(index)
        {
            chooser.message.setStringValue(&NSString::from_str(&format!(
                "Opening in {} in {seconds:.0}s. Interact to cancel automatic opening.",
                browser.name
            )));
        }
    }

    fn save_rule_from_settings(&self) {
        let index = {
            let settings = self.ivars().settings.borrow();
            let Some(settings) = settings.as_ref() else {
                return;
            };
            let domain = settings.rule_domain.stringValue().to_string();
            let target_key = settings
                .rule_browser
                .selectedItem()
                .and_then(|item| item.representedObject())
                .and_then(|object| object.downcast::<NSString>().ok())
                .map(|id| id.to_string())
                .unwrap_or_default();
            let Some(index) = self.ivars().app.save_domain_rule(&domain, &target_key) else {
                settings.rule_error.setHidden(false);
                return;
            };
            settings.rule_error.setHidden(true);
            settings.rule_domain.setStringValue(ns_string!(""));
            index
        };
        self.update_rules_settings(Some(index));
    }

    fn remove_selected_rule(&self) {
        let selected = self
            .ivars()
            .settings
            .borrow()
            .as_ref()
            .and_then(|settings| usize::try_from(settings.rules_table.selectedRow()).ok());
        let Some(index) = selected else {
            return;
        };
        if let Some(next_selected) = self.ivars().app.remove_rule(index) {
            self.update_rules_settings(Some(next_selected));
        }
    }

    fn update_rules_settings(&self, selected_row: Option<usize>) {
        if let Some(settings) = &*self.ivars().settings.borrow() {
            let browsers = self.ivars().app.settings_rule_targets();
            let selected =
                selected_row.or_else(|| usize::try_from(settings.rules_table.selectedRow()).ok());
            settings.update_rules(&browsers, selected);
        }
    }

    fn dispatch_browser(&self, index: usize, remember: bool) -> Option<&'static NSString> {
        self.cancel_timeout();
        let message = match self.ivars().app.open(index, remember) {
            OpenRequest::None => return None,
            OpenRequest::Unavailable => {
                self.refresh_chooser(false);
                ns_string!("That browser or profile is no longer available. Choose another.")
            }
            OpenRequest::ProfileBatchTooLarge => {
                ns_string!(
                    "Chrome profiles support about 24 KiB per request. Choose a regular browser for these links."
                )
            }
            OpenRequest::Ready(browser, urls, arguments) => {
                self.set_busy(true);
                let name = browser.name.clone();
                browsers::open(&browser, &urls, arguments.as_deref(), move |success| {
                    let name = name.clone();
                    on_main(move || {
                        with_delegate(|delegate| delegate.dispatch_finished(success, &name))
                    });
                });
                return None;
            }
        };
        if let Some(chooser) = self.ivars().chooser.get() {
            chooser.message.setStringValue(message);
        }
        Some(message)
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
        if !item.setString_forType(
            &NSString::from_str(&browser.order_key()),
            ui::browser_drag_type(),
        ) {
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
        let Some((key, insertion)) = self.browser_drop(table, info, row, operation) else {
            return false;
        };
        let Some(destination) = self.ivars().app.drop_browser(&key, insertion) else {
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
        let key = info
            .draggingPasteboard()
            .stringForType(ui::browser_drag_type())?
            .to_string();
        self.ivars().app.browser_drop_destination(&key, insertion)?;
        Some((key, insertion))
    }

    fn pick_profile_folder(&self, folder: &str) {
        if !browsers::profiles_enabled() {
            return;
        }
        self.cancel_timeout();
        let Some(expected) = browsers::profile_folder_url(folder) else {
            return;
        };
        let panel = NSOpenPanel::openPanel(self.mtm());
        panel.setCanChooseFiles(false);
        panel.setCanChooseDirectories(true);
        panel.setResolvesAliases(false);
        panel.setDirectoryURL(Some(&expected));
        panel.setPrompt(Some(ns_string!("Allow Access")));
        panel.setMessage(Some(&NSString::from_str(&format!(
            "Choose the {folder} folder to let ChooBro read its profile list."
        ))));
        if panel.runModal() != NSModalResponseOK {
            return;
        }
        if panel.URL().and_then(|url| url.path()) != expected.path() {
            ui::alert(
                "Choose the Chrome data folder",
                &format!(
                    "Select ~/Library/Application Support/Google/{folder}, not a profile subfolder."
                ),
                self.mtm(),
            );
            return;
        }
        self.refresh_profile_access();
    }

    fn refresh_profile_access(&self) {
        if self.ivars().settings.borrow().is_some() {
            self.refresh_settings(self.settings_selection());
        }
        self.refresh_chooser(false);
    }

    fn settings_selection(&self) -> Option<usize> {
        self.ivars()
            .settings
            .borrow()
            .as_ref()
            .and_then(|settings| usize::try_from(settings.table.selectedRow()).ok())
    }

    fn refresh_settings(&self, selected: Option<usize>) {
        let (selected_row, profile_notice) = self.ivars().app.refresh_settings(selected);
        if self.ivars().settings.borrow().is_none() {
            let settings = ui::settings(self, self.ivars().app.configuring());
            if let Some(content) = settings.window.contentView() {
                content.layoutSubtreeIfNeeded();
            }
            settings.select_tab(0);
            self.ivars().settings.borrow_mut().replace(settings);
        }
        self.update_settings(selected_row);
        if let Some(settings) = &*self.ivars().settings.borrow() {
            settings.update_profile_setup(
                browsers::profiles_enabled(),
                profile_notice.as_ref(),
                browsers::chrome_beta_installed(),
            );
        }
        self.refresh_default_status();
    }

    fn update_settings(&self, selected_row: usize) {
        if let Some(settings) = &*self.ivars().settings.borrow() {
            settings.update(selected_row, browsers::manual_order());
        }
        self.update_timeout_settings();
        self.update_rules_settings(None);
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
        };
        if !present {
            self.cancel_timeout();
        }
        let selected = usize::try_from(chooser.table.selectedRow()).ok();
        let content = self.ivars().app.refresh(selected);
        let route_notice = if present && content.is_some() {
            match self.ivars().app.claim_route() {
                Some(browsers::RuleRoute::MixedTargets) => Some(ns_string!(
                    "These links don’t share the same rule target. Choose where to open them."
                )),
                Some(browsers::RuleRoute::Target(target)) => {
                    match self.ivars().app.route_target(&target) {
                        SavedTarget::Unique(index) => {
                            let notice = self.dispatch_browser(index, false);
                            if self.ivars().app.busy() {
                                return;
                            }
                            notice
                        }
                        SavedTarget::Ambiguous => Some(ambiguous_browser_message()),
                        SavedTarget::Unavailable => Some(ns_string!(
                            "The browser or profile saved by your rule is unavailable. Choose where to open the link."
                        )),
                    }
                }
                None => None,
            }
        } else {
            None
        };
        self.ivars().updating_chooser.set(true);
        chooser.update(
            content.as_ref(),
            self.ivars().app.browser_count(),
            present,
            present,
            self.mtm(),
        );
        self.ivars().updating_chooser.set(false);
        if let Some(notice) = route_notice {
            chooser.message.setStringValue(notice);
        } else if present && content.is_some() {
            self.start_timeout();
        }
    }

    fn set_busy(&self, busy: bool) {
        if let Some(chooser) = self.ivars().chooser.get() {
            chooser.set_busy(busy, self.ivars().app.browser_count());
        }
    }

    fn dispatch_finished(&self, success: bool, browser: &str) {
        let (present, rule) = self.ivars().app.dispatch_finished(success);
        if let Some(rule) = rule
            && let Some(index) = self
                .ivars()
                .app
                .save_domain_rule(&rule.pattern, &rule.target_key)
        {
            self.update_rules_settings(Some(index));
        }
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

fn is_rules_table(table: &NSTableView) -> bool {
    table.identifier().as_deref() == Some(ns_string!("settingsRules"))
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
