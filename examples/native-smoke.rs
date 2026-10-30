#[path = "../src/app.rs"]
mod app;
#[path = "../src/browsers.rs"]
mod browsers;
#[path = "../src/requests.rs"]
mod requests;
#[path = "../src/ui.rs"]
mod ui;

mod delegate {
    include!("../src/ui/delegate.rs");
    use objc2_foundation::NSIndexSet;

    pub fn smoke_test() {
        let will_launch = RcBlock::new(|_notification: std::ptr::NonNull<NSNotification>| {
            with_delegate(|delegate| {
                assert!(!delegate.ivars().ready.get());
                let url = NSURL::URLWithString(ns_string!("https://example.invalid/cold-launch"))
                    .unwrap();
                let urls = NSArray::from_retained_slice(&[url]);
                delegate.open_urls(
                    sel!(application:openURLs:),
                    &NSApplication::sharedApplication(delegate.mtm()),
                    &urls,
                );
            });
        });
        let center = objc2_foundation::NSNotificationCenter::defaultCenter();
        let _will_token = unsafe {
            center.addObserverForName_object_queue_usingBlock(
                Some(NSApplicationWillFinishLaunchingNotification),
                None,
                None,
                &will_launch,
            )
        };
        let observer = RcBlock::new(|_notification: std::ptr::NonNull<NSNotification>| {
            on_main(|| {
                let _ = std::panic::take_hook();
                with_delegate(|delegate| {
                    let state = delegate.ivars();
                    assert!(state.ready.get());
                    let chooser = state.chooser.get().unwrap();
                    assert!(chooser.window.isVisible());
                    assert!(
                        chooser.window.accessibilityCloseButton().is_none(),
                        "the accessory chooser must be a transient popup for window managers"
                    );
                    assert!(chooser.cancel.isEnabled());
                    assert_eq!(chooser.copy.action(), Some(sel!(copyLink:)));
                    assert_eq!(chooser.copy.keyEquivalent().to_string(), "c");
                    assert_eq!(
                        chooser.copy.keyEquivalentModifierMask(),
                        NSEventModifierFlags::Command
                    );
                    assert!(!chooser.window.isReleasedWhenClosed());
                    assert!(chooser.window.contentViewController().is_some());
                    assert_eq!(chooser.open.action(), Some(sel!(openBrowser:)));
                    assert_eq!(chooser.open.keyEquivalent().to_string(), "\r");
                    assert_eq!(chooser.cancel.action(), Some(sel!(cancelRequest:)));
                    assert_eq!(chooser.cancel.keyEquivalent().to_string(), "\u{1b}");
                    assert!(state.settings.borrow().is_none());
                    delegate.cancel_request(sel!(cancelRequest:), None);
                    assert!(!state.app.has_pending());
                    assert!(!chooser.window.isVisible());
                    delegate.show_settings(sel!(showSettings:), None);
                    {
                        let settings = state.settings.borrow();
                        let settings = settings.as_ref().unwrap();
                        assert!(settings.window.isVisible());
                        assert!(settings.window.accessibilityCloseButton().is_some());
                        assert!(!settings.window.isReleasedWhenClosed());
                        assert!(settings.window.contentViewController().is_some());
                        assert_eq!(settings.make_default.action(), Some(sel!(makeDefault:)));
                        assert_eq!(
                            settings.default_warning.isHidden(),
                            browsers::is_default_browser()
                        );
                        assert_eq!(
                            settings.default_warning.textColor().as_deref(),
                            Some(&*NSColor::systemRedColor())
                        );
                        assert!(
                            settings
                                .default_warning
                                .stringValue()
                                .to_string()
                                .contains("must be your default browser")
                        );
                        settings.set_status(true, "Waiting for macOS approval…");
                        assert!(!settings.make_default.isEnabled());
                        assert_eq!(
                            settings.default_warning.isHidden(),
                            browsers::is_default_browser()
                        );
                        settings.set_status(false, &browsers::default_status());
                        assert!(settings.make_default.isEnabled());
                        let content = settings.window.contentView().unwrap();
                        assert!(
                            content.wantsLayer(),
                            "settings content must use native layer backing"
                        );
                        content.layoutSubtreeIfNeeded();
                        assert!(content.frame().size.height > 0.0);
                        assert_eq!(
                            settings.table.numberOfRows(),
                            state.app.settings_browser_count() as isize
                        );
                        assert_eq!(settings.table.tableColumns().len(), 2);
                        assert!(
                            settings
                                .table
                                .registeredDraggedTypes()
                                .iter()
                                .any(|drag_type| &*drag_type == ui::browser_drag_type())
                        );
                        assert!(
                            delegate
                                .tableView_pasteboardWriterForRow(&chooser.table, 0)
                                .is_none()
                        );
                        assert!(
                            delegate
                                .tableView_pasteboardWriterForRow(&settings.table, -1)
                                .is_none()
                        );
                        assert!(
                            delegate
                                .tableView_pasteboardWriterForRow(
                                    &settings.table,
                                    settings.table.numberOfRows()
                                )
                                .is_none()
                        );
                        let writer = delegate.tableView_pasteboardWriterForRow(&settings.table, 0);
                        assert_eq!(
                            writer.is_some(),
                            browsers::manual_order() && settings.table.numberOfRows() > 0
                        );
                        if let Some(writer) = writer {
                            let pasteboard = NSPasteboard::pasteboardWithUniqueName();
                            assert!(
                                pasteboard.writeObjects(&NSArray::from_retained_slice(&[writer]))
                            );
                            assert_eq!(
                                pasteboard
                                    .stringForType(ui::browser_drag_type())
                                    .unwrap()
                                    .to_string(),
                                state.app.settings_browser(0).unwrap().path
                            );
                            pasteboard.clearContents();
                        }
                        if let Some(browser) = state.app.settings_browser(0) {
                            let count = state.app.settings_browser_count();
                            let expected =
                                (browsers::manual_order() && count > 1).then_some((0, count - 1));
                            assert_eq!(
                                state.app.browser_drop_destination(&browser.path, count),
                                expected
                            );
                            assert!(
                                state
                                    .app
                                    .browser_drop_destination(&browser.path, 0)
                                    .is_none()
                            );
                            assert!(
                                state
                                    .app
                                    .browser_drop_destination(&browser.path, count + 1)
                                    .is_none()
                            );
                            assert!(
                                state
                                    .app
                                    .browser_drop_destination("/Missing.app", count)
                                    .is_none()
                            );
                            assert_eq!(state.app.settings_browser(0).unwrap().path, browser.path);
                        }
                        assert_eq!(settings.sort.action(), Some(sel!(changeBrowserSort:)));
                        assert_eq!(settings.move_up.action(), Some(sel!(moveBrowserUp:)));
                        assert_eq!(settings.move_down.action(), Some(sel!(moveBrowserDown:)));
                        assert!(settings.move_up.image().is_some());
                        assert!(settings.move_down.image().is_some());
                        assert_eq!(
                            settings.move_up.imagePosition(),
                            NSCellImagePosition::ImageOnly
                        );
                        assert!(settings.table.frame().size.height >= 320.0);
                        if settings.table.numberOfRows() >= 7 {
                            let seventh_row = settings.table.rectOfRow(6);
                            let visible = settings.table.visibleRect();
                            assert!(
                                seventh_row.origin.y + seventh_row.size.height
                                    <= visible.origin.y + visible.size.height
                            );
                        }
                        let table_frame = settings
                            .table
                            .enclosingScrollView()
                            .unwrap()
                            .convertRect_toView(
                                settings.table.enclosingScrollView().unwrap().bounds(),
                                Some(&content),
                            );
                        let up_frame = settings
                            .move_up
                            .convertRect_toView(settings.move_up.bounds(), Some(&content));
                        let root = content
                            .subviews()
                            .objectAtIndex(0)
                            .downcast::<NSStackView>()
                            .unwrap();
                        let default_section = root
                            .arrangedSubviews()
                            .lastObject()
                            .unwrap()
                            .downcast::<NSStackView>()
                            .unwrap();
                        assert!(
                            default_section
                                .arrangedSubviews()
                                .iter()
                                .any(|view| &*view == settings.status.as_ref())
                        );
                        let default_header = default_section
                            .arrangedSubviews()
                            .objectAtIndex(0)
                            .downcast::<NSStackView>()
                            .unwrap();
                        assert!(
                            default_header
                                .arrangedSubviews()
                                .iter()
                                .any(|view| &*view == settings.make_default.as_ref())
                        );
                        let heading = default_header
                            .arrangedSubviews()
                            .objectAtIndex(0)
                            .downcast::<NSTextField>()
                            .unwrap();
                        assert_eq!(heading.stringValue().to_string(), "Default browser");
                        let default_frame = default_section
                            .convertRect_toView(default_section.bounds(), Some(&content));
                        assert!(
                            default_frame.origin.y + default_frame.size.height
                                < table_frame.origin.y
                        );
                        assert!(root.arrangedSubviews().iter().any(|view| {
                            view.downcast::<NSBox>()
                                .is_ok_and(|divider| divider.boxType() == NSBoxType::Separator)
                        }));
                        assert!(up_frame.origin.x >= table_frame.origin.x + table_frame.size.width);
                        assert!(
                            up_frame.origin.x + up_frame.size.width
                                <= content.bounds().size.width - 23.9
                        );
                        if let Some(browser) = state.app.settings_browser(0) {
                            for enabled in [false, true] {
                                let view = ui::browser_toggle(&browser, 0, enabled, delegate);
                                let toggle = view
                                    .subviews()
                                    .objectAtIndex(0)
                                    .downcast::<NSButton>()
                                    .unwrap();
                                assert_eq!(toggle.action(), Some(sel!(toggleBrowser:)));
                                assert_eq!(toggle.state() == NSControlStateValueOn, enabled);
                                assert_eq!(toggle.tag(), 0);
                                assert!(
                                    NSAccessibility::accessibilityLabel(&*toggle)
                                        .unwrap()
                                        .to_string()
                                        .contains(&browser.name)
                                );
                            }
                            let cell = ui::browser_cell(&browser, delegate.mtm(), 0.0)
                                .downcast::<NSTableCellView>()
                                .unwrap();
                            assert_eq!(
                                unsafe { cell.textField() }
                                    .unwrap()
                                    .stringValue()
                                    .to_string(),
                                browser.name
                            );
                            assert!(unsafe { cell.imageView() }.unwrap().image().is_some());
                        }
                        for manual in [false, true] {
                            settings.update(0, manual);
                            assert_eq!(settings.sort.indexOfSelectedItem(), isize::from(manual));
                            let count = settings.table.numberOfRows();
                            for row in 0..count {
                                settings.table.selectRowIndexes_byExtendingSelection(
                                    &NSIndexSet::indexSetWithIndex(row as usize),
                                    false,
                                );
                                assert_eq!(settings.move_up.isEnabled(), manual && row > 0);
                                assert_eq!(
                                    settings.move_down.isEnabled(),
                                    manual && row + 1 < count
                                );
                            }
                            if manual {
                                settings.update(0, manual);
                                snapshot(&content, ns_string!("target/native-manual-settings.png"));
                            }
                        }
                        settings.update(0, browsers::manual_order());
                        unsafe { settings.table.setDataSource(None) };
                        settings.update(0, true);
                        assert_eq!(settings.table.numberOfRows(), 0);
                        assert!(!settings.move_up.isEnabled());
                        assert!(!settings.move_down.isEnabled());
                        unsafe {
                            settings
                                .table
                                .setDataSource(Some(ProtocolObject::from_ref(delegate)))
                        };
                        settings.update(0, browsers::manual_order());
                        snapshot(&content, ns_string!("target/native-settings.png"));
                        settings.default_warning.setHidden(false);
                        content.layoutSubtreeIfNeeded();
                        assert!(settings.table.frame().size.height >= 320.0);
                        snapshot(&content, ns_string!("target/native-default-warning.png"));
                        settings.set_status(false, &browsers::default_status());
                        settings.window.close();
                        assert!(!settings.window.isVisible());
                        assert_eq!(settings.window.title().to_string(), "ChooBro Settings");
                    }
                    delegate.show_settings(sel!(showSettings:), None);
                    assert!(state.settings.borrow().as_ref().unwrap().window.isVisible());
                    if state.app.settings_browser_count() > 1 {
                        let table = state.settings.borrow().as_ref().unwrap().table.clone();
                        table.selectRowIndexes_byExtendingSelection(
                            &NSIndexSet::indexSetWithIndex(1),
                            false,
                        );
                        let selected_path = state.app.settings_browser(1).unwrap().path;
                        delegate.show_settings(sel!(showSettings:), None);
                        assert_eq!(&*state.settings.borrow().as_ref().unwrap().table, &*table);
                        assert_eq!(
                            state
                                .app
                                .settings_browser(table.selectedRow() as usize)
                                .unwrap()
                                .path,
                            selected_path
                        );
                    }
                    let url = NSURL::URLWithString(ns_string!(
                        "https://example.invalid/a%2Fb?x=a%26b#test"
                    ))
                    .unwrap();
                    let urls = NSArray::from_retained_slice(&[url]);
                    let application = NSApplication::sharedApplication(delegate.mtm());
                    delegate.open_urls(sel!(application:openURLs:), &application, &urls);
                    assert!(chooser.window.isVisible());
                    assert!(
                        chooser.window.contentView().unwrap().wantsLayer(),
                        "chooser content must use native layer backing"
                    );
                    chooser
                        .window
                        .contentView()
                        .unwrap()
                        .layoutSubtreeIfNeeded();
                    assert!(chooser.table.frame().size.height >= 100.0);
                    assert!(chooser.table.frame().size.width > 0.0);
                    assert_eq!(
                        chooser.table.numberOfRows(),
                        state.app.browser_count() as isize
                    );
                    assert_eq!(
                        chooser.preview.stringValue().to_string(),
                        "https://example.invalid/a%2Fb?x=a%26b#test"
                    );
                    snapshot(
                        &chooser.window.contentView().unwrap(),
                        ns_string!("target/native-chooser.png"),
                    );
                    if chooser.table.numberOfRows() > 1 {
                        chooser.table.selectRowIndexes_byExtendingSelection(
                            &NSIndexSet::indexSetWithIndex(1),
                            false,
                        );
                    }
                    let selected = chooser.table.selectedRow();
                    delegate.open_urls(sel!(application:openURLs:), &application, &urls);
                    assert_eq!(chooser.table.selectedRow(), selected);
                    assert!(state.app.begin_dispatch().is_some());
                    delegate.set_busy(true);
                    assert!(!chooser.open.isEnabled());
                    assert!(!chooser.copy.isEnabled());
                    assert!(state.app.busy());
                    assert!(state.app.has_pending());
                    assert!(
                        !chooser.window.isVisible(),
                        "dismiss the chooser before asking macOS to activate the browser"
                    );
                    delegate.refresh_chooser(true);
                    assert!(
                        !chooser.window.isVisible(),
                        "an in-flight handoff must not present the chooser again"
                    );
                    delegate.dispatch_finished(false, "Test Browser");
                    assert!(chooser.window.isVisible());
                    assert!(chooser.open.isEnabled());
                    assert!(chooser.copy.isEnabled());
                    assert_eq!(chooser.table.selectedRow(), selected);
                    assert!(
                        chooser
                            .message
                            .stringValue()
                            .to_string()
                            .contains("Could not open Test Browser")
                    );
                    assert!(state.app.begin_dispatch().is_some());
                    delegate.set_busy(true);
                    assert!(!chooser.window.isVisible());
                    delegate.dispatch_finished(true, "Test Browser");
                    assert!(state.app.has_pending());
                    assert!(chooser.window.isVisible());
                    assert!(state.app.begin_dispatch().is_some());
                    delegate.set_busy(true);
                    assert!(!chooser.window.isVisible());
                    delegate.dispatch_finished(true, "Test Browser");
                    assert!(!state.app.has_pending());
                    assert!(!chooser.window.isVisible());
                    assert!(chooser.preview.stringValue().is_empty());
                    let long_url = format!(
                        "https://example.invalid/safelink?url={}",
                        "a%2Fb%3Dc%26d".repeat(1000)
                    );
                    let url =
                        NSURL::URLWithString(&objc2_foundation::NSString::from_str(&long_url))
                            .unwrap();
                    delegate.open_urls(
                        sel!(application:openURLs:),
                        &application,
                        &NSArray::from_retained_slice(&[url]),
                    );
                    let content = chooser.window.contentView().unwrap();
                    content.layoutSubtreeIfNeeded();
                    chooser.window.displayIfNeeded();
                    assert!(
                        (content.frame().size.width - 480.0).abs() < 1.0,
                        "a long URL must not widen the chooser: {:?}",
                        content.frame()
                    );
                    let preview_rect = chooser
                        .preview
                        .alignmentRectForFrame(chooser.preview.frame());
                    assert!(
                        preview_rect.size.width <= 433.0,
                        "preview alignment rect: {preview_rect:?}"
                    );
                    assert!(
                        chooser.preview.frame().size.height > 20.0,
                        "a long URL must wrap onto multiple lines"
                    );
                    assert!(!chooser.preview.cell().unwrap().usesSingleLineMode());
                    assert!(chooser.preview.cell().unwrap().wraps());
                    assert!(chooser.preview.cell().unwrap().truncatesLastVisibleLine());
                    assert!(!chooser.preview.isSelectable());
                    assert!(!chooser.preview.isEditable());
                    assert!(!chooser.preview.acceptsFirstResponder());
                    assert_eq!(chooser.preview.maximumNumberOfLines(), 3);
                    assert_eq!(chooser.preview.toolTip().unwrap().to_string(), long_url);
                    assert_eq!(chooser.preview.stringValue().to_string(), long_url);
                    let pasteboard = NSPasteboard::pasteboardWithUniqueName();
                    delegate.copy_link_to(&pasteboard);
                    assert_eq!(
                        pasteboard
                            .stringForType(ns_string!("public.utf8-plain-text"))
                            .unwrap()
                            .to_string(),
                        long_url
                    );
                    assert_eq!(chooser.message.stringValue().to_string(), "Link copied.");
                    assert!(!state.app.busy());
                    assert_eq!(state.app.preview().unwrap().to_string(), long_url);
                    let open_frame = chooser
                        .open
                        .convertRect_toView(chooser.open.bounds(), Some(&content));
                    assert!(
                        open_frame.origin.x + open_frame.size.width <= content.frame().size.width
                    );
                    assert!(open_frame.origin.y >= 0.0);
                    let copy_frame = chooser
                        .copy
                        .convertRect_toView(chooser.copy.bounds(), Some(&content));
                    assert!(copy_frame.origin.x >= 0.0);
                    assert!(
                        copy_frame.origin.x + copy_frame.size.width <= content.frame().size.width
                    );
                    assert!(copy_frame.origin.y >= 0.0);
                    snapshot(&content, ns_string!("target/native-long-url.png"));
                    delegate.cancel_request(sel!(cancelRequest:), None);
                    delegate.copy_link_to(&pasteboard);
                    assert_eq!(
                        pasteboard
                            .stringForType(ns_string!("public.utf8-plain-text"))
                            .unwrap()
                            .to_string(),
                        long_url
                    );
                    pasteboard.clearContents();
                    println!(
                        "Native smoke test passed: pre-launch and warm URL callbacks, settings, chooser layout, stable selection, failure/retry, queue, cancellation, long-URL wrapping and bounded window geometry. Snapshots: target/native-chooser.png, target/native-settings.png and target/native-long-url.png"
                    );
                });
                std::process::exit(0);
            })
        });
        let _token = unsafe {
            objc2_foundation::NSNotificationCenter::defaultCenter()
                .addObserverForName_object_queue_usingBlock(
                    Some(NSApplicationDidFinishLaunchingNotification),
                    None,
                    None,
                    &observer,
                )
        };
        run();
    }

    fn snapshot(view: &NSView, path: &objc2_foundation::NSString) {
        view.window().unwrap().displayIfNeeded();
        let bitmap = view
            .bitmapImageRepForCachingDisplayInRect(view.bounds())
            .unwrap();
        view.cacheDisplayInRect_toBitmapImageRep(view.bounds(), &bitmap);
        let png = unsafe {
            bitmap.representationUsingType_properties(
                NSBitmapImageFileType::PNG,
                &objc2_foundation::NSDictionary::new(),
            )
        }
        .unwrap();
        assert!(png.writeToFile_atomically(path, true));
    }
}

fn main() {
    delegate::smoke_test();
}
