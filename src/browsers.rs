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
    browsers.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then(a.path.cmp(&b.path))
    });
    browsers
}

pub fn default_status() -> String {
    let workspace = NSWorkspace::sharedWorkspace();
    let statuses: Vec<_> = ["http", "https"]
        .into_iter()
        .map(|scheme| {
            let probe =
                NSURL::URLWithString(&NSString::from_str(&format!("{scheme}://example.invalid")))
                    .expect("fixed probe URL");
            let is_default = workspace
                .URLForApplicationToOpenURL(&probe)
                .and_then(|url| NSBundle::bundleWithURL(&url))
                .and_then(|bundle| bundle.bundleIdentifier())
                .is_some_and(|id| id.to_string() == BUNDLE_ID);
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
