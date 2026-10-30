//! A minimal element tree over quick-xml for the XML importers.

use quick_xml::escape::resolve_predefined_entity;
use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Node {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Node>,
    pub text: String,
}

impl Node {
    pub fn attr(&self, k: &str) -> Option<&str> {
        self.attrs.iter().find(|(a, _)| a == k).map(|(_, v)| v.as_str())
    }

    pub fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == name)
    }

    pub fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> + 'a {
        self.children.iter().filter(move |c| c.name == name)
    }

    /// Descendant by a `/`-separated path of child names ("rate/timebase").
    pub fn at(&self, path: &str) -> Option<&Node> {
        path.split('/').try_fold(self, |n, p| n.child(p))
    }

    pub fn text_at(&self, path: &str) -> Option<&str> {
        self.at(path).map(|n| n.text.trim())
    }

    /// Integer text at `path` (tolerates "123.0").
    pub fn int_at(&self, path: &str) -> Option<i64> {
        let t = self.text_at(path)?;
        t.parse().ok().or_else(|| t.parse::<f64>().ok().map(|f| f.round() as i64))
    }

    /// All descendants named `name`, depth first.
    pub fn find<'a>(&'a self, name: &str, out: &mut Vec<&'a Node>) {
        for c in &self.children {
            if c.name == name {
                out.push(c);
            }
            c.find(name, out);
        }
    }
}

fn element(e: &BytesStart) -> Result<Node, String> {
    let name = e.name().as_ref().to_string();
    let mut attrs = Vec::new();
    for a in e.attributes() {
        let a = a.map_err(|x| x.to_string())?;
        let k = a.key.as_ref().to_string();
        let v = a.normalized_value(XmlVersion::Implicit1_0).map_err(|x| x.to_string())?.into_owned();
        attrs.push((k, v));
    }
    Ok(Node { name, attrs, ..Node::default() })
}

/// Parses a document into its root element.
pub fn parse(xml: &str) -> Result<Node, String> {
    let mut r = Reader::from_str(xml);
    r.config_mut().check_end_names = true;
    let mut stack: Vec<Node> = vec![Node::default()];
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => stack.push(element(&e)?),
            Ok(Event::Empty(e)) => {
                let n = element(&e)?;
                if let Some(p) = stack.last_mut() {
                    p.children.push(n);
                }
            }
            Ok(Event::End(_)) => {
                let n = stack.pop().ok_or("unbalanced end tag")?;
                stack.last_mut().ok_or("unbalanced end tag")?.children.push(n);
            }
            Ok(Event::Text(t)) => {
                if let Some(s) = stack.last_mut().map(|n| &mut n.text) {
                    s.push_str(&t.xml10_content());
                }
            }
            Ok(Event::CData(t)) => {
                if let Some(s) = stack.last_mut().map(|n| &mut n.text) {
                    s.push_str(&t);
                }
            }
            Ok(Event::GeneralRef(g)) => {
                let s = stack.last_mut().map(|n| &mut n.text).ok_or("text outside the root")?;
                if let Ok(Some(c)) = g.resolve_char_ref() {
                    s.push(c);
                } else {
                    let name: &str = &g;
                    s.push_str(resolve_predefined_entity(name).unwrap_or(""));
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => return Err(format!("XML error at byte {}: {e}", r.buffer_position())),
        }
    }
    if stack.len() != 1 {
        return Err("unclosed elements".into());
    }
    stack.pop().and_then(|mut d| d.children.pop()).ok_or_else(|| "empty document".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_text_entities_and_attrs() {
        let n = parse("<?xml version=\"1.0\"?><!DOCTYPE x><a k=\"1 &amp; 2\"><b>x &lt; y &#233;</b><c/><b>2.0</b></a>").unwrap();
        assert_eq!(n.name, "a");
        assert_eq!(n.attr("k"), Some("1 & 2"));
        assert_eq!(n.text_at("b"), Some("x < y é"));
        assert_eq!(n.all("b").count(), 2);
        assert_eq!(n.all("b").nth(1).and_then(|b| b.text.parse::<f64>().ok()), Some(2.0));
        assert!(n.child("c").is_some());
        assert!(parse("<a><b></a>").is_err());
    }
}
