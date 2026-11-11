use std::collections::VecDeque;

use objc2::{Message, rc::Retained};
use objc2_foundation::{NSArray, NSURL, NSURLComponents, NSUTF8StringEncoding};

const MAX_PENDING_URLS: usize = 256;
const MAX_PENDING_URL_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rejection {
    Unsupported,
    Full,
}

impl Rejection {
    pub fn message(self) -> (&'static str, &'static str) {
        match self {
            Self::Unsupported => (
                "Unsupported link",
                "ChooBro accepts only absolute HTTP and HTTPS links with a host. This request was not opened.",
            ),
            Self::Full => (
                "Pending link limit reached",
                "This request was not added. ChooBro holds up to 256 pending links and 8 MiB of URL text. Open or cancel pending links, or send a smaller batch, then try again.",
            ),
        }
    }
}

pub fn is_web_url(url: &NSURL) -> bool {
    url.baseURL().is_none()
        && url.scheme().is_some_and(|scheme| {
            matches!(
                scheme.to_string().to_ascii_lowercase().as_str(),
                "http" | "https"
            )
        })
        && url.host().is_some_and(|host| !host.is_empty())
}

pub fn unwrap_url(url: &NSURL) -> Retained<NSURL> {
    let mut current = url.retain();
    for _ in 0..4 {
        let Some(inner) = wrapped_target(&current) else {
            break;
        };
        current = inner;
    }
    current
}

fn wrapped_target(url: &NSURL) -> Option<Retained<NSURL>> {
    if !is_safelink_wrapper(url) {
        return None;
    }
    let components = NSURLComponents::componentsWithURL_resolvingAgainstBaseURL(url, false)?;
    let items = components.queryItems()?;
    let target = items.iter().find_map(|item| {
        (item.name().to_string().eq_ignore_ascii_case("url"))
            .then(|| item.value())
            .flatten()
    })?;
    let candidate = NSURL::URLWithString(&target)?;
    is_web_url(&candidate).then_some(candidate)
}

fn is_safelink_wrapper(url: &NSURL) -> bool {
    let Some(host) = url.host().map(|h| h.to_string().to_ascii_lowercase()) else {
        return false;
    };
    let path = url
        .path()
        .map(|p| p.to_string().to_ascii_lowercase())
        .unwrap_or_default();
    host.ends_with(".safelinks.protection.outlook.com")
        || host == "safelinks.protection.outlook.com"
        || ((host == "teams.public.onecdn.static.microsoft" || host.ends_with(".microsoft.com"))
            && path.contains("/safelinks/"))
}

#[derive(Default)]
pub struct Requests {
    pending: VecDeque<Pending>,
    pending_urls: usize,
    pending_bytes: usize,
    pub in_flight: bool,
}

struct Pending {
    urls: Retained<NSArray<NSURL>>,
    bytes: usize,
}

impl Requests {
    pub fn push(&mut self, urls: &NSArray<NSURL>) -> Result<(), Rejection> {
        if urls.is_empty() {
            return Err(Rejection::Unsupported);
        }
        if urls.len() > MAX_PENDING_URLS - self.pending_urls {
            return Err(Rejection::Full);
        }
        let mut resolved = Vec::with_capacity(urls.len());
        let mut bytes = 0;
        for url in urls {
            if !is_web_url(&url) {
                return Err(Rejection::Unsupported);
            }
            let url = unwrap_url(&url);
            let value = url.absoluteString().ok_or(Rejection::Unsupported)?;
            let remaining = MAX_PENDING_URL_BYTES - self.pending_bytes - bytes;
            if value.len() > remaining {
                return Err(Rejection::Full);
            }
            let length = value.lengthOfBytesUsingEncoding(NSUTF8StringEncoding);
            if length > remaining {
                return Err(Rejection::Full);
            }
            bytes += length;
            resolved.push(url);
        }
        let urls = NSArray::from_retained_slice(&resolved);
        self.pending_urls += urls.len();
        self.pending_bytes += bytes;
        self.pending.push_back(Pending { urls, bytes });
        Ok(())
    }

    pub fn current(&self) -> Option<&NSArray<NSURL>> {
        self.pending.front().map(|pending| &*pending.urls)
    }

