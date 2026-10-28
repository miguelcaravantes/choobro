#[path = "../src/browsers.rs"]
mod browsers;
#[path = "../src/requests.rs"]
mod requests;
#[path = "../src/ui.rs"]
mod ui;

mod app {
    include!("../src/app.rs");

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
                    assert!(state.settings.borrow().is_none());
                    delegate.cancel_request(sel!(cancelRequest:), None);
                    delegate.show_settings(sel!(showSettings:), None);
                    {
                        let settings = state.settings.borrow();
                        let settings = settings.as_ref().unwrap();
                        assert!(settings.window.isVisible());
                        let content = settings.window.contentView().unwrap();
                        assert!(
                            content.wantsLayer(),
                            "settings content must use native layer backing"
                        );
                        content.layoutSubtreeIfNeeded();
                        assert!(content.frame().size.height > 0.0);
                        snapshot(&content, ns_string!("target/native-settings.png"));
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
                        state.browsers.borrow().len() as isize
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
                    assert!(state.requests.borrow_mut().begin().is_some());
                    delegate.set_busy(true);
                    assert!(!chooser.open.isEnabled());
                    delegate.dispatch_finished(false, "Test Browser");
                    assert_eq!(chooser.table.selectedRow(), selected);
                    assert!(
                        chooser
                            .message
                            .stringValue()
                            .to_string()
                            .contains("Could not open Test Browser")
                    );
                    assert!(state.requests.borrow_mut().begin().is_some());
                    delegate.dispatch_finished(true, "Test Browser");
                    assert!(state.requests.borrow().current().is_some());
                    delegate.cancel_request(sel!(cancelRequest:), None);
                    assert!(state.requests.borrow().current().is_none());
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
                    assert_eq!(
                        state
                            .requests
                            .borrow()
                            .current()
                            .unwrap()
                            .objectAtIndex(0)
                            .absoluteString()
                            .unwrap()
                            .to_string(),
                        long_url
                    );
                    let open_frame = chooser
                        .open
                        .convertRect_toView(chooser.open.bounds(), Some(&content));
                    assert!(
                        open_frame.origin.x + open_frame.size.width <= content.frame().size.width
                    );
                    assert!(open_frame.origin.y >= 0.0);
                    snapshot(&content, ns_string!("target/native-long-url.png"));
                    delegate.cancel_request(sel!(cancelRequest:), None);
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
    app::smoke_test();
}
