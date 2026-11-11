use std::{collections::BTreeSet, path::Path};

use block2::RcBlock;
use objc2::{rc::Retained, runtime::AnyObject};
use objc2_app_kit::{NSImage, NSRunningApplication, NSWorkspace, NSWorkspaceOpenConfiguration};
use objc2_foundation::{
    NSArray, NSBundle, NSDictionary, NSError, NSString, NSURL, NSUserDefaults, ns_string,
};

pub const BUNDLE_ID: &str = "io.github.miguelcaravantes.choobro";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatchKind {
    Domain,
}

impl MatchKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Domain => "domain",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "domain" => Some(Self::Domain),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    pub kind: MatchKind,
    pub pattern: String,
    pub browser_id: String,
}

impl Rule {
    pub fn matches(&self, url: &NSURL) -> bool {
        match self.kind {
            MatchKind::Domain => url_domain(url).is_some_and(|domain| {
                if let Some(suffix) = self.pattern.strip_prefix("*.") {
                    domain == suffix
                        || domain
                            .strip_suffix(suffix)
                            .is_some_and(|prefix| prefix.ends_with('.'))
                } else {
                    domain == self.pattern
                }
            }),
        }
    }
}

pub fn normalize_domain(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    let (wildcard, rest) = if let Some(after) = trimmed.strip_prefix("*.") {
        (true, after)
    } else {
        (false, trimmed)
    };
    if rest.contains('*') {
        return None;
    }
    let parsed_host = if rest.contains("://") {
        if wildcard {
            return None;
        }
        NSURL::URLWithString(&NSString::from_str(rest))
            .and_then(|url| url.host())
            .map(|host| host.to_string())?
    } else {
        let host_port = rest.split(['/', '?', '#']).next()?;
        if let Some(bracketed) = host_port.strip_prefix('[') {
            if wildcard {
                return None;
            }
            let (ipv6, rem) = bracketed.split_once(']')?;
            if !rem.is_empty() && !rem.starts_with(':') {
                return None;
            }
            format!("[{ipv6}]")
        } else if let Some((host, port)) = host_port.rsplit_once(':') {
            if port.chars().all(|c| c.is_ascii_digit()) && !port.is_empty() {
                host.to_owned()
            } else {
                return None;
            }
        } else {
            host_port.to_owned()
        }
    };
    let domain = parsed_host.trim_matches('.').to_ascii_lowercase();
    if domain.is_empty()
        || domain
            .split('.')
            .any(|label| label.is_empty() || label.starts_with('-') || label.ends_with('-'))
        || domain
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '/' | '?' | '#' | '@' | '\\' | '*'))
    {
        return None;
    }
    Some(if wildcard {
        format!("*.{domain}")
    } else {
        domain
    })
}

pub fn url_domain(url: &NSURL) -> Option<String> {
    url.host()
        .and_then(|host| normalize_domain(&host.to_string()))
}

pub fn shared_domain(urls: &NSArray<NSURL>) -> Option<String> {
    let first = url_domain(&urls.objectAtIndex(0))?;
    urls.iter()
        .all(|url| url_domain(&url).as_deref() == Some(first.as_str()))
        .then_some(first)
}

pub fn rules() -> Vec<Rule> {
    rules_from_defaults(&NSUserDefaults::standardUserDefaults())
}

fn rules_from_defaults(defaults: &NSUserDefaults) -> Vec<Rule> {
    let Some(items) = defaults.arrayForKey(ns_string!("RoutingRules")) else {
        return Vec::new();
    };
    let mut loaded = Vec::new();
    for item in items {
        let Ok(dict) = item.downcast::<NSDictionary>() else {
            continue;
        };
        let Some(kind) = dict
            .objectForKey(ns_string!("kind"))
            .and_then(|v| v.downcast::<NSString>().ok())
            .and_then(|v| MatchKind::parse(&v.to_string()))
        else {
            continue;
        };
        let Some(pattern) = dict
            .objectForKey(ns_string!("pattern"))
            .and_then(|v| v.downcast::<NSString>().ok())
            .and_then(|v| match kind {
                MatchKind::Domain => normalize_domain(&v.to_string()),
            })
        else {
            continue;
        };
        let Some(browser_id) = dict
            .objectForKey(ns_string!("browser"))
            .and_then(|v| v.downcast::<NSString>().ok())
            .map(|v| v.to_string())
            .filter(|v| !v.is_empty())
        else {
            continue;
        };
        if let Some(existing) = loaded
            .iter_mut()
            .find(|rule: &&mut Rule| rule.kind == kind && rule.pattern == pattern)
        {
            existing.browser_id = browser_id;
        } else {
            loaded.push(Rule {
                kind,
                pattern,
                browser_id,
            });
        }
    }
    loaded
}

