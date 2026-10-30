use objc2::{
    MainThreadOnly,
    rc::Retained,
    runtime::{ProtocolObject, Sel},
    sel,
};
use objc2_app_kit::*;
use objc2_foundation::{MainThreadMarker, NSArray, NSIndexSet, NSSize, NSString, ns_string};

use crate::{app::ChooserContent, browsers::Browser, delegate::Delegate};

pub fn browser_drag_type() -> &'static NSString {
    ns_string!("io.github.miguelcaravantes.choobro.browser-order")
}

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
    pub default_warning: Retained<NSTextField>,
    pub make_default: Retained<NSButton>,
    pub table: Retained<NSTableView>,
    pub sort: Retained<NSPopUpButton>,
    pub move_up: Retained<NSButton>,
    pub move_down: Retained<NSButton>,
    hint: Retained<NSTextField>,
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
    pub fn update(&self, selected_row: usize, manual: bool) {
        self.sort.selectItemAtIndex(isize::from(manual));
        self.table.reloadData();
        if self.table.numberOfRows() > 0 {
            self.table.selectRowIndexes_byExtendingSelection(
                &NSIndexSet::indexSetWithIndex(selected_row),
                false,
            );
            self.table.scrollRowToVisible(selected_row as isize);
        }
        self.hint.setStringValue(if self.table.numberOfRows() == 0 {
            ns_string!("No browsers found. Install a browser, then click Refresh.")
        } else if manual {
            ns_string!("Drag browsers or use the arrows to change their order.")
        } else {
            ns_string!("")
        });
        self.hint
            .setHidden(!manual && self.table.numberOfRows() > 0);
        self.update_move_buttons();
    }

    pub fn update_move_buttons(&self) {
        let row = self.table.selectedRow();
        let manual = self.sort.indexOfSelectedItem() == 1;
        self.move_up.setEnabled(manual && row > 0);
        self.move_down
            .setEnabled(manual && row >= 0 && row + 1 < self.table.numberOfRows());
    }

    pub fn set_status(&self, configuring: bool, status: &str) {
        self.make_default.setEnabled(!configuring);
        self.status.setStringValue(&NSString::from_str(status));
        self.default_warning
            .setHidden(crate::browsers::is_default_browser());
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

pub fn browser_cell(browser: &Browser, mtm: MainThreadMarker, icon_inset: f64) -> Retained<NSView> {
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
            .constraintEqualToAnchor_constant(&cell.leadingAnchor(), icon_inset),
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

pub fn browser_toggle(
    browser: &Browser,
    index: usize,
    enabled: bool,
    target: &Delegate,
) -> Retained<NSView> {
    let mtm = target.mtm();
    let cell = NSTableCellView::new(mtm);
    let toggle = unsafe {
        NSButton::checkboxWithTitle_target_action(
            ns_string!(""),
            Some(target),
            Some(sel!(toggleBrowser:)),
            mtm,
        )
    };
    toggle.setTag(index as isize);
    toggle.setState(if enabled {
        NSControlStateValueOn
    } else {
        NSControlStateValueOff
    });
    let description = NSString::from_str(&format!("Show {} in the browser chooser", browser.name));
    toggle.setAccessibilityLabel(Some(&description));
    toggle.setToolTip(Some(&description));
    toggle.setTranslatesAutoresizingMaskIntoConstraints(false);
    cell.addSubview(&toggle);
    NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&[
        toggle
            .centerXAnchor()
            .constraintEqualToAnchor(&cell.centerXAnchor()),
        toggle
            .centerYAnchor()
            .constraintEqualToAnchor(&cell.centerYAnchor()),
    ]));
    Retained::into_super(cell)
}

