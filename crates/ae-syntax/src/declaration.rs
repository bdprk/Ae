use crate::{Kind, Token};

/// Find the token immediately after the brace body whose opening brace has
/// already been consumed. On failure, return the position of the end token.
pub fn brace_body_end(tokens: &[Token], start: usize) -> Result<usize, usize> {
    let mut depth = 1;
    for (index, token) in tokens.iter().enumerate().skip(start) {
        match token.kind {
            Kind::LBrace => depth += 1,
            Kind::RBrace => {
                depth -= 1;
                if depth == 0 {
                    return Ok(index + 1);
                }
            }
            Kind::Eof => return Err(index),
            _ => {}
        }
    }
    Err(tokens.len())
}