pub fn save_rules(rules: &[Rule]) {
    save_rules_to_defaults(&NSUserDefaults::standardUserDefaults(), rules);
}

fn save_rules_to_defaults(defaults: &NSUserDefaults, rules: &[Rule]) {
    let dicts: Vec<_> = rules
        .iter()
        .map(|rule| {
            let kind = NSString::from_str(rule.kind.as_str());
            let pattern = NSString::from_str(&rule.pattern);
            let browser = NSString::from_str(&rule.browser_id);
            NSDictionary::<NSString, AnyObject>::from_slices(
                &[
                    ns_string!("kind"),
                    ns_string!("pattern"),
                    ns_string!("browser"),
                ],
                &[&*kind, &*pattern, &*browser],
            )
        })
        .collect();
    let array = NSArray::from_retained_slice(&dicts);
    unsafe {
        defaults.setObject_forKey(Some(&array), ns_string!("RoutingRules"));
    }
}

pub fn upsert_rule(rules: &mut Vec<Rule>, rule: Rule) {
    if let Some(existing) = rules
        .iter_mut()
        .find(|existing| existing.kind == rule.kind && existing.pattern == rule.pattern)
    {
        existing.browser_id = rule.browser_id;
    } else {
        rules.push(rule);
    }
}

pub fn matching_rule_browser(rules: &[Rule], urls: &NSArray<NSURL>) -> Option<String> {
    if urls.is_empty() {
        return None;
    }
    let match_url = |url: &NSURL| {
        rules
            .iter()
            .find(|rule| !rule.pattern.starts_with("*.") && rule.matches(url))
            .or_else(|| rules.iter().find(|rule| rule.matches(url)))
    };
    let first = urls.objectAtIndex(0);
    let browser_id = match_url(&first)?.browser_id.clone();
    urls.iter()
        .all(|url| match_url(&url).is_some_and(|rule| rule.browser_id == browser_id))
        .then_some(browser_id)
}

#[derive(Clone)]
pub struct Browser {
    pub id: String,
    pub name: String,
    pub path: String,
    pub url: Retained<NSURL>,
    pub icon: Retained<NSImage>,
}

pub fn exclusions() -> BTreeSet<String> {
    NSUserDefaults::standardUserDefaults()
        .stringArrayForKey(ns_string!("ExcludedBrowsers"))
        .map(|values| values.iter().map(|value| value.to_string()).collect())
        .unwrap_or_default()
}

pub fn set_excluded(id: &str, excluded: bool) {
    let mut ids = exclusions();
    if excluded {
        ids.insert(id.to_owned());
    } else {
        ids.remove(id);
    }
    let values: Vec<_> = ids.iter().map(|id| NSString::from_str(id)).collect();
    let values = NSArray::from_retained_slice(&values);
    unsafe {
        NSUserDefaults::standardUserDefaults()
            .setObject_forKey(Some(&values), ns_string!("ExcludedBrowsers"));
    }
}

pub struct AutoOpen {
    pub enabled: bool,
    pub seconds: f64,
    pub browser_id: String,
}

impl AutoOpen {
    pub fn load() -> Self {
        Self::from_defaults(&NSUserDefaults::standardUserDefaults())
    }

