use std::collections::VecDeque;

use objc2::{Message, rc::Retained};
use objc2_foundation::{NSArray, NSURL};

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

#[derive(Default)]
pub struct Requests {
    pending: VecDeque<Retained<NSArray<NSURL>>>,
    pub in_flight: bool,
}

impl Requests {
    pub fn push(&mut self, urls: &NSArray<NSURL>) -> bool {
        if urls.is_empty() || !urls.iter().all(|url| is_web_url(&url)) {
            return false;
        }
        self.pending.push_back(urls.retain());
        true
    }

    pub fn current(&self) -> Option<&NSArray<NSURL>> {
        self.pending.front().map(|urls| &**urls)
    }

    pub fn begin(&mut self) -> Option<Retained<NSArray<NSURL>>> {
        if self.in_flight {
            return None;
        }
        let urls = self.pending.front()?.clone();
        self.in_flight = true;
        Some(urls)
    }

    pub fn finish(&mut self, success: bool) {
        if !self.in_flight {
            return;
        }
        self.in_flight = false;
        if success {
            self.pending.pop_front();
        }
    }

    pub fn cancel(&mut self) {
        if !self.in_flight {
            self.pending.pop_front();
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
    fn retains_encoding_query_and_fragment() {
        let value = "https://example.com/a%2Fb?q=a%26b&token=secret#part%202";
        let mut requests = Requests::default();
        assert!(requests.push(&urls(value)));
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
        assert!(!requests.push(&NSArray::new()));
        assert!(requests.begin().is_none());
        requests.push(&urls("https://example.com"));
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
        assert!(!requests.push(&batch));
        assert!(requests.current().is_none());
    }

    #[test]
    fn queues_deliveries_while_launching_and_retries_failures() {
        let mut requests = Requests::default();
        requests.push(&urls("https://first.example"));
        assert!(requests.begin().is_some());
        assert!(requests.begin().is_none());
        requests.push(&urls("https://second.example"));
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
}
