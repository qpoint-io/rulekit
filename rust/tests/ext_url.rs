//! `url::Url` and `http::Uri` match rulekit's own URL for the same text
//! after those crates have normalized it.

use rulekit::value::{Url, Value};
use rulekit::{Input, KvInput, Opts};

const URLS: &[&str] = &[
    "https://example.com/a",
    "https://example.com:8443/a",
    "https://ada@example.com/a",
    "https://ada:secret@example.com/a?env=prod&x=1#top",
    "http://example.com/",
    "https://example.com",
    "http://example.com:80/a",
    "https://example.com:443/a",
    "http://[::1]/a",
    "https://a%20b@example.com/p%20q?q=%20#c%20d",
    "https://example.com/a?",
    "https://example.com/a#",
    "HTTP://Example.COM/Path",
    "https://example.com/a?env=prod",
];

fn rulekit_input(text: &str) -> Option<KvInput> {
    let url = Url::parse(text).ok()?;
    Some(KvInput::from_values(rulekit::value::Map::from_iter([(
        "page".to_owned(),
        Value::Url(Box::new(url)),
    )])))
}

fn same(label: &str, text: &str, expr: &str, external: &impl Input) {
    let Some(native) = rulekit_input(text) else {
        return;
    };
    let rule = rulekit::parse(expr).unwrap();
    let want = rule.eval(&(), &native, Opts::default());
    let got = rule.eval(&(), external, Opts::default());
    assert_eq!(
        got.error().map(|e| e.to_string()),
        want.error().map(|e| e.to_string()),
        "{label} {text} {expr} error"
    );
    assert_eq!(
        got.unknown(),
        want.unknown(),
        "{label} {text} {expr} missing got {:?} want {:?}",
        got.missing_fields().collect::<Vec<_>>(),
        want.missing_fields().collect::<Vec<_>>()
    );
    assert_eq!(
        got.value().to_owned(),
        want.value().to_owned(),
        "{label} {text} {expr}"
    );
}

fn check(label: &str, text: &str, external: &impl Input) {
    for expr in [
        "page.scheme",
        "page.user",
        "page.host",
        "page.port",
        "page.path",
        "page.query",
        "page.fragment",
        "page.query.env",
        "page == page",
    ] {
        same(label, text, expr, external);
    }
    let text_form = Url::parse(text)
        .map(|u| u.as_str().to_owned())
        .unwrap_or_default();
    let quoted = text_form.replace('\\', "\\\\").replace('"', "\\\"");
    same(label, text, &format!("page == \"{quoted}\""), external);
}

#[test]
fn url_crate_matches_rulekit_url() {
    for text in URLS {
        let Ok(parsed) = url::Url::parse(text) else {
            continue;
        };
        let normalized = parsed.as_str().to_owned();
        let input = std::collections::HashMap::from([("page".to_owned(), parsed)]);
        check("url", &normalized, &input);
    }
}

#[test]
fn http_uri_matches_rulekit_url() {
    for text in URLS {
        let Ok(parsed) = text.parse::<http::Uri>() else {
            continue;
        };
        let normalized = parsed.to_string();
        let input = std::collections::HashMap::from([("page".to_owned(), parsed)]);
        check("http", &normalized, &input);
    }
}