    fn from_defaults(defaults: &NSUserDefaults) -> Self {
        let seconds = defaults.doubleForKey(ns_string!("AutoOpenSeconds"));
        Self {
            enabled: defaults.boolForKey(ns_string!("AutoOpenEnabled")),
            seconds: if valid_timeout(seconds) {
                seconds
            } else {
                10.0
            },
            browser_id: defaults
                .stringForKey(ns_string!("AutoOpenBrowser"))
                .map(|value| value.to_string())
                .unwrap_or_default(),
        }
    }

    pub fn save(&self) {
        let defaults = NSUserDefaults::standardUserDefaults();
        defaults.setBool_forKey(self.enabled, ns_string!("AutoOpenEnabled"));
        defaults.setDouble_forKey(self.seconds, ns_string!("AutoOpenSeconds"));
        unsafe {
            defaults.setObject_forKey(
                Some(&NSString::from_str(&self.browser_id)),
                ns_string!("AutoOpenBrowser"),
            );
        }
    }
}

pub fn valid_timeout(seconds: f64) -> bool {
    seconds.is_finite() && seconds > 0.0
}

pub fn manual_order() -> bool {
    NSUserDefaults::standardUserDefaults().boolForKey(ns_string!("ManualBrowserOrder"))
}

pub fn set_manual_order(manual: bool) {
    NSUserDefaults::standardUserDefaults().setBool_forKey(manual, ns_string!("ManualBrowserOrder"));
}

pub fn set_browser_order(browsers: &[Browser]) {
    let paths: Vec<_> = browsers
        .iter()
        .map(|browser| NSString::from_str(&browser.path))
        .collect();
    unsafe {
        NSUserDefaults::standardUserDefaults().setObject_forKey(
            Some(&NSArray::from_retained_slice(&paths)),
            ns_string!("BrowserOrder"),
        );
    }
}

fn sort_browsers(browsers: &mut [Browser], order: &[String]) {
    browsers.sort_by(|a, b| {
        let rank = |path: &str| {
            order
                .iter()
                .position(|saved| saved == path)
                .unwrap_or(usize::MAX)
        };
        rank(&a.path)
            .cmp(&rank(&b.path))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.path.cmp(&b.path))
    });
}

pub fn move_browser(browsers: &mut [Browser], index: usize, offset: isize) -> bool {
    let Some(destination) = index.checked_add_signed(offset) else {
        return false;
    };
    if index >= browsers.len() || destination >= browsers.len() || index == destination {
        return false;
    }
    if index < destination {
        browsers[index..=destination].rotate_left(1);
    } else {
        browsers[destination..=index].rotate_right(1);
    }
    true
}

pub fn drop_destination(source: usize, insertion: usize, count: usize) -> Option<usize> {
    if source >= count || insertion > count {
        return None;
    }
    let destination = insertion - usize::from(source < insertion);
    (destination != source).then_some(destination)
}

fn eligible(id: &str, path: &str, own_path: &str) -> bool {
    !id.is_empty() && id != BUNDLE_ID && path != own_path
}

pub fn discover(schemes: &BTreeSet<String>) -> Vec<Browser> {
    discover_handlers(schemes, true)
}

pub fn discover_all() -> Vec<Browser> {
    discover_handlers(&BTreeSet::from(["http".into(), "https".into()]), false)
}

fn matching_paths(supported_paths: &[BTreeSet<String>], require_all: bool) -> BTreeSet<String> {
    supported_paths
        .iter()
        .flatten()
        .filter(|path| !require_all || supported_paths.iter().all(|paths| paths.contains(*path)))
        .cloned()
        .collect()
}

