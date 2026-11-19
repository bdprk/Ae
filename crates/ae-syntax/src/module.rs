use ae_span::Diagnostic;

use crate::{diagnostic_range, Kind, Token};

pub struct UseHeaders {
    pub paths: Vec<String>,
    pub body_start: usize,
}

/// Parse consecutive leading `use "path";` declarations before the module body.
pub fn parse_use_headers(file: &str, source: &str, tokens: &[Token]) -> Result<UseHeaders, Diagnostic> {
    let mut paths = Vec::new();
    let mut pos = 0;
    let error = |index: usize, message| {
        let range = tokens.get(index).map(|token| token.offset..token.end).unwrap_or(0..0);
        diagnostic_range(file, source, range, message)
    };
    while matches!(tokens.get(pos).map(|token| &token.kind), Some(Kind::Use)) {
        let Some(Kind::Text(path)) = tokens.get(pos + 1).map(|token| &token.kind) else {
            return Err(error(pos + 1, "expected a string path after 'use'"));
        };
        if !matches!(tokens.get(pos + 2).map(|token| &token.kind), Some(Kind::Semi)) {
            return Err(error(pos + 2, "expected ';' after 'use' path"));
        }
        paths.push(path.clone());
        pos += 3;
    }
    Ok(UseHeaders { paths, body_start: pos })
}
