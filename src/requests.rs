use std::collections::VecDeque;

use objc2::rc::Retained;
use objc2_foundation::{NSArray, NSURL, NSURLComponents};

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

fn safelink_destination(url: &NSURL) -> Option<Retained<NSURL>> {
    if !url.scheme()?.to_string().eq_ignore_ascii_case("https") {
        return None;
    }
    let host = url.host()?.to_string().to_ascii_lowercase();
    let outlook = host == "safelinks.protection.outlook.com"
        || host.ends_with(".safelinks.protection.outlook.com");
    let teams = host == "teams.public.onecdn.static.microsoft"
        && url.path().is_some_and(|path| {
            let path = path.to_string();
            path.starts_with("/evergreen-assets/safelinks/")
                && path.ends_with("/atp-safelinks.html")
        });
    if !outlook && !teams {
        return None;
    }
    let components = NSURLComponents::componentsWithURL_resolvingAgainstBaseURL(url, false)?;
    let items = components.queryItems()?;
    let mut targets = items
        .iter()
        .filter(|item| item.name().to_string().eq_ignore_ascii_case("url"));
    let target = targets.next()?.value()?;
    if targets.next().is_some() {
        return None;
    }
    let destination = NSURL::URLWithString(&target)?;
    is_web_url(&destination).then_some(destination)
}

fn link_destination(url: &Retained<NSURL>) -> Retained<NSURL> {
    let mut destination = url.clone();
    for _ in 0..4 {
        let Some(inner) = safelink_destination(&destination) else {
            break;
        };
        destination = inner;
    }
    destination
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
    destinations: Retained<NSArray<NSURL>>,
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
        let mut original = Vec::with_capacity(urls.len());
        let mut bytes = 0;
        for url in urls {
            let value = url.relativeString();
            let remaining = MAX_PENDING_URL_BYTES - self.pending_bytes - bytes;
            if value.len_utf16() > remaining {
                return Err(Rejection::Full);
            }
            let length = value.len();
            if length > remaining {
                return Err(Rejection::Full);
            }
            bytes += length;
            original.push(url);
        }
        if !original.iter().all(|url| is_web_url(url)) {
            return Err(Rejection::Unsupported);
        }
        let destinations: Vec<_> = original.iter().map(link_destination).collect();
        let destinations = NSArray::from_retained_slice(&destinations);
        let urls = NSArray::from_retained_slice(&original);
        self.pending_urls += urls.len();
        self.pending_bytes += bytes;
        self.pending.push_back(Pending {
            urls,
            destinations,
            bytes,
        });
        Ok(())
    }

    pub fn current(&self) -> Option<&NSArray<NSURL>> {
        self.pending.front().map(|pending| &*pending.urls)
    }

    pub fn destinations(&self) -> Option<&NSArray<NSURL>> {
        self.pending.front().map(|pending| &*pending.destinations)
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
    fn decodes_safelink_destinations_without_rewriting_forwarded_wrappers() {
        for (wrapper, destination) in [
            (
                "https://teams.public.onecdn.static.microsoft/evergreen-assets/safelinks/2/atp-safelinks.html?url=https%3A%2F%2Fwww.example.com%2Finbox%3Fq%3D1%26b%3D2&locale=en-us&dest=https%3A%2F%2Fteams.microsoft.com%2F",
                "https://www.example.com/inbox?q=1&b=2",
            ),
            (
                "https://na01.safelinks.protection.outlook.com/?url=https%3A%2F%2Fwww.example.com%2Fa%252Fb%3Fq%3Da+b%23part%25202&data=token%2F1#outer",
                "https://www.example.com/a%2Fb?q=a+b#part%202",
            ),
            (
                "https://na01.safelinks.protection.outlook.com/?url=https%3A%2F%2Feu01.safelinks.protection.outlook.com%2F%3Furl%3Dhttps%253A%252F%252Fexample.com",
                "https://example.com",
            ),
        ] {
            let mut requests = Requests::default();
            let delivered = urls(wrapper);
            requests.push(&delivered).unwrap();
            let decoded = requests.destinations().unwrap().objectAtIndex(0);
            assert_eq!(decoded.absoluteString().unwrap().to_string(), destination);
            let forwarded = requests.begin().unwrap().objectAtIndex(0);
            assert!(std::ptr::eq(&*forwarded, &*delivered.objectAtIndex(0)));
            assert_eq!(forwarded.absoluteString().unwrap().to_string(), wrapper);
            assert_eq!(requests.pending_bytes, wrapper.len());
        }
    }

    #[test]
    fn leaves_unrecognized_or_invalid_safelinks_untouched() {
        for wrapper in [
            "https://na01.safelinks.protection.outlook.com.example.org/?url=https%3A%2F%2Fexample.com",
            "https://teams.public.onecdn.static.microsoft/other?url=https%3A%2F%2Fexample.com",
            "https://na01.safelinks.protection.outlook.com/?url=file%3A%2F%2F%2Fprivate",
            "https://na01.safelinks.protection.outlook.com/?url=%2Frelative",
            "https://na01.safelinks.protection.outlook.com/?url=https%3A%2F%2Fexample.com&URL=https%3A%2F%2Fother.example",
        ] {
            let url = urls(wrapper).objectAtIndex(0);
            assert!(std::ptr::eq(&*link_destination(&url), &*url));
        }
    }

    #[test]
    fn original_batch_size_is_checked_before_url_validation() {
        let mut requests = Requests::default();
        let prefix = "https://example.com/";
        requests.push(&urls(prefix)).unwrap();
        let wrapper = urls(&format!(
            "https://na01.safelinks.protection.outlook.com/?url=https%3A%2F%2Fexample.com&data={}",
            "x".repeat(MAX_PENDING_URL_BYTES)
        ));
        let batch = NSArray::from_retained_slice(&[
            urls("file:///unsupported").objectAtIndex(0),
            wrapper.objectAtIndex(0),
        ]);
        assert_eq!(requests.push(&batch), Err(Rejection::Full));
        assert_eq!(requests.push(&wrapper), Err(Rejection::Full));
        assert_eq!(requests.pending_urls, 1);
        assert_eq!(requests.pending_bytes, prefix.len());
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
