use base64::{engine::general_purpose::STANDARD, Engine as _};

pub fn embedded_html_navigation(html: &str) -> impl Fn(String) -> bool {
    // NavigateToString reports the embedded document as a data URI on WebView2.
    // Match the document itself, not the data scheme, to keep arbitrary pages blocked.
    let expected = format!("data:text/html;charset=utf-8;base64,{}", STANDARD.encode(html.as_bytes()));
    move |url| {
        let allowed = url == expected;
        allowed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_only_the_exact_embedded_document() {
        let html = "<html><body>Candidate fixture</body></html>";
        let allow = embedded_html_navigation(html);
        assert!(allow(format!("data:text/html;charset=utf-8;base64,{}", STANDARD.encode(html))));
        for url in ["about:blank", "https://example.invalid/", "file:///C:/fixture.txt",
            "javascript:alert(1)", "data:text/html,<script>alert(1)</script>",
            "data:text/html;base64,PHNjcmlwdD4="] {
            assert!(!allow(url.to_owned()), "{url}");
        }
        assert!(!allow(format!("data:text/html;charset=utf-8;base64,{}", STANDARD.encode(format!("{html} ")))));
    }
}
