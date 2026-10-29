use objc2::{
    MainThreadOnly, define_class, msg_send,
    rc::Retained,
    runtime::{ProtocolObject, Sel},
    sel,
};
use objc2_app_kit::*;
use objc2_foundation::{
    MainThreadMarker, NSArray, NSEdgeInsets, NSIndexSet, NSObjectProtocol, NSSize, NSString,
    ns_string,
};

use crate::{app::ChooserContent, browsers::Browser, delegate::Delegate};

define_class!(
    #[unsafe(super = NSStackView)]
    #[thread_kind = MainThreadOnly]
    struct BrowserList;

    unsafe impl NSObjectProtocol for BrowserList {}

    impl BrowserList {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }
    }
);

pub struct Chooser {
    pub window: Retained<NSWindow>,
    pub table: Retained<NSTableView>,
    pub preview: Retained<NSTextField>,
    pub copy: Retained<NSButton>,
    pub message: Retained<NSTextField>,
    pub open: Retained<NSButton>,
    pub cancel: Retained<NSButton>,
}

pub struct Settings {
    pub window: Retained<NSWindow>,
    pub status: Retained<NSTextField>,
    pub make_default: Retained<NSButton>,
    reorder_buttons: Vec<[Retained<NSButton>; 2]>,
}

impl Chooser {
    pub fn copy_link(&self, pasteboard: &NSPasteboard, url: &NSString) {
        pasteboard.clearContents();
        let copied = pasteboard.setString_forType(url, ns_string!("public.utf8-plain-text"));
        self.message.setStringValue(if copied {
            ns_string!("Link copied.")
        } else {
            ns_string!("Could not copy the link. Try again.")
        });
    }

    pub fn browser_unavailable(&self) {
        self.message.setStringValue(ns_string!(
            "That browser is no longer available. Choose another browser."
        ));
    }

    pub fn dispatch_failed(&self, browser: &str, mtm: MainThreadMarker) {
        self.message.setStringValue(&NSString::from_str(&format!(
            "Could not open {browser}. Choose another browser or try again."
        )));
        present(&self.window, mtm);
    }

    pub fn update(
        &self,
        content: Option<&ChooserContent>,
        browser_count: usize,
        show: bool,
        mtm: MainThreadMarker,
    ) {
        let Some(content) = content else {
            self.window.orderOut(None);
            self.preview.setStringValue(ns_string!(""));
            self.preview.setToolTip(None);
            return;
        };
        self.preview.setStringValue(&content.preview);
        self.preview.setToolTip(Some(&content.preview));
        self.table.reloadData();
        if browser_count > 0 {
            self.table.selectRowIndexes_byExtendingSelection(
                &NSIndexSet::indexSetWithIndex(content.selected_row),
                false,
            );
        }
        self.message
            .setStringValue(&NSString::from_str(&if browser_count == 0 {
                "No enabled browsers. Open Settings to enable or refresh browsers.".into()
            } else if content.count > 1 {
                format!("Open all {} links in the selected browser.", content.count)
            } else {
                "Use ↑ ↓ to choose, Return to open, or Escape to cancel.".into()
            }));
        self.set_busy(false, browser_count);
        if show {
            present(&self.window, mtm);
            self.window.makeFirstResponder(Some(&self.table));
        }
    }

    pub fn set_busy(&self, busy: bool, browser_count: usize) {
        self.copy.setEnabled(!busy);
        self.open.setEnabled(!busy && browser_count > 0);
        self.cancel.setEnabled(!busy);
        self.table.setEnabled(!busy);
        if busy {
            self.window.orderOut(None);
            self.message
                .setStringValue(ns_string!("Opening in your browser…"));
        }
    }
}