fn discover_handlers(schemes: &BTreeSet<String>, require_all: bool) -> Vec<Browser> {
    let workspace = NSWorkspace::sharedWorkspace();
    let own_path = NSBundle::mainBundle().bundlePath().to_string();
    let mut candidates = Vec::new();
    let mut supported_paths = Vec::new();
    for scheme in schemes {
        let probe =
            NSURL::URLWithString(&NSString::from_str(&format!("{scheme}://example.invalid")))
                .expect("fixed probe URL");
        let applications = workspace.URLsForApplicationsToOpenURL(&probe);
        supported_paths.push(
            applications
                .iter()
                .filter_map(|url| url.path().map(|path| path.to_string()))
                .collect::<BTreeSet<_>>(),
        );
        candidates.extend(applications.iter());
    }
    let mut supported_paths = matching_paths(&supported_paths, require_all);
    let mut browsers = Vec::new();
    for url in candidates {
        let Some(path) = url.path().map(|value| value.to_string()) else {
            continue;
        };
        if !supported_paths.remove(&path) || !Path::new(&path).is_dir() {
            continue;
        }
        let Some(bundle) = NSBundle::bundleWithURL(&url) else {
            continue;
        };
        let Some(id) = bundle.bundleIdentifier().map(|value| value.to_string()) else {
            continue;
        };
        if !eligible(&id, &path, &own_path) {
            continue;
        }
        let name = [
            ns_string!("CFBundleDisplayName"),
            ns_string!("CFBundleName"),
        ]
        .into_iter()
        .find_map(|key| {
            bundle
                .objectForInfoDictionaryKey(key)?
                .downcast::<NSString>()
                .ok()
        })
        .map(|value| value.to_string())
        .unwrap_or_else(|| {
            Path::new(&path)
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        });
        browsers.push(Browser {
            id,
            name,
            icon: workspace.iconForFile(&NSString::from_str(&path)),
            path,
            url,
        });
    }
    let order = if manual_order() {
        NSUserDefaults::standardUserDefaults()
            .stringArrayForKey(ns_string!("BrowserOrder"))
            .map(|values| values.iter().map(|value| value.to_string()).collect())
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    sort_browsers(&mut browsers, &order);
    browsers
}

fn is_default_for_scheme(scheme: &str) -> bool {
    let probe = NSURL::URLWithString(&NSString::from_str(&format!("{scheme}://example.invalid")))
        .expect("fixed probe URL");
    NSWorkspace::sharedWorkspace()
        .URLForApplicationToOpenURL(&probe)
        .and_then(|url| NSBundle::bundleWithURL(&url))
        .and_then(|bundle| bundle.bundleIdentifier())
        .is_some_and(|id| id.to_string() == BUNDLE_ID)
}

pub fn is_default_browser() -> bool {
    ["http", "https"].into_iter().all(is_default_for_scheme)
}

pub fn default_status() -> String {
    let statuses: Vec<_> = ["http", "https"]
        .into_iter()
        .map(|scheme| {
            let is_default = is_default_for_scheme(scheme);
            format!(
                "{}: {}",
                scheme.to_uppercase(),
                if is_default { "ChooBro" } else { "another app" }
            )
        })
        .collect();
    statuses.join(" · ")
}

pub fn available(browser: &Browser) -> bool {
    Path::new(&browser.path).is_dir()
        && NSBundle::bundleWithURL(&browser.url)
            .and_then(|bundle| bundle.bundleIdentifier())
            .is_some_and(|id| id.to_string() == browser.id)
}

pub fn is_app_bundle() -> bool {
    NSBundle::mainBundle()
        .bundleIdentifier()
        .is_some_and(|id| id.to_string() == BUNDLE_ID)
}

fn open_configuration() -> Retained<NSWorkspaceOpenConfiguration> {
    let configuration = NSWorkspaceOpenConfiguration::new();
    configuration.setActivates(true);
    configuration.setAllowsRunningApplicationSubstitution(false);
    configuration.setAddsToRecentItems(false);
    configuration
}

pub fn open(browser: &Browser, urls: &NSArray<NSURL>, finished: impl Fn(bool) + Send + 'static) {
    let completion = RcBlock::new(
        move |_app: *mut NSRunningApplication, error: *mut NSError| {
            finished(error.is_null());
        },
    );
    let configuration = open_configuration();
    NSWorkspace::sharedWorkspace().openURLs_withApplicationAtURL_configuration_completionHandler(
        urls,
        &browser.url,
        &configuration,
        Some(&completion),
    );
}

pub fn set_default_scheme(index: usize, finished: impl Fn(bool) + Send + 'static) {
    let scheme = [ns_string!("http"), ns_string!("https")][index];
    let completion = RcBlock::new(move |error: *mut NSError| finished(!error.is_null()));
    NSWorkspace::sharedWorkspace()
        .setDefaultApplicationAtURL_toOpenURLsWithScheme_completionHandler(
            &NSBundle::mainBundle().bundleURL(),
            scheme,
            Some(&completion),
        );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn browser(name: &str, path: &str) -> Browser {
        Browser {
            id: name.into(),
            name: name.into(),
            path: path.into(),
            url: NSURL::fileURLWithPath(&NSString::from_str(path)),
            icon: NSImage::new(),
        }
    }

    #[test]
    fn normalizes_domains_and_matches_exact_hosts() {
        assert_eq!(normalize_domain("Example.COM"), Some("example.com".into()));
        assert_eq!(
            normalize_domain("*.Example.COM"),
            Some("*.example.com".into())
        );
        assert_eq!(
            normalize_domain("  https://WWW.Example.com:8080/path?q=1#top "),
            Some("www.example.com".into())
        );
        assert_eq!(
            normalize_domain("sub.example.com/some/path"),
            Some("sub.example.com".into())
        );
        assert_eq!(normalize_domain("localhost:3000"), Some("localhost".into()));
        assert_eq!(normalize_domain("[::1]:8000"), Some("[::1]".into()));
        assert_eq!(normalize_domain("example.com."), Some("example.com".into()));
        for invalid in [
            "",
            "   ",
            "*",
            "*.",
            "**.example.com",
            "a.*.example.com",
            "-bad.example",
            "bad-.example",
            "a..b.example",
            "bad host.example",
        ] {
            assert_eq!(normalize_domain(invalid), None, "{invalid}");
        }
        let exact = Rule {
            kind: MatchKind::Domain,
            pattern: "www.example.com".into(),
            browser_id: "org.mozilla.firefox".into(),
        };
        let wildcard = Rule {
            kind: MatchKind::Domain,
            pattern: "*.example.com".into(),
            browser_id: "com.apple.Safari".into(),
        };
        let www = NSURL::URLWithString(ns_string!("https://WWW.EXAMPLE.COM:443/hello")).unwrap();
        let apex = NSURL::URLWithString(ns_string!("https://example.com/hello")).unwrap();
        let deep = NSURL::URLWithString(ns_string!("https://a.b.example.com/hello")).unwrap();
        let cousin = NSURL::URLWithString(ns_string!("https://badexample.com/hello")).unwrap();
        assert!(exact.matches(&www));
        assert!(!exact.matches(&apex));
        assert!(wildcard.matches(&www));
        assert!(wildcard.matches(&apex));
        assert!(wildcard.matches(&deep));
        assert!(!wildcard.matches(&cousin));
        let rules = vec![wildcard, exact];
        assert_eq!(
            matching_rule_browser(&rules, &NSArray::from_retained_slice(&[www])),
            Some("org.mozilla.firefox".into())
        );
        assert_eq!(
            matching_rule_browser(&rules, &NSArray::from_retained_slice(&[apex])),
            Some("com.apple.Safari".into())
        );
    }

    #[test]
    fn rules_round_trip_and_deduplicate_in_user_defaults() {
        use objc2::AnyThread;
        let defaults = NSUserDefaults::initWithSuiteName(
            NSUserDefaults::alloc(),
            Some(ns_string!("io.github.miguelcaravantes.choobro.rules-tests")),
        )
        .unwrap();
        defaults.removeObjectForKey(ns_string!("RoutingRules"));
        let mut list = Vec::new();
        upsert_rule(
            &mut list,
            Rule {
                kind: MatchKind::Domain,
                pattern: "example.com".into(),
                browser_id: "com.google.Chrome".into(),
            },
        );
        upsert_rule(
            &mut list,
            Rule {
                kind: MatchKind::Domain,
                pattern: "www.example.com".into(),
                browser_id: "org.mozilla.firefox".into(),
            },
        );
        upsert_rule(
            &mut list,
            Rule {
                kind: MatchKind::Domain,
                pattern: "example.com".into(),
                browser_id: "com.apple.Safari".into(),
            },
        );
        save_rules_to_defaults(&defaults, &list);
        let loaded = rules_from_defaults(&defaults);
        assert_eq!(
            loaded,
            vec![
                Rule {
                    kind: MatchKind::Domain,
                    pattern: "example.com".into(),
                    browser_id: "com.apple.Safari".into(),
                },
                Rule {
                    kind: MatchKind::Domain,
                    pattern: "www.example.com".into(),
                    browser_id: "org.mozilla.firefox".into(),
                },
            ]
        );
        let single = NSArray::from_retained_slice(&[NSURL::URLWithString(ns_string!(
            "https://example.com/search?q=rust"
        ))
        .unwrap()]);
        assert_eq!(
            matching_rule_browser(&loaded, &single),
            Some("com.apple.Safari".into())
        );
        let mixed = NSArray::from_retained_slice(&[
            NSURL::URLWithString(ns_string!("https://example.com/a")).unwrap(),
            NSURL::URLWithString(ns_string!("https://www.example.com/b")).unwrap(),
        ]);
        assert_eq!(matching_rule_browser(&loaded, &mixed), None);
        assert_eq!(shared_domain(&single), Some("example.com".into()));
        assert_eq!(shared_domain(&mixed), None);
        defaults.removeObjectForKey(ns_string!("RoutingRules"));
    }

    #[test]
    fn auto_open_is_opt_in_and_rejects_invalid_intervals() {
        use objc2::AnyThread;
        let defaults = NSUserDefaults::initWithSuiteName(
            NSUserDefaults::alloc(),
            Some(ns_string!(
                "io.github.miguelcaravantes.choobro.timeout-tests"
            )),
        )
        .unwrap();
        let values =
            objc2_foundation::NSDictionary::<NSString, objc2::runtime::AnyObject>::from_slices(
                &[ns_string!("AutoOpenEnabled"), ns_string!("AutoOpenSeconds")],
                &[
                    &*objc2_foundation::NSNumber::new_bool(false),
                    &*objc2_foundation::NSNumber::new_f64(0.0),
                ],
            );
        unsafe {
            defaults.setVolatileDomain_forName(&values, objc2_foundation::NSArgumentDomain);
        }
        let preference = AutoOpen::from_defaults(&defaults);
        assert!(!preference.enabled);
        assert_eq!(preference.seconds, 10.0);
        assert!(valid_timeout(0.25));
        assert!(valid_timeout(86400.0));
        for seconds in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(!valid_timeout(seconds));
        }
    }

    #[test]
    fn dispatch_uses_the_selected_installation_without_recording_recent_items() {
        let configuration = open_configuration();
        assert!(configuration.activates());
        assert!(!configuration.allowsRunningApplicationSubstitution());
        assert!(!configuration.addsToRecentItems());
    }

    #[test]
    fn settings_include_single_scheme_handlers_but_requests_require_every_scheme() {
        let supported = [
            BTreeSet::from(["http-only".into(), "both".into()]),
            BTreeSet::from(["https-only".into(), "both".into()]),
        ];
        assert_eq!(
            matching_paths(&supported, true),
            BTreeSet::from(["both".into()])
        );
        assert_eq!(
            matching_paths(&supported, false),
            BTreeSet::from(["http-only".into(), "https-only".into(), "both".into()])
        );
        assert_eq!(matching_paths(&supported[..1], true), supported[0]);
        assert!(matching_paths(&[], true).is_empty());
        assert!(matching_paths(&[supported[0].clone(), BTreeSet::new()], true).is_empty());
    }

    #[test]
    fn alphabetical_order_is_case_insensitive_and_breaks_ties_by_path() {
        let mut browsers = vec![
            browser("Safari", "/Safari.app"),
            browser("chrome", "/Copy.app"),
            browser("Chrome", "/Chrome.app"),
        ];
        sort_browsers(&mut browsers, &[]);
        assert_eq!(
            browsers
                .iter()
                .map(|browser| browser.path.as_str())
                .collect::<Vec<_>>(),
            ["/Chrome.app", "/Copy.app", "/Safari.app"]
        );
    }

    #[test]
    fn manual_order_ignores_missing_browsers_and_appends_new_ones_alphabetically() {
        let mut browsers = vec![
            browser("Chrome", "/Chrome.app"),
            browser("Firefox", "/Firefox.app"),
            browser("Safari", "/Safari.app"),
            browser("Brave", "/Brave.app"),
        ];
        let order = ["/Missing.app", "/Safari.app", "/Chrome.app"].map(String::from);
        sort_browsers(&mut browsers, &order);
        assert_eq!(
            browsers
                .iter()
                .map(|browser| browser.name.as_str())
                .collect::<Vec<_>>(),
            ["Safari", "Chrome", "Brave", "Firefox"]
        );
        sort_browsers(&mut browsers, &[]);
        assert_eq!(browsers[0].name, "Brave");
        sort_browsers(&mut browsers, &order);
        assert_eq!(browsers[0].name, "Safari");
    }

    #[test]
    fn moving_browsers_swaps_neighbors_and_rejects_out_of_bounds_moves() {
        let mut browsers = vec![
            browser("Chrome", "/Chrome.app"),
            browser("Safari", "/Safari.app"),
        ];
        assert!(!move_browser(&mut browsers, 0, -1));
        assert!(!move_browser(&mut browsers, 1, 1));
        assert!(!move_browser(&mut browsers, usize::MAX, 1));
        assert!(!move_browser(&mut browsers, 2, -1));
        assert!(!move_browser(&mut browsers, 0, 0));
        assert!(move_browser(&mut browsers, 0, 1));
        assert_eq!(browsers[0].name, "Safari");
        assert!(move_browser(&mut browsers, 1, -1));
        assert_eq!(browsers[0].name, "Chrome");
        assert!(!move_browser(&mut [], 0, 1));
    }

    #[test]
    fn dragging_preserves_the_relative_order_of_other_browsers() {
        let mut browsers = vec![
            browser("Chrome", "/Chrome.app"),
            browser("Firefox", "/Firefox.app"),
            browser("Safari", "/Safari.app"),
            browser("Brave", "/Brave.app"),
        ];
        assert!(move_browser(&mut browsers, 0, 3));
        assert_eq!(
            browsers
                .iter()
                .map(|browser| browser.name.as_str())
                .collect::<Vec<_>>(),
            ["Firefox", "Safari", "Brave", "Chrome"]
        );
        assert!(move_browser(&mut browsers, 3, -3));
        assert_eq!(
            browsers
                .iter()
                .map(|browser| browser.name.as_str())
                .collect::<Vec<_>>(),
            ["Chrome", "Firefox", "Safari", "Brave"]
        );
    }

    #[test]
    fn drop_positions_account_for_removing_the_dragged_row() {
        assert_eq!(drop_destination(0, 4, 4), Some(3));
        assert_eq!(drop_destination(0, 2, 4), Some(1));
        assert_eq!(drop_destination(3, 0, 4), Some(0));
        assert_eq!(drop_destination(3, 2, 4), Some(2));
        assert_eq!(drop_destination(1, 1, 4), None);
        assert_eq!(drop_destination(1, 2, 4), None);
        assert_eq!(drop_destination(0, 0, 0), None);
        assert_eq!(drop_destination(0, 1, 1), None);
        assert_eq!(drop_destination(4, 0, 4), None);
        assert_eq!(drop_destination(0, 5, 4), None);
        assert_eq!(drop_destination(usize::MAX, usize::MAX, 4), None);
    }

    #[test]
    fn excludes_self_by_identifier_and_location() {
        assert!(!eligible(
            BUNDLE_ID,
            "/Applications/Copy.app",
            "/Applications/ChooBro.app"
        ));
        assert!(!eligible(
            "different.id",
            "/Applications/ChooBro.app",
            "/Applications/ChooBro.app"
        ));
        assert!(!eligible(
            "",
            "/Applications/Unknown.app",
            "/Applications/ChooBro.app"
        ));
        assert!(eligible(
            "com.apple.Safari",
            "/Applications/Safari.app",
            "/Applications/ChooBro.app"
        ));
    }
}