pub fn settings(target: &Delegate, configuring: bool) -> Settings {
    let mtm = target.mtm();
    let (window, root) = window("ChooBro Settings", 560.0, 620.0, target);
    let heading = label("Browsers", mtm);
    heading.setFont(Some(&NSFont::boldSystemFontOfSize(20.0)));
    let refresh = button("", ButtonAction::Settings, target);
    let header = stack(mtm, false);
    header.addArrangedSubview(&heading);
    header.addArrangedSubview(&NSView::new(mtm));
    header.addArrangedSubview(&refresh);
    root.addArrangedSubview(&header);
    fill_width(&header, &root);
    let subtitle = label("Show in browser chooser:", mtm);
    subtitle.setTextColor(Some(&NSColor::secondaryLabelColor()));
    let toolbar = stack(mtm, false);
    toolbar.addArrangedSubview(&subtitle);
    toolbar.addArrangedSubview(&NSView::new(mtm));
    toolbar.addArrangedSubview(&label("Sort:", mtm));
    let sort = NSPopUpButton::new(mtm);
    sort.addItemsWithTitles(&NSArray::from_slice(&[
        ns_string!("Alphabetical"),
        ns_string!("Manual"),
    ]));
    sort.setAccessibilityLabel(Some(ns_string!("Browser sort order")));
    unsafe {
        sort.setTarget(Some(target));
        sort.setAction(Some(sel!(changeBrowserSort:)));
    }
    toolbar.addArrangedSubview(&sort);
    root.addArrangedSubview(&toolbar);
    fill_width(&toolbar, &root);

    let table = NSTableView::new(mtm);
    table.setIdentifier(Some(ns_string!("settingsBrowsers")));
    table.registerForDraggedTypes(&NSArray::from_slice(&[browser_drag_type()]));
    table.setDraggingSourceOperationMask_forLocal(NSDragOperation::Move, true);
    table.setDraggingSourceOperationMask_forLocal(NSDragOperation::None, false);
    let enabled =
        NSTableColumn::initWithIdentifier(NSTableColumn::alloc(mtm), ns_string!("enabled"));
    enabled.setWidth(20.0);
    enabled.setMinWidth(20.0);
    enabled.setMaxWidth(20.0);
    let browser =
        NSTableColumn::initWithIdentifier(NSTableColumn::alloc(mtm), ns_string!("browser"));
    browser.setWidth(400.0);
    table.addTableColumn(&enabled);
    table.addTableColumn(&browser);
    table.setHeaderView(None);
    table.setStyle(NSTableViewStyle::Inset);
    table.setRowHeight(44.0);
    table.setAllowsEmptySelection(false);
    table.setAllowsMultipleSelection(false);
    table.setColumnAutoresizingStyle(
        NSTableViewColumnAutoresizingStyle::LastColumnOnlyAutoresizingStyle,
    );
    table.setAccessibilityLabel(Some(ns_string!("Browsers shown in the chooser")));
    unsafe {
        table.setDataSource(Some(ProtocolObject::from_ref(target)));
        table.setDelegate(Some(ProtocolObject::from_ref(target)));
    }
    let scroll = NSScrollView::new(mtm);
    scroll.setDocumentView(Some(&table));
    scroll.setHasVerticalScroller(true);
    scroll.setAutohidesScrollers(true);
    scroll.setBorderType(NSBorderType::BezelBorder);
    let list_area = stack(mtm, false);
    list_area.setAlignment(NSLayoutAttribute::Top);
    list_area.addArrangedSubview(&scroll);
    let moves = stack(mtm, true);
    moves.setSpacing(6.0);
    let move_up = button("", ButtonAction::MoveUp, target);
    let move_down = button("", ButtonAction::MoveDown, target);
    for (control, symbol, description) in [
        (&move_up, "chevron.up", "Move selected browser up"),
        (&move_down, "chevron.down", "Move selected browser down"),
        (&refresh, "arrow.clockwise", "Refresh browsers"),
    ] {
        control.setImage(
            NSImage::imageWithSystemSymbolName_accessibilityDescription(
                &NSString::from_str(symbol),
                Some(&NSString::from_str(description)),
            )
            .as_deref(),
        );
        control.setImagePosition(NSCellImagePosition::ImageOnly);
        control.setAccessibilityLabel(Some(&NSString::from_str(description)));
        control.setToolTip(Some(&NSString::from_str(description)));
        control
            .widthAnchor()
            .constraintEqualToConstant(32.0)
            .setActive(true);
    }
    moves.addArrangedSubview(&move_up);
    moves.addArrangedSubview(&move_down);
    list_area.addArrangedSubview(&moves);
    root.addArrangedSubview(&list_area);
    fill_width(&list_area, &root);
    NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&[
        scroll
            .heightAnchor()
            .constraintEqualToAnchor(&list_area.heightAnchor()),
        scroll
            .heightAnchor()
            .constraintGreaterThanOrEqualToConstant(320.0),
    ]));
    let hint = label("", mtm);
    hint.setTextColor(Some(&NSColor::secondaryLabelColor()));
    hint.setFont(Some(&NSFont::systemFontOfSize(
        NSFont::smallSystemFontSize(),
    )));
    root.addArrangedSubview(&hint);
    fill_width(&hint, &root);
    let divider = NSBox::new(mtm);
    divider.setBoxType(NSBoxType::Separator);
    root.addArrangedSubview(&divider);
    fill_width(&divider, &root);
    divider
        .heightAnchor()
        .constraintEqualToConstant(1.0)
        .setActive(true);
    let default_section = stack(mtm, true);
    default_section.setSpacing(8.0);
    let default_header = stack(mtm, false);
    let default_heading = label("Default browser", mtm);
    default_heading.setFont(Some(&NSFont::boldSystemFontOfSize(15.0)));
    default_header.addArrangedSubview(&default_heading);
    default_header.addArrangedSubview(&NSView::new(mtm));
    let make_default = button("Make Default Browser…", ButtonAction::MakeDefault, target);
    make_default.setEnabled(!configuring);
    default_header.addArrangedSubview(&make_default);
    default_section.addArrangedSubview(&default_header);
    fill_width(&default_header, &default_section);
    let default_warning = NSTextField::wrappingLabelWithString(
        ns_string!("ChooBro must be your default browser to choose where links open."),
        mtm,
    );
    default_warning.setTextColor(Some(&NSColor::systemRedColor()));
    default_warning.setHidden(crate::browsers::is_default_browser());
    default_section.addArrangedSubview(&default_warning);
    fill_width(&default_warning, &default_section);
    let status = label(&crate::browsers::default_status(), mtm);
    status.setMaximumNumberOfLines(3);
    default_section.addArrangedSubview(&status);
    fill_width(&status, &default_section);
    let help = label(
        "You can also change the default browser in System Settings.",
        mtm,
    );
    help.setTextColor(Some(&NSColor::secondaryLabelColor()));
    help.setFont(Some(&NSFont::systemFontOfSize(
        NSFont::smallSystemFontSize(),
    )));
    default_section.addArrangedSubview(&help);
    root.addArrangedSubview(&default_section);
    fill_width(&default_section, &root);
    Settings {
        window,
        status,
        default_warning,
        make_default,
        table,
        sort,
        move_up,
        move_down,
        hint,
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