impl Settings {
    pub fn focus_browser_move(&self, index: usize, offset: isize) {
        if let Some(buttons) = self.reorder_buttons.get(index) {
            let preferred = usize::from(offset > 0);
            let button = if buttons[preferred].isEnabled() {
                &buttons[preferred]
            } else {
                &buttons[1 - preferred]
            };
            self.window.contentView().unwrap().layoutSubtreeIfNeeded();
            button.scrollRectToVisible(button.bounds());
            self.window.makeFirstResponder(Some(button));
        }
    }

    pub fn set_status(&self, configuring: bool, status: &str) {
        self.make_default.setEnabled(!configuring);
        self.status.setStringValue(&NSString::from_str(status));
    }
}

pub fn confirm_quit(mtm: MainThreadMarker) -> bool {
    let alert = NSAlert::new(mtm);
    alert.setMessageText(ns_string!("Quit with pending links?"));
    alert.setInformativeText(ns_string!(
        "Pending links are kept only in memory and will be discarded."
    ));
    alert.addButtonWithTitle(ns_string!("Keep ChooBro Open"));
    alert.addButtonWithTitle(ns_string!("Quit"));
    alert.runModal() == NSAlertSecondButtonReturn
}

pub fn label(text: &str, mtm: MainThreadMarker) -> Retained<NSTextField> {
    NSTextField::labelWithString(&NSString::from_str(text), mtm)
}

fn stack(mtm: MainThreadMarker, vertical: bool) -> Retained<NSStackView> {
    let stack = NSStackView::new(mtm);
    stack.setOrientation(if vertical {
        NSUserInterfaceLayoutOrientation::Vertical
    } else {
        NSUserInterfaceLayoutOrientation::Horizontal
    });
    stack.setAlignment(if vertical {
        NSLayoutAttribute::Leading
    } else {
        NSLayoutAttribute::CenterY
    });
    stack.setSpacing(12.0);
    stack.setTranslatesAutoresizingMaskIntoConstraints(false);
    stack
}

enum ButtonAction {
    CopyLink,
    Settings,
    Cancel,
    Open,
    MakeDefault,
    MoveUp,
    MoveDown,
}

impl ButtonAction {
    fn selector(&self) -> Sel {
        match self {
            Self::CopyLink => sel!(copyLink:),
            Self::Settings => sel!(showSettings:),
            Self::Cancel => sel!(cancelRequest:),
            Self::Open => sel!(openBrowser:),
            Self::MakeDefault => sel!(makeDefault:),
            Self::MoveUp => sel!(moveBrowserUp:),
            Self::MoveDown => sel!(moveBrowserDown:),
        }
    }
}

fn button(title: &str, action: ButtonAction, target: &Delegate) -> Retained<NSButton> {
    unsafe {
        NSButton::buttonWithTitle_target_action(
            &NSString::from_str(title),
            Some(target),
            Some(action.selector()),
            target.mtm(),
        )
    }
}

fn window(
    title: &str,
    width: f64,
    height: f64,
    target: &Delegate,
) -> (Retained<NSWindow>, Retained<NSStackView>) {
    let mtm = target.mtm();
    let controller = NSViewController::new(mtm);
    controller.setView(&NSView::new(mtm));
    let window = NSWindow::windowWithContentViewController(&controller);
    window.setStyleMask(NSWindowStyleMask::Titled | NSWindowStyleMask::Closable);
    window.setContentSize(NSSize::new(width, height));
    window.setTitle(&NSString::from_str(title));
    window.setDelegate(Some(ProtocolObject::from_ref(target)));
    window.center();
    let content = window.contentView().expect("window content view");
    content.setWantsLayer(true);
    let root = stack(mtm, true);
    content.addSubview(&root);
    NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&[
        root.widthAnchor().constraintEqualToConstant(width - 48.0),
        root.leadingAnchor()
            .constraintEqualToAnchor_constant(&content.leadingAnchor(), 24.0),
        root.trailingAnchor()
            .constraintEqualToAnchor_constant(&content.trailingAnchor(), -24.0),
        root.topAnchor()
            .constraintEqualToAnchor_constant(&content.topAnchor(), 24.0),
        root.bottomAnchor()
            .constraintEqualToAnchor_constant(&content.bottomAnchor(), -24.0),
    ]));
    (window, root)
}

