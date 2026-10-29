#![forbid(unsafe_code)]

use std::{
    cell::{Cell, RefCell},
    collections::BTreeSet,
};

use objc2::rc::Retained;
use objc2_foundation::{NSArray, NSString, NSURL, ns_string};

use crate::{
    browsers::{self, Browser},
    requests::Requests,
};

#[derive(Default)]
pub struct App {
    requests: RefCell<Requests>,
    browsers: RefCell<Vec<Browser>>,
    settings_browsers: RefCell<Vec<Browser>>,
    configuring: Cell<bool>,
    default_failed: Cell<bool>,
}

#[derive(Debug, PartialEq)]
pub enum Incoming {
    Show,
    Queued,
    Rejected,
}

pub enum OpenRequest {
    None,
    Unavailable,
    Ready(Browser, Retained<NSArray<NSURL>>),
}

pub struct ChooserContent {
    pub preview: Retained<NSString>,
    pub selected_row: usize,
    pub count: usize,
}

impl App {
    pub fn receive(&self, urls: &NSArray<NSURL>) -> Incoming {
        let mut requests = self.requests.borrow_mut();
        let pending = requests.current().is_some();
        if !requests.push(urls) {
            Incoming::Rejected
        } else if pending {
            Incoming::Queued
        } else {
            Incoming::Show
        }
    }

    pub fn has_pending(&self) -> bool {
        self.requests.borrow().current().is_some()
    }

    pub fn busy(&self) -> bool {
        self.requests.borrow().in_flight
    }

    pub fn preview(&self) -> Option<Retained<NSString>> {
        self.requests
            .borrow()
            .current()
            .and_then(|urls| urls.objectAtIndex(0).absoluteString())
    }

    pub fn browser_count(&self) -> usize {
        self.browsers.borrow().len()
    }

    pub fn browser(&self, index: usize) -> Option<Browser> {
        self.browsers.borrow().get(index).cloned()
    }

    pub fn refresh(&self, selected: Option<usize>) -> Option<ChooserContent> {
        let (schemes, preview, count) = {
            let requests = self.requests.borrow();
            let urls = requests.current()?;
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
            )
        };
        let selected_path = selected
            .and_then(|index| self.browser(index))
            .map(|browser| browser.path);
        let excluded = browsers::exclusions();
        let browsers: Vec<_> = browsers::discover(&schemes)
            .into_iter()
            .filter(|browser| !excluded.contains(&browser.id))
            .collect();
        let selected_row = browsers
            .iter()
            .position(|browser| Some(&browser.path) == selected_path.as_ref())
            .unwrap_or(0);
        *self.browsers.borrow_mut() = browsers;
        Some(ChooserContent {
            preview,
            selected_row,
            count,
        })
    }

    pub fn refresh_settings(&self) -> (Vec<Browser>, BTreeSet<String>) {
        let browsers = browsers::discover(&BTreeSet::from(["http".into(), "https".into()]));
        *self.settings_browsers.borrow_mut() = browsers.clone();
        (browsers, browsers::exclusions())
    }

    pub fn set_browser_enabled(&self, index: usize, enabled: bool) {
        if let Some(browser) = self.settings_browsers.borrow().get(index) {
            browsers::set_excluded(&browser.id, !enabled);
        }
    }

    pub fn move_browser(&self, index: usize, offset: isize) -> bool {
        if !browsers::manual_order() {
            return false;
        }
        let mut browsers = self.settings_browsers.borrow_mut();
        if !browsers::move_browser(&mut browsers, index, offset) {
            return false;
        }
        browsers::set_browser_order(&browsers);
        true
    }

    pub fn cancel(&self) {
        self.requests.borrow_mut().cancel();
    }

    pub fn begin_dispatch(&self) -> Option<Retained<NSArray<NSURL>>> {
        self.requests.borrow_mut().begin()
    }

    pub fn open(&self, index: usize) -> OpenRequest {
        let Some(browser) = self.browser(index) else {
            return OpenRequest::None;
        };
        if !browsers::available(&browser) {
            return OpenRequest::Unavailable;
        }
        match self.begin_dispatch() {
            Some(urls) => OpenRequest::Ready(browser, urls),
            None => OpenRequest::None,
        }
    }

    pub fn dispatch_finished(&self, success: bool) -> bool {
        self.requests.borrow_mut().finish(success);
        success && self.has_pending()
    }

    pub fn configuring(&self) -> bool {
        self.configuring.get()
    }

    pub fn begin_default_change(&self) -> bool {
        if self.configuring.replace(true) {
            return false;
        }
        self.default_failed.set(false);
        true
    }

    pub fn default_scheme_finished(&self, index: usize, failed: bool) -> Option<String> {
        if failed {
            self.default_failed.set(true);
        }
        if index == 0 {
            return None;
        }
        self.configuring.set(false);
        let mut status = browsers::default_status();
        if self.default_failed.get() {
            status.push_str("\nA change was declined or failed. You can use System Settings.");
        }
        Some(status)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn urls(value: &str) -> Retained<NSArray<NSURL>> {
        NSArray::from_retained_slice(&[NSURL::URLWithString(&NSString::from_str(value)).unwrap()])
    }

    #[test]
    fn incoming_links_only_present_the_first_pending_request() {
        let app = App::default();
        assert_eq!(app.receive(&urls("https://first.example")), Incoming::Show);
        assert_eq!(
            app.receive(&urls("https://second.example")),
            Incoming::Queued
        );
        assert_eq!(app.receive(&urls("file:///private")), Incoming::Rejected);
        assert_eq!(app.preview().unwrap().to_string(), "https://first.example");
        app.cancel();
        assert_eq!(app.preview().unwrap().to_string(), "https://second.example");
        app.cancel();
        assert!(!app.has_pending());
    }

    #[test]
    fn failed_dispatch_keeps_the_request_and_success_presents_the_next() {
        let app = App::default();
        app.receive(&urls("https://first.example"));
        app.receive(&urls("https://second.example"));
        assert!(app.begin_dispatch().is_some());
        assert!(app.begin_dispatch().is_none());
        assert!(app.busy());
        app.cancel();
        assert!(!app.dispatch_finished(false));
        assert_eq!(app.preview().unwrap().to_string(), "https://first.example");
        assert!(app.begin_dispatch().is_some());
        assert!(app.dispatch_finished(true));
        assert_eq!(app.preview().unwrap().to_string(), "https://second.example");
        assert!(app.begin_dispatch().is_some());
        assert!(!app.dispatch_finished(true));
        assert!(!app.has_pending());
        assert!(!app.busy());
    }

    #[test]
    fn invalid_selection_does_not_start_dispatch() {
        let app = App::default();
        app.receive(&urls("https://example.com"));
        assert!(matches!(app.open(0), OpenRequest::None));
        assert!(!app.busy());
        assert!(app.has_pending());
    }

    #[test]
    fn default_change_waits_for_both_schemes_and_retains_partial_failure() {
        let app = App::default();
        assert!(app.begin_default_change());
        assert!(!app.begin_default_change());
        assert!(app.default_scheme_finished(0, true).is_none());
        assert!(app.configuring());
        let status = app.default_scheme_finished(1, false).unwrap();
        assert!(status.contains("declined or failed"));
        assert!(!app.configuring());
        assert!(app.begin_default_change());
        assert!(app.default_scheme_finished(0, false).is_none());
        let status = app.default_scheme_finished(1, false).unwrap();
        assert!(!status.contains("declined or failed"));
    }
}
