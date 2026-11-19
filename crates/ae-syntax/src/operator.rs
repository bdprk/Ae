use crate::Kind;

/// Binding power required of a prefix operator's operand.
pub const PREFIX_BINDING_POWER: u8 = 10;

/// Binding powers of a binary operator and its right operand.
/// The higher right binding power makes operators left-associative.
pub fn infix_binding_power(kind: &Kind) -> Option<(u8, u8)> {
    let left = match kind {
        Kind::Or => 1,
        Kind::And => 2,
        Kind::Pipe => 3,
        Kind::Caret => 4,
        Kind::Amp => 5,
        Kind::EqEq | Kind::NotEq | Kind::Less | Kind::LessEq
        | Kind::Greater | Kind::GreaterEq => 6,
        Kind::Shl | Kind::Shr => 7,
        Kind::Plus | Kind::Minus => 8,
        Kind::Star | Kind::Slash | Kind::Percent => 9,
        _ => return None,
    };
    Some((left, left + 1))
}