fn fill_width(view: &NSView, root: &NSView) {
    view.widthAnchor()
        .constraintEqualToAnchor(&root.widthAnchor())
        .setActive(true);
}

pub fn chooser(target: &Delegate) -> Chooser {
    let mtm = target.mtm();
    let (window, root) = window("Choose Browser", 480.0, 400.0, target);
    window.setStyleMask(NSWindowStyleMask::Titled);
    let heading = label("Open with…", mtm);
    heading.setFont(Some(&NSFont::boldSystemFontOfSize(20.0)));
    let header = stack(mtm, false);
    header.addArrangedSubview(&heading);
    header.addArrangedSubview(&NSView::new(mtm));
    let copy = button("Copy Link", ButtonAction::CopyLink, target);
    copy.setToolTip(Some(ns_string!("Copy the full displayed link")));
    copy.setKeyEquivalent(ns_string!("c"));
    copy.setKeyEquivalentModifierMask(NSEventModifierFlags::Command);
    header.addArrangedSubview(&copy);
    root.addArrangedSubview(&header);
    fill_width(&header, &root);
    let preview = NSTextField::wrappingLabelWithString(ns_string!(""), mtm);
    preview.setSelectable(false);
    preview.setPreferredMaxLayoutWidth(432.0);
    preview.setLineBreakMode(NSLineBreakMode::ByCharWrapping);
    preview.setMaximumNumberOfLines(3);
    preview
        .cell()
        .expect("URL preview cell")
        .setTruncatesLastVisibleLine(true);
    root.addArrangedSubview(&preview);
    fill_width(&preview, &root);

    let table = NSTableView::new(mtm);
    let column =
        NSTableColumn::initWithIdentifier(NSTableColumn::alloc(mtm), ns_string!("browser"));
    column.setWidth(420.0);
    table.addTableColumn(&column);
    table.setHeaderView(None);
    table.setRowHeight(44.0);
    table.setAllowsEmptySelection(false);
    table.setAllowsMultipleSelection(false);
    unsafe {
        table.setDataSource(Some(ProtocolObject::from_ref(target)));
        table.setDelegate(Some(ProtocolObject::from_ref(target)));
        table.setTarget(Some(target));
        table.setDoubleAction(Some(sel!(openBrowser:)));
    }
    table.setAccessibilityLabel(Some(ns_string!("Installed browsers")));
    let scroll = NSScrollView::new(mtm);
    scroll.setDocumentView(Some(&table));
    scroll.setHasVerticalScroller(true);
    scroll.setAutohidesScrollers(true);
    root.addArrangedSubview(&scroll);
    fill_width(&scroll, &root);
    scroll
        .heightAnchor()
        .constraintGreaterThanOrEqualToConstant(100.0)
        .setActive(true);

    let message = label("", mtm);
    message.setMaximumNumberOfLines(2);
    message.setTextColor(Some(&NSColor::secondaryLabelColor()));
    root.addArrangedSubview(&message);
    fill_width(&message, &root);
    let actions = stack(mtm, false);
    let settings = button("Settings…", ButtonAction::Settings, target);
    let cancel = button("Cancel", ButtonAction::Cancel, target);
    cancel.setKeyEquivalent(ns_string!("\u{1b}"));
    let open = button("Open", ButtonAction::Open, target);
    open.setKeyEquivalent(ns_string!("\r"));
    actions.addArrangedSubview(&settings);
    let spacer = NSView::new(mtm);
    actions.addArrangedSubview(&spacer);
    actions.addArrangedSubview(&cancel);
    actions.addArrangedSubview(&open);
    root.addArrangedSubview(&actions);
    fill_width(&actions, &root);
    window.setInitialFirstResponder(Some(&table));
    Chooser {
        window,
        table,
        preview,
        copy,
        message,
        open,
        cancel,
    }
}

