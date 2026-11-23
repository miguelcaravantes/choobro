use objc2_foundation::{
    NSData, NSDictionary, NSHomeDirectory, NSJSONReadingOptions, NSJSONSerialization, NSNumber,
    NSString, ns_string,
};
use std::{collections::HashMap, fs::File, io::Read, path::PathBuf};

pub const BETA_BUNDLE_ID: &str = "com.google.Chrome.beta";
pub const CHANNELS: [(&str, &str); 2] = [
    ("com.google.Chrome", "Chrome"),
    (BETA_BUNDLE_ID, "Chrome Beta"),
];
const MAX_STATE_BYTES: u64 = 8 * 1024 * 1024;

pub struct Profile {
    pub directory: String,
    pub name: String,
}

pub fn data_directory(folder: &str) -> PathBuf {
    PathBuf::from(NSHomeDirectory().to_string())
        .join("Library/Application Support/Google")
        .join(folder)
}

fn valid_directory(directory: &str) -> bool {
    directory == "Default"
        || directory
            .strip_prefix("Profile ")
            .is_some_and(|id| !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit()))
}

pub fn available(bundle_id: &str, directory: &str) -> bool {
    valid_directory(directory)
        && CHANNELS.iter().any(|(id, folder)| {
            *id == bundle_id
                && data_directory(folder)
                    .join(directory)
                    .join("Preferences")
                    .is_file()
        })
}

#[derive(Default)]
pub struct ProfileDiscovery {
    pub profiles: Vec<Profile>,
    pub permission_denied: bool,
}

pub fn profiles(folder: &str) -> std::io::Result<ProfileDiscovery> {
    let data = data_directory(folder);
    let file = File::open(data.join("Local State"))?;
    let mut state = Vec::new();
    file.take(MAX_STATE_BYTES + 1).read_to_end(&mut state)?;
    if state.len() as u64 > MAX_STATE_BYTES {
        return Ok(ProfileDiscovery::default());
    }
    Ok(discover_profiles(&state, |directory| {
        data.join(directory)
            .join("Preferences")
            .metadata()
            .map(|metadata| metadata.is_file())
    }))
}

fn discover_profiles(
    state: &[u8],
    profile_exists: impl Fn(&str) -> std::io::Result<bool>,
) -> ProfileDiscovery {
    let mut discovery = ProfileDiscovery::default();
    for profile in parse_profiles(state).unwrap_or_default() {
        match profile_exists(&profile.directory) {
            Ok(true) => discovery.profiles.push(profile),
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                discovery.permission_denied = true;
            }
            _ => {}
        }
    }
    let mut names = HashMap::new();
    for profile in &discovery.profiles {
        *names.entry(profile.name.to_lowercase()).or_insert(0) += 1;
    }
    for profile in &mut discovery.profiles {
        if names[&profile.name.to_lowercase()] > 1 {
            let base = format!("{} ({})", profile.name, profile.directory);
            let mut label = base.clone();
            let mut suffix = 2;
            while names.contains_key(&label.to_lowercase()) {
                label = format!("{base} ({suffix})");
                suffix += 1;
            }
            names.insert(label.to_lowercase(), 1);
            profile.name = label;
        }
    }
    discovery
}

fn parse_profiles(state: &[u8]) -> Option<Vec<Profile>> {
    let cache = NSJSONSerialization::JSONObjectWithData_options_error(
        &NSData::with_bytes(state),
        NSJSONReadingOptions::empty(),
    )
    .ok()?
    .downcast::<NSDictionary>()
    .ok()?
    .objectForKey(ns_string!("profile"))?
    .downcast::<NSDictionary>()
    .ok()?
    .objectForKey(ns_string!("info_cache"))?
    .downcast::<NSDictionary>()
    .ok()?;

    let mut profiles: Vec<_> = cache
        .keys()
        .filter_map(|key| {
            let directory = key.downcast::<NSString>().ok()?.to_string();
            if !valid_directory(&directory) {
                return None;
            }
            let entry = cache
                .objectForKey(&*NSString::from_str(&directory))?
                .downcast::<NSDictionary>()
                .ok()?;
            if [
                ns_string!("is_ephemeral"),
                ns_string!("force_signin_profile_locked"),
            ]
            .into_iter()
            .any(|key| {
                entry
                    .objectForKey(key)
                    .and_then(|value| value.downcast::<NSNumber>().ok())
                    .is_some_and(|value| value.boolValue())
            }) {
                return None;
            }
            let name = entry
                .objectForKey(ns_string!("name"))
                .and_then(|value| value.downcast::<NSString>().ok())
                .map(|name| name.to_string())
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| directory.clone());
            Some(Profile { directory, name })
        })
        .collect();
    profiles.sort_by_cached_key(|profile| (profile.name.to_lowercase(), profile.directory.clone()));
    Some(profiles)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_only_regular_chrome_profile_entries() {
        let state = br#"{"profile":{"info_cache":{
            "Default":{"name":"Work"},
            "Profile 1":{"name":"Personal"},
            "Profile 2":{"name":"Work"},
            "Profile 3":{"name":"  "},
            "Profile 4":{"name":"Temporary","is_ephemeral":true},
            "Profile 5":{"name":"Locked","force_signin_profile_locked":true},
            "Profile 6":null,
            "Guest Profile":{"name":"Guest"},
            "System Profile":{"name":"System"},
            "../Profile 7":{"name":"Outside"}
        }}}"#;
        let profiles = parse_profiles(state).unwrap();
        assert_eq!(
            profiles
                .iter()
                .map(|p| (p.directory.as_str(), p.name.as_str()))
                .collect::<Vec<_>>(),
            [
                ("Profile 1", "Personal"),
                ("Profile 3", "Profile 3"),
                ("Default", "Work"),
                ("Profile 2", "Work")
            ]
        );
    }

    #[test]
    fn generated_profile_names_do_not_collide_with_existing_names() {
        let state = br#"{"profile":{"info_cache":{
            "Default":{"name":"Work"},
            "Profile 1":{"name":"Work"},
            "Profile 2":{"name":"Work (Default)"}
        }}}"#;
        let discovery = discover_profiles(state, |_| Ok(true));
        assert_eq!(
            discovery
                .profiles
                .iter()
                .map(|profile| profile.name.as_str())
                .collect::<Vec<_>>(),
            ["Work (Default) (2)", "Work (Profile 1)", "Work (Default)"]
        );
    }

    #[test]
    fn readable_state_reports_denied_profile_directories_without_losing_other_profiles() {
        let state = br#"{"profile":{"info_cache":{
            "Default":{"name":"Work"},
            "Profile 1":{"name":"Personal"}
        }}}"#;
        let discovery = discover_profiles(state, |directory| {
            if directory == "Default" {
                Err(std::io::ErrorKind::PermissionDenied.into())
            } else {
                Ok(true)
            }
        });
        assert!(discovery.permission_denied);
        assert_eq!(discovery.profiles.len(), 1);
        assert_eq!(discovery.profiles[0].directory, "Profile 1");
    }

    #[test]
    fn rejects_missing_or_invalid_profile_data() {
        for state in [
            b"not JSON".as_slice(),
            b"[]",
            b"{}",
            br#"{"profile":{"info_cache":[]}}"#,
        ] {
            assert!(parse_profiles(state).is_none());
        }
    }
}
