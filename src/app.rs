#![forbid(unsafe_code)]

use std::cell::{Cell, RefCell};

use objc2::rc::Retained;
use objc2_foundation::{NSArray, NSString, NSURL, ns_string};

use crate::{
    browsers::{self, Browser},
    requests::{Rejection, Requests},
};

#[derive(Default)]
pub struct App {
    requests: RefCell<Requests>,
    browsers: RefCell<Vec<Browser>>,
    settings_browsers: RefCell<Vec<Browser>>,
    rules: RefCell<Vec<browsers::Rule>>,
    rules_loaded: Cell<bool>,
    configuring: Cell<bool>,
    default_failed: Cell<bool>,
    timeout_unclaimed: Cell<bool>,
    route_unclaimed: Cell<bool>,
}

#[derive(Debug, PartialEq)]
pub enum Incoming {
    Show,
    Queued,
    Rejected(Rejection),
}

pub enum OpenRequest {
    None,
    Unavailable,
    ProfileBatchTooLarge,
    Ready(
        Browser,
        Retained<NSArray<NSURL>>,
        Option<Retained<NSArray<NSString>>>,
    ),
}

pub struct ChooserContent {
    pub preview: Retained<NSString>,
    pub domain: Option<String>,
    pub selected_row: usize,
    pub count: usize,
    pub profile_notice: Option<browsers::ProfileNotice>,
}