pub fn browser_cell(browser: &Browser, mtm: MainThreadMarker) -> Retained<NSView> {
    let cell = NSTableCellView::new(mtm);
    let icon = NSImageView::new(mtm);
    icon.setImage(Some(&browser.icon));
    icon.setTranslatesAutoresizingMaskIntoConstraints(false);
    let text = label(&browser.name, mtm);
    text.setTranslatesAutoresizingMaskIntoConstraints(false);
    cell.addSubview(&icon);
    cell.addSubview(&text);
    unsafe {
        cell.setTextField(Some(&text));
        cell.setImageView(Some(&icon));
    }
    cell.setToolTip(Some(&NSString::from_str(&browser.path)));
    NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&[
        icon.leadingAnchor()
            .constraintEqualToAnchor_constant(&cell.leadingAnchor(), 8.0),
        icon.centerYAnchor()
            .constraintEqualToAnchor(&cell.centerYAnchor()),
        icon.widthAnchor().constraintEqualToConstant(28.0),
        icon.heightAnchor().constraintEqualToConstant(28.0),
        text.leadingAnchor()
            .constraintEqualToAnchor_constant(&icon.trailingAnchor(), 12.0),
        text.trailingAnchor()
            .constraintEqualToAnchor_constant(&cell.trailingAnchor(), -8.0),
        text.centerYAnchor()
            .constraintEqualToAnchor(&cell.centerYAnchor()),
    ]));
    Retained::into_super(cell)
}

pub fn settings(
    target: &Delegate,
    browsers: &[Browser],
    excluded: &std::collections::BTreeSet<String>,
    configuring: bool,
    manual: bool,
) -> Settings {
    let mtm = target.mtm();
    let (window, root) = window("ChooBro Settings", 520.0, 480.0, target);
    let heading = label("Choose your browsers", mtm);
    heading.setFont(Some(&NSFont::boldSystemFontOfSize(20.0)));
    root.addArrangedSubview(&heading);
    root.addArrangedSubview(&label("Checked browsers appear when you open a link.", mtm));
    let sort = unsafe {
        NSButton::checkboxWithTitle_target_action(
            ns_string!("Manual order (unchecked: alphabetical)"),
            Some(target),
            Some(sel!(changeBrowserSort:)),
            mtm,
        )
    };
    sort.setState(if manual {
        NSControlStateValueOn
    } else {
        NSControlStateValueOff
    });
    root.addArrangedSubview(&sort);
    let list: Retained<BrowserList> = unsafe { msg_send![BrowserList::alloc(mtm), init] };
    list.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
    list.setAlignment(NSLayoutAttribute::Leading);
    list.setSpacing(12.0);
    list.setEdgeInsets(NSEdgeInsets {
        top: 12.0,
        left: 12.0,
        bottom: 12.0,
        right: 12.0,
    });
    list.setTranslatesAutoresizingMaskIntoConstraints(false);
    let mut reorder_buttons = Vec::new();
    for (index, browser) in browsers.iter().enumerate() {
        let toggle = unsafe {
            NSButton::checkboxWithTitle_target_action(
                &NSString::from_str(&browser.name),
                Some(target),
                Some(sel!(toggleBrowser:)),
                mtm,
            )
        };
        toggle.setTag(index as isize);
        toggle.setState(if excluded.contains(&browser.id) {
            NSControlStateValueOff
        } else {
            NSControlStateValueOn
        });
        toggle.setToolTip(Some(&NSString::from_str(&browser.path)));
        let row = stack(mtm, false);
        row.addArrangedSubview(&toggle);
        row.addArrangedSubview(&NSView::new(mtm));
        let buttons = [
            ("↑", ButtonAction::MoveUp, manual && index > 0),
            (
                "↓",
                ButtonAction::MoveDown,
                manual && index + 1 < browsers.len(),
            ),
        ]
        .map(|(title, action, enabled)| {
            let move_button = button(title, action, target);
            move_button.setTag(index as isize);
            move_button.setEnabled(enabled);
            let direction = if title == "↑" { "up" } else { "down" };
            let description = NSString::from_str(&format!("Move {} {direction}", browser.name));
            move_button.setAccessibilityLabel(Some(&description));
            move_button.setToolTip(Some(&description));
            row.addArrangedSubview(&move_button);
            move_button
        });
        reorder_buttons.push(buttons);
        list.addArrangedSubview(&row);
        row.widthAnchor()
            .constraintEqualToAnchor_constant(&list.widthAnchor(), -24.0)
            .setActive(true);
    }
    if browsers.is_empty() {
        list.addArrangedSubview(&label(
            "No registered browsers found. Install a browser, then refresh.",
            mtm,
        ));
    }
    let scroll = NSScrollView::new(mtm);
    scroll.setDocumentView(Some(&list));
    scroll.setHasVerticalScroller(true);
    scroll.setAutohidesScrollers(true);
    root.addArrangedSubview(&scroll);
    fill_width(&scroll, &root);
    NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&[
        list.widthAnchor()
            .constraintEqualToAnchor(&scroll.contentView().widthAnchor()),
        scroll
            .heightAnchor()
            .constraintGreaterThanOrEqualToConstant(100.0),
    ]));
    let status = label(&crate::browsers::default_status(), mtm);
    status.setMaximumNumberOfLines(3);
    root.addArrangedSubview(&status);
    fill_width(&status, &root);
    let actions = stack(mtm, false);
    let refresh = button("Refresh", ButtonAction::Settings, target);
    let make_default = button("Make Default Browser…", ButtonAction::MakeDefault, target);
    make_default.setEnabled(!configuring);
    actions.addArrangedSubview(&refresh);
    actions.addArrangedSubview(&make_default);
    root.addArrangedSubview(&actions);
    let help = label(
        "You can also change the default browser in System Settings.",
        mtm,
    );
    help.setTextColor(Some(&NSColor::secondaryLabelColor()));
    root.addArrangedSubview(&help);
    Settings {
        window,
        status,
        make_default,
        reorder_buttons,
    }
}

