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

/// Parse the binary continuation of an expression whose left operand has
/// already been read. The caller consumes the operator, parses its right
/// operand at the supplied binding power, and constructs the combined node.
/// Combining each pair before inspecting the next operator preserves the
/// caller's diagnostic and operand-evaluation order.
pub fn parse_infix_tail<P, E, D>(
    parser: &mut P,
    mut left: E,
    minimum: u8,
    current: impl Fn(&P) -> &Kind,
    mut combine: impl FnMut(&mut P, E, Kind, u8) -> Result<E, D>,
) -> Result<E, D> {
    while let Some((left_power, right_power)) = infix_binding_power(current(parser)) {
        if left_power < minimum {
            break;
        }
        let operator = current(parser).clone();
        left = combine(parser, left, operator, right_power)?;
    }
    Ok(left)
}