impl App {
    pub fn receive(&self, urls: &NSArray<NSURL>) -> Incoming {
        let mut requests = self.requests.borrow_mut();
        let pending = requests.current().is_some();
        match requests.push(urls) {
            Err(reason) => Incoming::Rejected(reason),
            Ok(()) if pending => Incoming::Queued,
            Ok(()) => {
                self.timeout_unclaimed.set(true);
                self.route_unclaimed.set(true);
                Incoming::Show
            }
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

    pub fn current_domain(&self) -> Option<String> {
        let requests = self.requests.borrow();
        browsers::shared_domain(requests.current()?)
    }

    pub fn claim_route(&self) -> Option<browsers::RuleRoute> {
        self.claim_route_with(browsers::rules)
    }

    fn claim_route_with(
        &self,
        load: impl FnOnce() -> Vec<browsers::Rule>,
    ) -> Option<browsers::RuleRoute> {
        if !self.has_pending() || self.busy() || !self.route_unclaimed.replace(false) {
            return None;
        }
        let requests = self.requests.borrow();
        let rules = load();
        let route = browsers::rule_route(&rules, requests.current()?)?;
        if route == browsers::RuleRoute::MixedProfiles {
            self.cancel_timeout();
        }
        *self.rules.borrow_mut() = rules;
        self.rules_loaded.set(true);
        Some(route)
    }

    pub fn route_target(&self, key: &str) -> Option<usize> {
        self.browsers
            .borrow()
            .iter()
            .position(|browser| browser.target_key() == key)
    }

    pub fn refresh(&self, selected: Option<usize>) -> Option<ChooserContent> {
        let (schemes, preview, domain, count) = {
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
                browsers::shared_domain(urls),
                urls.len(),
            )
        };
        let selected = selected.and_then(|index| self.browser(index));
        let excluded = browsers::exclusions();
        let mut browsers: Vec<_> = browsers::discover(&schemes)
            .into_iter()
            .filter(|browser| !excluded.contains(&browser.id))
            .collect();
        let profile_notice = browsers::add_profile_choices(&mut browsers);
        browsers.retain(|browser| browser.is_visible(&excluded));
        let selected_row = selected_browser_row(&browsers, selected.as_ref());
        *self.browsers.borrow_mut() = browsers;
        Some(ChooserContent {
            preview,
            domain,
            selected_row,
            count,
            profile_notice,
        })
    }

    pub fn settings_browser_count(&self) -> usize {
        self.settings_browsers.borrow().len()
    }

    pub fn settings_browser(&self, index: usize) -> Option<Browser> {
        self.settings_browsers.borrow().get(index).cloned()
    }

    pub fn settings_rule_targets(&self) -> Vec<Browser> {
        self.settings_browsers.borrow().clone()
    }

    pub fn settings_regular_browsers(&self) -> Vec<Browser> {
        self.settings_browsers
            .borrow()
            .iter()
            .filter(|browser| browser.profile.is_none())
            .cloned()
            .collect()
    }

    pub fn refresh_settings(
        &self,
        selected: Option<usize>,
    ) -> (usize, Option<browsers::ProfileNotice>) {
        let selected = selected.and_then(|index| self.settings_browser(index));
        let mut browsers = browsers::discover_all();
        let profile_notice = browsers::add_profile_choices(&mut browsers);
        let selected_row = selected_browser_row(&browsers, selected.as_ref());
        *self.settings_browsers.borrow_mut() = browsers;
        *self.rules.borrow_mut() = browsers::rules();
        self.rules_loaded.set(true);
        (selected_row, profile_notice)
    }

    pub fn rule_count(&self) -> usize {
        self.rules.borrow().len()
    }

    pub fn rule(&self, index: usize) -> Option<browsers::Rule> {
        self.rules.borrow().get(index).cloned()
    }

    pub fn save_domain_rule(&self, domain: &str, target_key: &str) -> Option<usize> {
        let pattern = browsers::normalize_domain(domain)?;
        if target_key.is_empty() {
            return None;
        }
        if !self.rules_loaded.replace(true) {
            *self.rules.borrow_mut() = browsers::rules();
        }
        let mut rules = self.rules.borrow_mut();
        browsers::upsert_rule(
            &mut rules,
            browsers::Rule {
                kind: browsers::MatchKind::Domain,
                pattern: pattern.clone(),
                target_key: target_key.to_owned(),
            },
        );
        let index = rules
            .iter()
            .position(|rule| rule.pattern == pattern)
            .unwrap_or(0);
        browsers::save_rules(&rules);
        Some(index)
    }

    pub fn remove_rule(&self, index: usize) -> Option<usize> {
        let mut rules = self.rules.borrow_mut();
        if index >= rules.len() {
            return None;
        }
        rules.remove(index);
        let selected = index.min(rules.len().saturating_sub(1));
        browsers::save_rules(&rules);
        Some(selected)
    }

    pub fn set_browser_enabled(&self, index: usize, enabled: bool) {
        if let Some(browser) = self.settings_browsers.borrow().get(index) {
            browsers::set_excluded(&browser.target_key(), !enabled);
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

    pub fn browser_drop_destination(&self, key: &str, insertion: usize) -> Option<(usize, usize)> {
        if !browsers::manual_order() {
            return None;
        }
        let browsers = self.settings_browsers.borrow();
        let source = browsers
            .iter()
            .position(|browser| browser.order_key() == key)?;
        let destination = browsers::drop_destination(source, insertion, browsers.len())?;
        Some((source, destination))
    }

    pub fn drop_browser(&self, key: &str, insertion: usize) -> Option<usize> {
        let (source, destination) = self.browser_drop_destination(key, insertion)?;
        self.move_browser(source, destination as isize - source as isize)
            .then_some(destination)
    }

    pub fn claim_timeout(&self) -> bool {
        self.has_pending() && !self.busy() && self.timeout_unclaimed.replace(false)
    }

    pub fn cancel_timeout(&self) {
        self.timeout_unclaimed.set(false);
    }

    pub fn timeout_browser(&self, id: &str) -> Option<usize> {
        self.browsers
            .borrow()
            .iter()
            .position(|browser| browser.id == id && browser.profile.is_none())
    }

    pub fn cancel(&self) {
        if !self.busy() {
            self.requests.borrow_mut().cancel();
            let pending = self.has_pending();
            self.timeout_unclaimed.set(pending);
            self.route_unclaimed.set(pending);
        }
    }

    pub fn begin_dispatch(&self) -> Option<Retained<NSArray<NSURL>>> {
        self.cancel_timeout();
        self.route_unclaimed.set(false);
        self.requests.borrow_mut().begin()
    }

    pub fn open(&self, index: usize) -> OpenRequest {
        if self.busy() {
            return OpenRequest::None;
        }
        let Some(browser) = self.browser(index) else {
            return OpenRequest::None;
        };
        let arguments = if let Some(directory) = &browser.profile {
            let requests = self.requests.borrow();
            let Some(urls) = requests.current() else {
                return OpenRequest::None;
            };
            let Some(arguments) = browsers::chrome_arguments(directory, urls) else {
                return OpenRequest::ProfileBatchTooLarge;
            };
            Some(arguments)
        } else {
            None
        };
        if !browsers::available(&browser) {
            return OpenRequest::Unavailable;
        }
        match self.begin_dispatch() {
            Some(urls) => OpenRequest::Ready(browser, urls, arguments),
            None => OpenRequest::None,
        }
    }

    pub fn dispatch_finished(&self, success: bool) -> bool {
        self.requests.borrow_mut().finish(success);
        let next = success && self.has_pending();
        self.timeout_unclaimed.set(next);
        self.route_unclaimed.set(next);
        next
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

fn selected_browser_row(rows: &[Browser], selection: Option<&Browser>) -> usize {
    selection
        .and_then(|selected| {
            rows.iter()
                .position(|row| row.path == selected.path && row.profile == selected.profile)
                .or_else(|| {
                    rows.iter()
                        .position(|row| row.path == selected.path && row.profile.is_none())
                })
        })
        .unwrap_or(0)
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
        assert_eq!(
            app.receive(&urls("file:///private")),
            Incoming::Rejected(Rejection::Unsupported)
        );
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
    fn a_full_queue_reports_the_limit_without_replacing_pending_links() {
        let app = App::default();
        let url = urls("https://first.example").objectAtIndex(0);
        let batch = NSArray::from_retained_slice(&vec![url; 256]);
        assert_eq!(app.receive(&batch), Incoming::Show);
        assert_eq!(
            app.receive(&urls("https://second.example")),
            Incoming::Rejected(Rejection::Full)
        );
        assert_eq!(app.preview().unwrap().to_string(), "https://first.example");
        app.cancel();
        assert_eq!(app.receive(&urls("https://second.example")), Incoming::Show);
    }

    #[test]
    fn route_claims_once_per_request_and_respects_exclusions() {
        let app = App::default();
        assert!(app.claim_route().is_none());
        app.receive(&urls("https://first.example/path"));
        let _ = app.claim_route();
        assert!(app.claim_route().is_none());
        app.begin_dispatch();
        app.dispatch_finished(false);
        assert!(app.claim_route().is_none());
        app.cancel();
        assert!(!app.has_pending());
    }

    #[test]
    fn timeout_is_once_per_request_and_never_restarts_after_interaction_or_failure() {
        let app = App::default();
        assert!(!app.claim_timeout());
        app.receive(&urls("https://first.example"));
        assert!(app.claim_timeout());
        app.receive(&urls("https://second.example"));
        assert!(!app.claim_timeout());
        app.cancel_timeout();
        assert!(!app.claim_timeout());
        app.begin_dispatch();
        app.dispatch_finished(false);
        assert!(!app.claim_timeout());
        app.begin_dispatch();
        app.dispatch_finished(true);
        assert!(app.claim_timeout());
        app.cancel();
        assert!(!app.claim_timeout());
        app.receive(&urls("https://third.example"));
        app.cancel_timeout();
        assert!(!app.claim_timeout());
    }

    #[test]
    fn domain_rules_match_exact_profiles_while_auto_open_uses_regular_browsers() {
        let app = App::default();
        let chrome = Browser {
            id: "com.google.Chrome".into(),
            name: "Chrome".into(),
            path: "/Chrome.app".into(),
            url: NSURL::fileURLWithPath(ns_string!("/Chrome.app")),
            icon: objc2_app_kit::NSImage::new(),
            profile: None,
        };
        let profile = Browser {
            profile: Some("Default".into()),
            ..chrome.clone()
        };
        let key = profile.target_key();
        app.browsers.borrow_mut().push(chrome.clone());
        assert_eq!(app.route_target(&key), None);
        app.browsers.borrow_mut().insert(0, profile);
        assert_eq!(app.route_target(&key), Some(0));
        assert_eq!(app.route_target(&chrome.id), Some(1));
        assert_eq!(app.timeout_browser(&chrome.id), Some(1));
    }

    #[test]
    fn mixed_batch_with_a_hidden_profile_rule_cancels_standalone_auto_open() {
        let app = App::default();
        let batch = NSArray::from_retained_slice(&[
            urls("https://work.example").objectAtIndex(0),
            urls("https://unruled.example").objectAtIndex(0),
        ]);
        app.receive(&batch);
        let hidden_profile = "profile:com.google.Chrome/Profile 1";
        assert!(app.route_target(hidden_profile).is_none());
        assert_eq!(
            app.claim_route_with(|| vec![browsers::Rule {
                kind: browsers::MatchKind::Domain,
                pattern: "work.example".into(),
                target_key: hidden_profile.into(),
            }]),
            Some(browsers::RuleRoute::MixedProfiles)
        );
        assert!(!app.claim_timeout());
        assert!(app.has_pending());
    }

    #[test]
    fn oversized_profile_batch_is_rejected_before_dispatch_begins() {
        let app = App::default();
        let profile = Browser {
            id: "com.google.Chrome".into(),
            name: "Chrome — Work".into(),
            path: "/Chrome.app".into(),
            url: NSURL::fileURLWithPath(ns_string!("/Chrome.app")),
            icon: objc2_app_kit::NSImage::new(),
            profile: Some("Default".into()),
        };
        app.browsers.borrow_mut().push(profile);
        app.receive(&urls(&format!(
            "https://example.com/{}",
            "a".repeat(25 * 1024)
        )));
        assert!(matches!(app.open(0), OpenRequest::ProfileBatchTooLarge));
        assert!(!app.busy());
        assert!(app.has_pending());
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
