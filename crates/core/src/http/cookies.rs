//! A cookie jar shared by all requests in a session, so login flows just work.

use std::sync::Mutex;
use url::Url;

#[derive(Debug, Default)]
pub struct CookieJar {
    store: Mutex<cookie_store::CookieStore>,
}

impl CookieJar {
    pub fn new() -> Self {
        Self::default()
    }

    /// The `Cookie` header value to send to `url`, if any cookies match.
    pub fn header_for(&self, url: &Url) -> Option<String> {
        let store = self.store.lock().expect("cookie lock poisoned");
        let pairs: Vec<String> = store.get_request_values(url).map(|(name, value)| format!("{name}={value}")).collect();
        (!pairs.is_empty()).then(|| pairs.join("; "))
    }

    /// Stores cookies from `Set-Cookie` response headers.
    pub fn store<'a>(&self, url: &Url, set_cookie_values: impl Iterator<Item = &'a str>) {
        let mut store = self.store.lock().expect("cookie lock poisoned");
        for value in set_cookie_values {
            // Invalid cookies are ignored, as browsers do.
            let _ = store.parse(value, url);
        }
    }

    /// All cookies as (domain, name, value).
    pub fn list(&self) -> Vec<(String, String, String)> {
        let store = self.store.lock().expect("cookie lock poisoned");
        store.iter_unexpired().map(|c| (c.domain().unwrap_or_default().to_string(), c.name().to_string(), c.value().to_string())).collect()
    }

    pub fn clear(&self) {
        self.store.lock().expect("cookie lock poisoned").clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookies_are_sent_back_to_the_same_host() {
        let jar = CookieJar::new();
        let url = Url::parse("https://api.example.com/login").unwrap();
        jar.store(&url, ["session=abc; Path=/"].into_iter());
        assert_eq!(jar.header_for(&Url::parse("https://api.example.com/me").unwrap()).as_deref(), Some("session=abc"));
        assert_eq!(jar.header_for(&Url::parse("https://other.example.org/").unwrap()), None);
    }
}
