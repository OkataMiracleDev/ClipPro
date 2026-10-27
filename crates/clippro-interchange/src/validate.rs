//! Structural validation used by tests and by `clippro-cli export --check`.

use quick_xml::events::Event;
use quick_xml::Reader;

/// Parses an XML document fully; returns an error on malformed markup or mismatched tags.
pub fn well_formed(xml: &str) -> Result<usize, String> {
    let mut r = Reader::from_str(xml);
    r.config_mut().check_end_names = true;
    let mut depth: i64 = 0;
    let mut elements = 0usize;
    loop {
        match r.read_event() {
            Ok(Event::Start(_)) => {
                depth += 1;
                elements += 1;
            }
            Ok(Event::Empty(_)) => elements += 1,
            Ok(Event::End(_)) => depth -= 1,
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => return Err(format!("at byte {}: {e}", r.buffer_position())),
        }
    }
    if depth != 0 {
        return Err(format!("unbalanced elements (depth {depth})"));
    }
    Ok(elements)
}