fn menu(target: &Delegate) -> Retained<NSMenu> {
    let menu = NSMenu::new(target.mtm());
    for (title, action, key) in [
        ("Settings…", sel!(showSettings:), ","),
        ("Show Pending Links", sel!(showChooser:), ""),
        ("Quit ChooBro", sel!(quit:), "q"),
    ] {
        let item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(target.mtm()),
                &NSString::from_str(title),
                Some(action),
                &NSString::from_str(key),
            )
        };
        unsafe { item.setTarget(Some(target)) };
        menu.addItem(&item);
    }
    menu
}

pub fn menus(target: &Delegate) -> Retained<NSStatusItem> {
    let app = NSApplication::sharedApplication(target.mtm());
    let main = NSMenu::new(target.mtm());
    let item = NSMenuItem::new(target.mtm());
    item.setSubmenu(Some(&menu(target)));
    main.addItem(&item);
    app.setMainMenu(Some(&main));
    let status = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
    if let Some(button) = status.button(target.mtm()) {
        button.setTitle(ns_string!("CB"));
        button.setToolTip(Some(ns_string!("ChooBro — Choose Browser")));
    }
    status.setMenu(Some(&menu(target)));
    status
}

pub fn alert(title: &str, detail: &str, mtm: MainThreadMarker) {
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str(title));
    alert.setInformativeText(&NSString::from_str(detail));
    alert.addButtonWithTitle(ns_string!("OK"));
    alert.runModal();
}

pub fn present(window: &NSWindow, mtm: MainThreadMarker) {
    #[allow(deprecated)]
    NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
    window.makeKeyAndOrderFront(None);
}
