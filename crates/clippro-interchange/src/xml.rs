//! Minimal indented XML writer with correct escaping.

pub struct Xml {
    out: String,
    stack: Vec<&'static str>,
}

pub fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&apos;"),
            c if (c as u32) < 0x20 && c != '\n' && c != '\t' => {}
            c => o.push(c),
        }
    }
    o
}

impl Xml {
    pub fn new(doctype: Option<&str>) -> Xml {
        let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        if let Some(d) = doctype {
            out.push_str(d);
            out.push('\n');
        }
        Xml { out, stack: Vec::new() }
    }

    fn indent(&mut self) {
        for _ in 0..self.stack.len() {
            self.out.push_str("  ");
        }
    }

    fn attrs(&mut self, attrs: &[(&str, String)]) {
        for (k, v) in attrs {
            self.out.push(' ');
            self.out.push_str(k);
            self.out.push_str("=\"");
            self.out.push_str(&esc(v));
            self.out.push('"');
        }
    }

    pub fn open(&mut self, tag: &'static str, attrs: &[(&str, String)]) -> &mut Self {
        self.indent();
        self.out.push('<');
        self.out.push_str(tag);
        self.attrs(attrs);
        self.out.push_str(">\n");
        self.stack.push(tag);
        self
    }

    pub fn close(&mut self) -> &mut Self {
        if let Some(tag) = self.stack.pop() {
            self.indent();
            self.out.push_str("</");
            self.out.push_str(tag);
            self.out.push_str(">\n");
        }
        self
    }

    /// `<tag attrs/>`
    pub fn empty(&mut self, tag: &str, attrs: &[(&str, String)]) -> &mut Self {
        self.indent();
        self.out.push('<');
        self.out.push_str(tag);
        self.attrs(attrs);
        self.out.push_str("/>\n");
        self
    }

    /// `<tag>text</tag>`
    pub fn text(&mut self, tag: &str, text: impl AsRef<str>) -> &mut Self {
        self.indent();
        self.out.push('<');
        self.out.push_str(tag);
        self.out.push('>');
        self.out.push_str(&esc(text.as_ref()));
        self.out.push_str("</");
        self.out.push_str(tag);
        self.out.push_str(">\n");
        self
    }

    pub fn finish(mut self) -> String {
        while !self.stack.is_empty() {
            self.close();
        }
        self.out
    }
}

/// `file://localhost/D:/Path%20With%20Spaces/clip.mov` style URL (FCP7 XML `pathurl`).
pub fn file_url_localhost(path: &std::path::Path) -> String {
    format!("file://localhost/{}", url_path(path))
}

/// `file:///D:/Path%20With%20Spaces/clip.mov` (FCPXML, OTIO).
pub fn file_url(path: &std::path::Path) -> String {
    format!("file:///{}", url_path(path))
}

fn url_path(path: &std::path::Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    let s = s.trim_start_matches('/');
    let mut o = String::new();
    for (i, b) in s.bytes().enumerate() {
        let keep = b.is_ascii_alphanumeric() || b"-._~/".contains(&b) || (b == b':' && i == 1);
        if keep {
            o.push(b as char);
        } else {
            o.push_str(&format!("%{b:02X}"));
        }
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaping_and_urls() {
        assert_eq!(esc("a<b & \"c\"'"), "a&lt;b &amp; &quot;c&quot;&apos;");
        let p = std::path::Path::new(r"D:\Footage\Day 1\Ça va #2.mov");
        assert_eq!(file_url(p), "file:///D:/Footage/Day%201/%C3%87a%20va%20%232.mov");
        assert_eq!(file_url_localhost(p), "file://localhost/D:/Footage/Day%201/%C3%87a%20va%20%232.mov");
        let unc = std::path::Path::new(r"\\server\share\a.mov");
        assert_eq!(file_url(unc), "file:///server/share/a.mov");
        let mut x = Xml::new(None);
        x.open("a", &[("k", "v&w".into())]).text("b", "1<2").empty("c", &[]);
        let s = x.finish();
        assert!(s.contains("<a k=\"v&amp;w\">") && s.contains("<b>1&lt;2</b>") && s.contains("</a>"));
    }
}
