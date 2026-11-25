use std::{collections::BTreeSet, path::Path};

use block2::RcBlock;
use objc2::{rc::Retained, runtime::AnyObject};
use objc2_app_kit::{NSImage, NSRunningApplication, NSWorkspace, NSWorkspaceOpenConfiguration};
use objc2_foundation::{
    NSArray, NSBundle, NSDictionary, NSError, NSString, NSURL, NSUserDefaults, ns_string,
};

#[path = "browsers/chrome.rs"]
mod chrome;

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
    pub target_key: String,
}

impl Rule {
    fn matches_domain(&self, domain: &str) -> bool {
        match self.kind {
            MatchKind::Domain => {
                if let Some(suffix) = self.pattern.strip_prefix("*.") {
                    domain == suffix
                        || domain
                            .strip_suffix(suffix)
                            .is_some_and(|prefix| prefix.ends_with('.'))
                } else {
                    domain == self.pattern
                }
            }
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
    let mut urls = urls.iter();
    let first = url_domain(&*urls.next()?)?;
    urls.all(|url| url_domain(&url).as_deref() == Some(first.as_str()))
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
        let Some(target_key) = dict
            .objectForKey(ns_string!("browser"))
            .and_then(|v| v.downcast::<NSString>().ok())
            .map(|v| v.to_string())
            .filter(|v| !v.is_empty())
        else {
            continue;
        };
        upsert_rule(
            &mut loaded,
            Rule {
                kind,
                pattern,
                target_key,
            },
        );
    }
    loaded
}

pub fn save_rules(rules: &[Rule]) {
    let values = rule_values(rules);
    unsafe {
        NSUserDefaults::standardUserDefaults()
            .setObject_forKey(Some(&values), ns_string!("RoutingRules"));
    }
}

pub fn rule_values(rules: &[Rule]) -> Retained<NSArray<NSDictionary<NSString, AnyObject>>> {
    let dicts: Vec<_> = rules
        .iter()
        .map(|rule| {
            let kind = NSString::from_str(rule.kind.as_str());
            let pattern = NSString::from_str(&rule.pattern);
            let browser = NSString::from_str(&rule.target_key);
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
    NSArray::from_retained_slice(&dicts)
}

pub fn upsert_rule(rules: &mut Vec<Rule>, rule: Rule) {
    if let Some(existing) = rules
        .iter_mut()
        .find(|existing| existing.kind == rule.kind && existing.pattern == rule.pattern)
    {
        existing.target_key = rule.target_key;
    } else {
        rules.push(rule);
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum RuleRoute {
    Target(String),
    MixedTargets,
}

pub fn rule_route(rules: &[Rule], urls: &NSArray<NSURL>) -> Option<RuleRoute> {
    if rules.is_empty() {
        return None;
    }
    let mut targets = urls.iter().map(|url| {
        let domain = url_domain(&url)?;
        rules
            .iter()
            .find(|rule| !rule.pattern.starts_with("*.") && rule.matches_domain(&domain))
            .or_else(|| rules.iter().find(|rule| rule.matches_domain(&domain)))
            .map(|rule| rule.target_key.as_str())
    });
    let first = targets.next()?;
    if targets.any(|target| target != first) {
        Some(RuleRoute::MixedTargets)
    } else {
        first.map(|key| RuleRoute::Target(key.to_owned()))
    }
}

#[derive(Clone)]
pub struct Browser {
    pub id: String,
    pub name: String,
    pub path: String,
    pub url: Retained<NSURL>,
    pub icon: Retained<NSImage>,
    pub profile: Option<String>,
}

const PROFILE_KEY_PREFIX: &str = "profile:";

pub fn rule_target_label(key: &str) -> String {
    if let Some((bundle, directory)) = key
        .strip_prefix(PROFILE_KEY_PREFIX)
        .and_then(|value| value.split_once('/'))
        && let Some((_, channel)) = chrome::CHANNELS.iter().find(|(id, _)| *id == bundle)
    {
        return format!("Google {channel} — {directory}");
    }
    key.to_owned()
}

impl Browser {
    pub fn order_key(&self) -> String {
        match &self.profile {
            Some(profile) => format!("{PROFILE_KEY_PREFIX}{}/{profile}", self.path),
            None => self.path.clone(),
        }
    }

    pub fn target_key(&self) -> String {
        match &self.profile {
            Some(profile) => format!("{PROFILE_KEY_PREFIX}{}/{profile}", self.id),
            None => self.id.clone(),
        }
    }

    pub fn parent_is_visible(&self, excluded: &BTreeSet<String>) -> bool {
        self.profile.is_none() || !excluded.contains(&self.id)
    }

    pub fn is_visible(&self, excluded: &BTreeSet<String>) -> bool {
        self.parent_is_visible(excluded) && !excluded.contains(&self.target_key())
    }
}

pub fn profiles_enabled() -> bool {
    NSUserDefaults::standardUserDefaults().boolForKey(ns_string!("BrowserProfilesEnabled"))
}

pub fn set_profiles_enabled(enabled: bool) {
    NSUserDefaults::standardUserDefaults()
        .setBool_forKey(enabled, ns_string!("BrowserProfilesEnabled"));
}

#[derive(Debug, PartialEq, Eq)]
pub enum ProfileNotice {
    PermissionDenied(Vec<&'static str>),
    NoProfiles,
}

pub fn add_profile_choices(browsers: &mut Vec<Browser>) -> Option<ProfileNotice> {
    if !profiles_enabled() {
        return None;
    }
    let notice = add_profile_choices_from(browsers, chrome::profiles);
    if manual_order() {
        sort_browsers(browsers, &saved_browser_order());
    }
    notice
}

pub fn chrome_beta_installed() -> bool {
    NSWorkspace::sharedWorkspace()
        .URLForApplicationWithBundleIdentifier(&NSString::from_str(chrome::BETA_BUNDLE_ID))
        .and_then(|url| url.path())
        .is_some_and(|path| Path::new(&path.to_string()).is_dir())
}

pub fn profile_folder_url(folder: &str) -> Option<Retained<NSURL>> {
    chrome::CHANNELS
        .iter()
        .any(|(_, channel)| *channel == folder)
        .then(|| {
            let path = chrome::data_directory(folder);
            NSURL::fileURLWithPath_isDirectory(&NSString::from_str(&path.to_string_lossy()), true)
        })
}

fn add_profile_choices_from(
    browsers: &mut Vec<Browser>,
    load: impl Fn(&str) -> std::io::Result<chrome::ProfileDiscovery>,
) -> Option<ProfileNotice> {
    let mut checked = false;
    let mut denied = Vec::new();
    let initial_count = browsers.len();
    for (id, folder) in chrome::CHANNELS {
        if !browsers.iter().any(|browser| browser.id == id) {
            continue;
        }
        checked = true;
        let discovery = match load(folder) {
            Ok(discovery) => discovery,
            Err(error) => {
                if error.kind() == std::io::ErrorKind::PermissionDenied {
                    denied.push(folder);
                }
                continue;
            }
        };
        if discovery.permission_denied {
            denied.push(folder);
        }
        for index in (0..browsers.len()).rev() {
            let browser = &browsers[index];
            if browser.id == id {
                let choices: Vec<_> = discovery
                    .profiles
                    .iter()
                    .map(|profile| Browser {
                        name: format!("{} — {}", browser.name, profile.name),
                        profile: Some(profile.directory.clone()),
                        ..browser.clone()
                    })
                    .collect();
                browsers.splice(index + 1..index + 1, choices);
            }
        }
    }
    if !denied.is_empty() {
        Some(ProfileNotice::PermissionDenied(denied))
    } else if checked && browsers.len() == initial_count {
        Some(ProfileNotice::NoProfiles)
    } else {
        None
    }
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

fn saved_browser_order() -> Vec<String> {
    NSUserDefaults::standardUserDefaults()
        .stringArrayForKey(ns_string!("BrowserOrder"))
        .map(|values| values.iter().map(|value| value.to_string()).collect())
        .unwrap_or_default()
}

pub fn set_browser_order(browsers: &[Browser]) {
    let keys = browsers.iter().map(Browser::order_key).collect::<Vec<_>>();
    let order = merge_browser_order(&keys, &saved_browser_order());
    let values: Vec<_> = order.iter().map(|key| NSString::from_str(key)).collect();
    unsafe {
        NSUserDefaults::standardUserDefaults().setObject_forKey(
            Some(&NSArray::from_retained_slice(&values)),
            ns_string!("BrowserOrder"),
        );
    }
}

fn merge_browser_order(keys: &[String], saved: &[String]) -> Vec<String> {
    let mut pending = keys.iter().cloned();
    let mut order = Vec::new();
    for key in saved {
        if keys.contains(key) {
            order.extend(pending.next());
        } else if key.starts_with(PROFILE_KEY_PREFIX) {
            order.push(key.clone());
        }
    }
    order.extend(pending);
    order
}

fn sort_browsers(browsers: &mut [Browser], order: &[String]) {
    browsers.sort_by_cached_key(|browser| {
        let key = browser.order_key();
        let rank = order
            .iter()
            .position(|saved| saved == &key)
            .unwrap_or(usize::MAX);
        (rank, browser.name.to_lowercase(), key)
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
            profile: None,
            icon: workspace.iconForFile(&NSString::from_str(&path)),
            path,
            url,
        });
    }
    let order = if manual_order() {
        saved_browser_order()
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
        && browser
            .profile
            .as_deref()
            .is_none_or(|directory| profiles_enabled() && chrome::available(&browser.id, directory))
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

pub fn open(
    browser: &Browser,
    urls: &NSArray<NSURL>,
    arguments: Option<&NSArray<NSString>>,
    finished: impl Fn(bool) + Send + 'static,
) {
    let completion = RcBlock::new(
        move |_app: *mut NSRunningApplication, error: *mut NSError| {
            finished(error.is_null());
        },
    );
    let configuration = open_configuration();
    let workspace = NSWorkspace::sharedWorkspace();
    if let Some(arguments) = arguments {
        configuration.setArguments(arguments);
        configuration.setCreatesNewApplicationInstance(true);
        workspace.openApplicationAtURL_configuration_completionHandler(
            &browser.url,
            &configuration,
            Some(&completion),
        );
    } else {
        workspace.openURLs_withApplicationAtURL_configuration_completionHandler(
            urls,
            &browser.url,
            &configuration,
            Some(&completion),
        );
    }
}

pub fn chrome_arguments(
    directory: &str,
    urls: &NSArray<NSURL>,
) -> Option<Retained<NSArray<NSString>>> {
    const CHROME_SINGLETON_SAFE_ARGUMENT_BYTES: usize = 24 * 1024;
    let mut arguments = vec![
        NSString::from_str(&format!("--profile-directory={directory}")),
        ns_string!("--").into(),
    ];
    let mut bytes = arguments.iter().map(|arg| arg.len() + 1).sum::<usize>();
    for url in urls {
        let value = url.absoluteString()?;
        bytes += value.len() + 1;
        if bytes > CHROME_SINGLETON_SAFE_ARGUMENT_BYTES {
            return None;
        }
        arguments.push(value);
    }
    Some(NSArray::from_retained_slice(&arguments))
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
            profile: None,
        }
    }

    #[test]
    fn beta_profiles_appear_under_beta_even_when_stable_access_is_denied() {
        let stable = Browser {
            id: "com.google.Chrome".into(),
            ..browser("Chrome", "/Chrome.app")
        };
        let beta = Browser {
            id: "com.google.Chrome.beta".into(),
            ..browser("Chrome Beta", "/Chrome Work.app")
        };
        let mut choices = vec![stable, beta];
        let notice = add_profile_choices_from(&mut choices, |folder| match folder {
            "Chrome" => Err(std::io::ErrorKind::PermissionDenied.into()),
            "Chrome Beta" => Ok(chrome::ProfileDiscovery {
                profiles: vec![chrome::Profile {
                    directory: "Profile 2".into(),
                    name: "Work".into(),
                }],
                permission_denied: false,
            }),
            _ => panic!("Unexpected profile directory"),
        });
        assert_eq!(
            notice,
            Some(ProfileNotice::PermissionDenied(vec!["Chrome"]))
        );
        assert_eq!(choices.len(), 3);
        assert_eq!(choices[2].name, "Chrome Beta — Work");
        assert_eq!(choices[2].path, choices[1].path);
        assert_eq!(choices[2].profile.as_deref(), Some("Profile 2"));
    }

    #[test]
    fn chrome_profile_visibility_is_independent_but_respects_the_parent() {
        let chrome = Browser {
            id: "com.google.Chrome".into(),
            ..browser("Chrome", "/Chrome.app")
        };
        let work = Browser {
            profile: Some("Default".into()),
            ..chrome.clone()
        };
        let personal = Browser {
            profile: Some("Profile 1".into()),
            ..chrome.clone()
        };
        let mut excluded = BTreeSet::from([work.target_key()]);
        assert!(!work.is_visible(&excluded));
        assert!(personal.is_visible(&excluded));
        assert!(chrome.is_visible(&excluded));
        excluded.insert(chrome.target_key());
        assert!(!personal.is_visible(&excluded));
        excluded.remove(&chrome.target_key());
        assert!(personal.is_visible(&excluded));
        assert!(!work.is_visible(&excluded));
    }

    #[test]
    fn profile_order_survives_when_only_browser_rows_are_being_reordered() {
        let chrome = browser("Chrome", "/Chrome.app");
        let work = Browser {
            name: "Chrome — Work".into(),
            profile: Some("Default".into()),
            ..chrome.clone()
        };
        let safari = browser("Safari", "/Safari.app");
        let saved = [work.order_key(), chrome.order_key(), safari.order_key()];
        let reordered = merge_browser_order(&[safari.order_key(), chrome.order_key()], &saved);
        let mut rows = [chrome, work, safari];
        sort_browsers(&mut rows, &reordered);
        assert_eq!(
            rows.map(|row| row.name),
            ["Chrome — Work", "Safari", "Chrome"]
        );
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
            target_key: "org.mozilla.firefox".into(),
        };
        let wildcard = Rule {
            kind: MatchKind::Domain,
            pattern: "*.example.com".into(),
            target_key: "com.apple.Safari".into(),
        };
        let www = NSURL::URLWithString(ns_string!("https://WWW.EXAMPLE.COM:443/hello")).unwrap();
        let apex = NSURL::URLWithString(ns_string!("https://example.com/hello")).unwrap();
        let rules = [wildcard, exact];
        for (url, expected) in [
            (www, Some("org.mozilla.firefox")),
            (apex, Some("com.apple.Safari")),
            (
                NSURL::URLWithString(ns_string!("https://a.b.example.com/hello")).unwrap(),
                Some("com.apple.Safari"),
            ),
            (
                NSURL::URLWithString(ns_string!("https://badexample.com/hello")).unwrap(),
                None,
            ),
        ] {
            let urls = NSArray::from_retained_slice(&[url]);
            assert_eq!(
                rule_route(&rules, &urls),
                expected.map(|key| RuleRoute::Target(key.into()))
            );
        }
    }

    #[test]
    fn rules_from_defaults_upserts_normalized_duplicates_in_original_order() {
        use objc2::AnyThread;
        let defaults = NSUserDefaults::initWithSuiteName(
            NSUserDefaults::alloc(),
            Some(ns_string!("io.github.miguelcaravantes.choobro.rules-tests")),
        )
        .unwrap();
        let list = [
            Rule {
                kind: MatchKind::Domain,
                pattern: "Example.COM.".into(),
                target_key: "com.google.Chrome".into(),
            },
            Rule {
                kind: MatchKind::Domain,
                pattern: "www.example.com".into(),
                target_key: "profile:com.google.Chrome/Profile 1".into(),
            },
            Rule {
                kind: MatchKind::Domain,
                pattern: "example.com".into(),
                target_key: "profile:com.google.Chrome/Default".into(),
            },
        ];
        let values = NSDictionary::<NSString, AnyObject>::from_slices(
            &[ns_string!("RoutingRules")],
            &[&*rule_values(&list)],
        );
        unsafe {
            defaults.setVolatileDomain_forName(&values, objc2_foundation::NSArgumentDomain);
        }
        let loaded = rules_from_defaults(&defaults);
        assert_eq!(loaded, vec![list[2].clone(), list[1].clone()]);
        let single = NSArray::from_retained_slice(&[NSURL::URLWithString(ns_string!(
            "https://example.com/search?q=rust"
        ))
        .unwrap()]);
        assert_eq!(
            rule_route(&loaded, &single),
            Some(RuleRoute::Target(
                "profile:com.google.Chrome/Default".into()
            ))
        );
        let mixed = NSArray::from_retained_slice(&[
            NSURL::URLWithString(ns_string!("https://example.com/a")).unwrap(),
            NSURL::URLWithString(ns_string!("https://www.example.com/b")).unwrap(),
        ]);
        assert_eq!(rule_route(&loaded, &mixed), Some(RuleRoute::MixedTargets));
        assert_eq!(shared_domain(&single), Some("example.com".into()));
        assert_eq!(shared_domain(&mixed), None);
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
        assert!(!configuration.createsNewApplicationInstance());
    }

    #[test]
    fn chrome_arguments_keep_each_link_intact_and_separate_from_switches() {
        let links = [
            "https://example.com/a%2Fb?x=a%26b#frag",
            "http://example.org/?x=--incognito",
        ];
        let urls = links.map(|link| NSURL::URLWithString(&NSString::from_str(link)).unwrap());
        let arguments =
            chrome_arguments("Profile 2", &NSArray::from_retained_slice(&urls)).unwrap();
        assert_eq!(
            arguments
                .iter()
                .map(|arg| arg.to_string())
                .collect::<Vec<_>>(),
            ["--profile-directory=Profile 2", "--", links[0], links[1]]
        );
    }

    #[test]
    fn chrome_rejects_batches_that_could_overflow_the_running_browser_handoff() {
        let link = NSString::from_str(&format!("https://example.com/{}", "x".repeat(13 * 1024)));
        let link = NSURL::URLWithString(&link).unwrap();
        assert!(chrome_arguments("Default", &NSArray::from_slice(&[&*link])).is_some());
        assert!(chrome_arguments("Default", &NSArray::from_slice(&[&*link, &*link])).is_none());
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