    pub fn begin(&mut self) -> Option<Retained<NSArray<NSURL>>> {
        if self.in_flight {
            return None;
        }
        let urls = self.pending.front()?.urls.clone();
        self.in_flight = true;
        Some(urls)
    }

    pub fn finish(&mut self, success: bool) {
        if !self.in_flight {
            return;
        }
        self.in_flight = false;
        if success {
            self.remove_current();
        }
    }

    pub fn cancel(&mut self) {
        if !self.in_flight {
            self.remove_current();
        }
    }

    fn remove_current(&mut self) {
        if let Some(pending) = self.pending.pop_front() {
            self.pending_urls -= pending.urls.len();
            self.pending_bytes -= pending.bytes;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2_foundation::NSString;

    fn urls(value: &str) -> Retained<NSArray<NSURL>> {
        NSArray::from_retained_slice(&[NSURL::URLWithString(&NSString::from_str(value)).unwrap()])
    }

    #[test]
    fn accepts_only_absolute_web_urls_with_hosts() {
        for value in [
            "https://example.com",
            "HTTP://localhost:3000/path",
            "http://[::1]:8000/",
        ] {
            assert!(is_web_url(&urls(value).objectAtIndex(0)), "{value}");
        }
        for value in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "/relative",
            "https:/path",
            "mailto:a@b.com",
        ] {
            assert!(!is_web_url(&urls(value).objectAtIndex(0)), "{value}");
        }
    }

    #[test]
    fn unwraps_teams_and_outlook_safelinks_to_original_destination() {
        let teams = "https://teams.public.onecdn.static.microsoft/evergreen-assets/safelinks/2/atp-safelinks.html?url=https%3A%2F%2Fwww.example.com%2Finbox%3Fq%3D1%26b%3D2&locale=en-us&dest=https%3A%2F%2Fteams.microsoft.com%2F";
        let mut requests = Requests::default();
        requests.push(&urls(teams)).unwrap();
        assert_eq!(
            requests
                .current()
                .unwrap()
                .objectAtIndex(0)
                .absoluteString()
                .unwrap()
                .to_string(),
            "https://www.example.com/inbox?q=1&b=2"
        );
        let evil = "https://teams.public.onecdn.static.microsoft/evergreen-assets/safelinks/2/atp-safelinks.html?url=file%3A%2F%2F%2Fetc%2Fpasswd";
        assert_eq!(
            unwrap_url(&urls(evil).objectAtIndex(0))
                .absoluteString()
                .unwrap()
                .to_string(),
            evil
        );
    }

    #[test]
    fn retains_encoding_query_and_fragment() {
        let value = "https://example.com/a%2Fb?q=a%26b&token=secret#part%202";
        let mut requests = Requests::default();
        assert_eq!(requests.push(&urls(value)), Ok(()));
        assert_eq!(
            requests
                .begin()
                .unwrap()
                .objectAtIndex(0)
                .absoluteString()
                .unwrap()
                .to_string(),
            value
        );
    }

    #[test]
    fn rejects_relative_urls_even_with_a_web_base() {
        let base = NSURL::URLWithString(&NSString::from_str("https://example.com")).unwrap();
        let relative =
            NSURL::URLWithString_relativeToURL(&NSString::from_str("/path"), Some(&base)).unwrap();
        assert!(!is_web_url(&relative));
    }

    #[test]
    fn empty_requests_and_unsolicited_completions_do_nothing() {
        let mut requests = Requests::default();
        assert_eq!(requests.push(&NSArray::new()), Err(Rejection::Unsupported));
        assert!(requests.begin().is_none());
        requests.push(&urls("https://example.com")).unwrap();
        requests.finish(true);
        assert!(requests.current().is_some());
        requests.cancel();
        requests.cancel();
        assert!(requests.current().is_none());
    }

    #[test]
    fn rejects_an_entire_mixed_batch() {
        let mut requests = Requests::default();
        let batch = NSArray::from_retained_slice(&[
            urls("https://example.com").objectAtIndex(0),
            urls("file:///private").objectAtIndex(0),
        ]);
        assert_eq!(requests.push(&batch), Err(Rejection::Unsupported));
        assert!(requests.current().is_none());
        assert_eq!(requests.pending_urls, 0);
        assert_eq!(requests.pending_bytes, 0);
    }

    #[test]
    fn queues_deliveries_while_launching_and_retries_failures() {
        let mut requests = Requests::default();
        requests.push(&urls("https://first.example")).unwrap();
        assert!(requests.begin().is_some());
        assert!(requests.begin().is_none());
        requests.push(&urls("https://second.example")).unwrap();
        requests.cancel();
        requests.finish(false);
        assert_eq!(
            requests
                .current()
                .unwrap()
                .objectAtIndex(0)
                .host()
                .unwrap()
                .to_string(),
            "first.example"
        );
        requests.begin();
        requests.finish(true);
        assert_eq!(
            requests
                .current()
                .unwrap()
                .objectAtIndex(0)
                .host()
                .unwrap()
                .to_string(),
            "second.example"
        );
        requests.cancel();
        assert!(requests.current().is_none());
    }

    #[test]
    fn count_limit_rejects_whole_batches_and_recovers_after_dispatch_or_cancel() {
        let mut requests = Requests::default();
        let url = urls("https://example.com").objectAtIndex(0);
        let batch = NSArray::from_retained_slice(&vec![url.clone(); MAX_PENDING_URLS - 1]);
        requests.push(&batch).unwrap();
        let pair = NSArray::from_retained_slice(&[url.clone(), url.clone()]);
        assert_eq!(requests.push(&pair), Err(Rejection::Full));
        assert_eq!(requests.pending_urls, MAX_PENDING_URLS - 1);
        requests
            .push(&NSArray::from_retained_slice(std::slice::from_ref(&url)))
            .unwrap();
        assert_eq!(requests.push(&pair), Err(Rejection::Full));
        requests.begin().unwrap();
        requests.cancel();
        requests.finish(false);
        assert_eq!(requests.push(&pair), Err(Rejection::Full));
        requests.begin().unwrap();
        requests.finish(true);
        requests.push(&pair).unwrap();
        requests.cancel();
        requests.cancel();
        assert_eq!(requests.pending_urls, 0);
        assert_eq!(requests.pending_bytes, 0);
        let oversized = NSArray::from_retained_slice(&vec![url; MAX_PENDING_URLS + 1]);
        assert_eq!(requests.push(&oversized), Err(Rejection::Full));
        assert!(requests.current().is_none());
    }

    #[test]
    fn byte_limit_is_atomic_and_released_when_a_request_is_removed() {
        let mut requests = Requests::default();
        let prefix = "https://example.com/";
        let value = format!(
            "{prefix}{}",
            "a".repeat(MAX_PENDING_URL_BYTES / 2 - prefix.len())
        );
        let half = urls(&value).objectAtIndex(0);
        let batch = NSArray::from_retained_slice(&[half.clone(), half]);
        requests.push(&batch).unwrap();
        assert_eq!(requests.pending_bytes, MAX_PENDING_URL_BYTES);
        assert_eq!(requests.push(&urls(prefix)), Err(Rejection::Full));
        requests.begin().unwrap();
        requests.finish(false);
        assert_eq!(requests.pending_bytes, MAX_PENDING_URL_BYTES);
        requests.cancel();
        requests.push(&urls(prefix)).unwrap();
        assert_eq!(requests.push(&batch), Err(Rejection::Full));
        assert_eq!(requests.pending_urls, 1);
        assert_eq!(requests.pending_bytes, prefix.len());
        requests.begin().unwrap();
        requests.finish(true);
        assert_eq!(requests.pending_bytes, 0);
        assert_eq!(requests.pending_urls, 0);
    }

    #[test]
    fn queued_batches_are_immutable_snapshots_of_the_delivered_array() {
        let batch = objc2_foundation::NSMutableArray::new();
        batch.addObjectsFromArray(&urls("https://example.com"));
        let mut requests = Requests::default();
        requests.push(&batch).unwrap();
        batch.removeAllObjects();
        assert_eq!(requests.current().unwrap().len(), 1);
        assert_eq!(requests.pending_urls, 1);
        requests.cancel();
        assert_eq!(requests.pending_urls, 0);
        assert_eq!(requests.pending_bytes, 0);
    }
}
