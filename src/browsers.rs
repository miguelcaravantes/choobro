use std::{collections::BTreeSet, path::Path};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2_app_kit::{NSImage, NSRunningApplication, NSWorkspace, NSWorkspaceOpenConfiguration};
use objc2_foundation::{NSArray, NSBundle, NSError, NSString, NSURL, NSUserDefaults, ns_string};

pub const BUNDLE_ID: &str = "io.github.miguelcaravantes.choobro";

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
    let mut seen = BTreeSet::new();
    let mut browsers = Vec::new();
    for url in candidates {
        let Some(path) = url.path().map(|value| value.to_string()) else {
            continue;
        };
        if !seen.insert(path.clone())
            || !supported_paths.iter().all(|paths| paths.contains(&path))
            || !Path::new(&path).is_dir()
        {
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

pub fn open(browser: &Browser, urls: &NSArray<NSURL>, finished: impl Fn(bool) + Send + 'static) {
    let completion = RcBlock::new(
        move |_app: *mut NSRunningApplication, error: *mut NSError| {
            finished(error.is_null());
        },
    );
    let configuration = NSWorkspaceOpenConfiguration::new();
    configuration.setActivates(true);
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
