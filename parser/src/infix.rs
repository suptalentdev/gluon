//! Infix expressions in gluon are initially parsed as if they were all left-
//! associative with the same precedence. Therefore we need to rebalance them
//! after the fact.

use base::ast::{walk_mut_expr, Expr, IdentEnv, MutVisitor, SpannedExpr, SpannedIdent};
use base::error::Errors;
use base::fnv::FnvMap;
use base::pos::{self, BytePos, Spanned};
use std::cmp::Ordering;
use std::error::Error as StdError;
use std::fmt;
use std::hash::Hash;
use std::marker::PhantomData;
use std::mem;

/// The fixity (associativity) of an infix operator
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Fixity {
    /// Left operator associativity.
    ///
    /// For example, when the `(~)` operator is left-associative:
    ///
    /// ```text
    /// x ~ y ~ z ≡ (x ~ y) ~ z
    /// ```
    Left,
    /// Right operator associativity.
    ///
    /// For example, when the `(~)` operator is right-associative:
    ///
    /// ```text
    /// x ~ y ~ z ≡ x ~ (y ~ z)
    /// ```
    Right,
}

impl fmt::Display for Fixity {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Fixity::Left => write!(f, "infixl"),
            Fixity::Right => write!(f, "infixr"),
        }
    }
}

/// Metadata pertaining to an infix operator
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct OpMeta {
    /// The precedence of the operator
    pub precedence: i32,
    /// The fixity of the operator
    pub fixity: Fixity,
}

impl OpMeta {
    pub fn new(precedence: i32, fixity: Fixity) -> OpMeta {
        OpMeta {
            precedence: precedence,
            fixity: fixity,
        }
    }
}

impl fmt::Display for OpMeta {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} {}", self.fixity, self.precedence)
    }
}

/// A table of operator metadata
pub struct OpTable<Id> {
    pub operators: FnvMap<Id, OpMeta>,
}

impl<Id> OpTable<Id> {
    pub fn new<I>(ops: I) -> OpTable<Id>
    where
        I: IntoIterator<Item = (Id, OpMeta)>,
        Id: Eq + Hash,
    {
        OpTable {
            operators: ops.into_iter().collect(),
        }
    }
}

impl<Id> OpTable<Id>
where
    Id: Eq + Hash + AsRef<str> + ::std::fmt::Debug,
{
    fn get_at(&self, name: &SpannedIdent<Id>) -> Result<&OpMeta, Spanned<Error, BytePos>> {
        self.get(&name.value.name).ok_or_else(|| {
            pos::spanned(
                name.span,
                Error::UndefinedFixity(name.value.name.as_ref().to_string()),
            )
        })
    }

    fn get(&self, name: &Id) -> Option<&OpMeta> {
        self.operators.get(name).or_else(|| {
            let name = name.as_ref();
            if name.starts_with('#') || name == "&&" || name == "||" {
                const OPS: &[(&str, OpMeta)] = &[
                    (
                        "*",
                        OpMeta {
                            precedence: 7,
                            fixity: Fixity::Left,
                        },
                    ),
                    (
                        "/",
                        OpMeta {
                            precedence: 7,
                            fixity: Fixity::Left,
                        },
                    ),
                    (
                        "+",
                        OpMeta {
                            precedence: 6,
                            fixity: Fixity::Left,
                        },
                    ),
                    (
                        "-",
                        OpMeta {
                            precedence: 6,
                            fixity: Fixity::Left,
                        },
                    ),
                    (
                        "==",
                        OpMeta {
                            precedence: 4,
                            fixity: Fixity::Left,
                        },
                    ),
                    (
                        "/=",
                        OpMeta {
                            precedence: 4,
                            fixity: Fixity::Left,
                        },
                    ),
                    (
                        "<",
                        OpMeta {
                            precedence: 4,
                            fixity: Fixity::Left,
                        },
                    ),
                    (
                        ">",
                        OpMeta {
                            precedence: 4,
                            fixity: Fixity::Left,
                        },
                    ),
                    (
                        "<=",
                        OpMeta {
                            precedence: 4,
                            fixity: Fixity::Left,
                        },
                    ),
                    (
                        ">=",
                        OpMeta {
                            precedence: 4,
                            fixity: Fixity::Left,
                        },
                    ),
                    (
                        "&&",
                        OpMeta {
                            precedence: 3,
                            fixity: Fixity::Right,
                        },
                    ),
                    (
                        "||",
                        OpMeta {
                            precedence: 2,
                            fixity: Fixity::Right,
                        },
                    ),
                ];

                let op = name
                    .trim_left_matches('#')
                    .trim_left_matches(char::is_alphanumeric);

                OPS.iter().find(|t| t.0 == op).map(|t| &t.1)
            } else {
                None
            }
        })
    }
}

pub struct Reparser<'s, Id: 's> {
    operators: OpTable<Id>,
    symbols: &'s IdentEnv<Ident = Id>,
    errors: Errors<Spanned<Error, BytePos>>,
    _marker: PhantomData<Id>,
}

impl<'s, Id> Reparser<'s, Id> {
    pub fn new(operators: OpTable<Id>, symbols: &'s IdentEnv<Ident = Id>) -> Reparser<'s, Id> {
        Reparser {
            operators: operators,
            symbols: symbols,
            errors: Errors::new(),
            _marker: PhantomData,
        }
    }

    pub fn reparse(
        &mut self,
        expr: &mut SpannedExpr<Id>,
    ) -> Result<(), Errors<Spanned<Error, BytePos>>>
    where
        Id: Eq + Hash + AsRef<str> + ::std::fmt::Debug,
    {
        self.visit_expr(expr);
        if self.errors.has_errors() {
            Err(mem::replace(&mut self.errors, Errors::new()))
        } else {
            Ok(())
        }
    }
}

impl<'a, 's, Id> MutVisitor<'a> for Reparser<'s, Id>
where
    Id: Eq + Hash + AsRef<str> + ::std::fmt::Debug + 'a,
{
    type Ident = Id;

    fn visit_expr(&mut self, e: &mut SpannedExpr<Self::Ident>) {
        if let Expr::Infix { .. } = e.value {
            let dummy = pos::spanned(e.span, Expr::Error(None));
            let expr = mem::replace(e, dummy);
            match reparse(expr, self.symbols, &self.operators) {
                Ok(expr) => {
                    *e = expr;
                }
                Err((err, reconstructed_expr)) => {
                    info!("Infix error: {}", err);
                    if let Some(reconstructed_expr) = reconstructed_expr {
                        e.value = reconstructed_expr;
                    }
                    match err.value {
                        // Undefined fixity errors are reported at the definition site
                        Error::UndefinedFixity(_) => (),
                        _ => self.errors.push(err),
                    }
                }
            }
        }
        walk_mut_expr(self, e);
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    ConflictingFixities((String, OpMeta), (String, OpMeta)),
    UndefinedFixity(String),
    InvalidFixity,
    InvalidPrecedence,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use self::Error::*;

        match *self {
            ConflictingFixities((ref lhs_name, lhs_meta), (ref rhs_name, rhs_meta)) => {
                write!(f, "Conflicting fixities at the same precedence level. ")?;
                write!(
                    f,
                    "left: `{} {}`, right: `{} {}`",
                    lhs_meta, lhs_name, rhs_meta, rhs_name
                )
            }
            UndefinedFixity(ref op) => write!(f, "No fixity specified for `{}`. Fixity must be specified with the `#[infix]` attribute", op),
            InvalidFixity => write!(
                f,
                "Only `left` or `right` is valid associativity specifications"
            ),
            InvalidPrecedence => write!(f, "Only positive integers are valid precedences"),
        }
    }
}

impl StdError for Error {
    fn description(&self) -> &str {
        "Conflicting fixities at the same precedence level"
    }
}

/// Reconstruct the infix expression using the correct associativities
/// and precedences.
///
/// Inspired by [`Language.Haskell.Infix`].
///
/// [`Language.Haskell.Infix`]: https://hackage.haskell.org/package/infix-0.1.1/docs/src/Language-Haskell-Infix.html
pub fn reparse<Id>(
    expr: SpannedExpr<Id>,
    symbols: &IdentEnv<Ident = Id>,
    operators: &OpTable<Id>,
) -> Result<SpannedExpr<Id>, (Spanned<Error, BytePos>, Option<Expr<Id>>)>
where
    Id: Eq + Hash + AsRef<str> + ::std::fmt::Debug,
{
    use self::Error::*;
    use base::pos;

    let make_op = |lhs: Box<SpannedExpr<Id>>, op, rhs: Box<SpannedExpr<Id>>| {
        let span = pos::span(lhs.span.start(), rhs.span.end());
        Box::new(pos::spanned(
            span,
            Expr::Infix {
                lhs,
                op,
                rhs,
                implicit_args: Vec::new(),
            },
        ))
    };

    let mut infixes = Infixes::new(expr);
    let mut arg_stack = Vec::new();
    let mut op_stack = Vec::new();

    while let Some(token) = infixes.next() {
        match token {
            InfixToken::Arg(next_expr) => arg_stack.push(next_expr),
            InfixToken::Op(next_op) => {
                let stack_op = match op_stack.pop() {
                    Some(stack_op) => stack_op,
                    None => {
                        op_stack.push(next_op);
                        continue;
                    }
                };

                macro_rules! try_infix {
                    ($expr:expr) => {
                        match $expr {
                            Ok(e) => e,
                            Err(err) => {
                                match infixes.remaining_expr {
                                    Some(expr) => arg_stack.push(expr),
                                    None => (),
                                }
                                op_stack.push(next_op);
                                op_stack.push(stack_op);
                                while arg_stack.len() > 1 {
                                    let rhs = arg_stack.pop().unwrap();
                                    let lhs = arg_stack.pop().unwrap();
                                    let op = op_stack.pop().unwrap();

                                    arg_stack.push(make_op(lhs, op, rhs));
                                }
                                return Err((
                                    err,
                                    arg_stack.pop().map(|original| original.value),
                                ));
                            }
                        }
                    };
                }

                let next_op_meta = *try_infix!(operators.get_at(&next_op));
                let stack_op_meta = *try_infix!(operators.get_at(&stack_op));

                match i32::cmp(&next_op_meta.precedence, &stack_op_meta.precedence) {
                    // Reduce
                    Ordering::Less => {
                        let rhs = arg_stack.pop().unwrap();
                        let lhs = arg_stack.pop().unwrap();

                        infixes.next_op = Some(next_op);
                        arg_stack.push(make_op(lhs, stack_op, rhs));
                    }
                    // Shift
                    Ordering::Greater => {
                        op_stack.push(stack_op);
                        op_stack.push(next_op);
                    }
                    Ordering::Equal => {
                        match (next_op_meta.fixity, stack_op_meta.fixity) {
                            // Reduce
                            (Fixity::Left, Fixity::Left) => {
                                let rhs = arg_stack.pop().unwrap();
                                let lhs = arg_stack.pop().unwrap();

                                infixes.next_op = Some(next_op);
                                arg_stack.push(make_op(lhs, stack_op, rhs));
                            }
                            // Shift
                            (Fixity::Right, Fixity::Right) => {
                                op_stack.push(stack_op);
                                op_stack.push(next_op);
                            }
                            // Conflicting fixities at the same precedence level!
                            (Fixity::Left, Fixity::Right) | (Fixity::Right, Fixity::Left) => {
                                let next_op_name = symbols.string(&next_op.value.name).to_string();
                                let stack_op_name =
                                    symbols.string(&stack_op.value.name).to_string();
                                let span = pos::span(stack_op.span.start(), next_op.span.end());
                                let error = ConflictingFixities(
                                    (stack_op_name, stack_op_meta),
                                    (next_op_name, next_op_meta),
                                );

                                return Err((pos::spanned(span, error), None));
                            }
                        }
                    }
                }
            }
        }
    }

    for op in op_stack.into_iter().rev() {
        let rhs = arg_stack.pop().unwrap();
        let lhs = arg_stack.pop().unwrap();
        arg_stack.push(make_op(lhs, op, rhs));
    }

    assert_eq!(arg_stack.len(), 1);

    Ok(*arg_stack.pop().unwrap())
}

#[derive(Debug, Clone, PartialEq)]
enum InfixToken<Id> {
    Arg(Box<SpannedExpr<Id>>),
    // TODO: Make this spanned to allow for accurate error reporting
    Op(SpannedIdent<Id>),
}

/// An iterator that takes an expression that has had its operators grouped
/// with _right associativity_, and yeilds a sequence of `InfixToken`s. This
/// is useful for reparsing the operators with their correct associativies
/// and precedences.
///
/// For example, the expression:
///
/// ```text
/// (1 + (2 ^ (4 * (6 - 8))))
/// ```
///
/// Will result in the following iterations:
///
/// ```text
/// Arg:  1
/// Op:   +
/// Arg:  2
/// Op:   ^
/// Arg:  4
/// Op:   *
/// Arg:  6
/// Op:   -
/// Arg:  8
/// ```
struct Infixes<Id> {
    /// The next part of the expression that we need to flatten
    remaining_expr: Option<Box<SpannedExpr<Id>>>,
    /// Cached operator from a previous iteration
    next_op: Option<SpannedIdent<Id>>,
}

impl<Id> Infixes<Id> {
    fn new(expr: SpannedExpr<Id>) -> Infixes<Id> {
        Infixes {
            remaining_expr: Some(Box::new(expr)),
            next_op: None,
        }
    }
}

impl<Id> Iterator for Infixes<Id> {
    type Item = InfixToken<Id>;

    fn next(&mut self) -> Option<InfixToken<Id>> {
        if let Some(op) = self.next_op.take() {
            return Some(InfixToken::Op(op));
        }

        self.remaining_expr.take().map(|expr| {
            let expr = *expr; // Workaround for http://stackoverflow.com/questions/28466809/
            match expr.value {
                Expr::Infix {
                    lhs,
                    op,
                    rhs,
                    implicit_args,
                } => {
                    assert!(
                        implicit_args.is_empty(),
                        "Implicit args on infix operators is not implemented"
                    );
                    self.remaining_expr = Some(rhs);
                    self.next_op = Some(op);
                    InfixToken::Arg(lhs)
                }
                _ => InfixToken::Arg(Box::new(expr)), // FIXME: remove reallocation?
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use base::ast::{DisplayEnv, Expr, IdentEnv, Literal, SpannedExpr, TypedIdent};
    use base::pos::{self, BytePos, Spanned};
    use std::marker::PhantomData;

    use super::Error::*;
    use super::*;

    fn reparse<Id>(
        expr: SpannedExpr<Id>,
        symbols: &IdentEnv<Ident = Id>,
        operators: &OpTable<Id>,
    ) -> Result<SpannedExpr<Id>, Spanned<Error, BytePos>>
    where
        Id: Eq + Hash + AsRef<str> + ::std::fmt::Debug,
    {
        super::reparse(expr, symbols, operators).map_err(|t| t.0)
    }

    pub struct MockEnv<T>(PhantomData<T>);

    impl<T> MockEnv<T> {
        pub fn new() -> MockEnv<T> {
            MockEnv(PhantomData)
        }
    }

    impl<T: AsRef<str>> DisplayEnv for MockEnv<T> {
        type Ident = T;

        fn string<'a>(&'a self, ident: &'a Self::Ident) -> &'a str {
            ident.as_ref()
        }
    }

    impl<T> IdentEnv for MockEnv<T>
    where
        T: AsRef<str> + for<'a> From<&'a str>,
    {
        fn from_str(&mut self, s: &str) -> Self::Ident {
            T::from(s)
        }
    }

    fn no_loc<T>(value: T) -> Spanned<T, BytePos> {
        pos::spanned2(BytePos::from(0), BytePos::from(0), value)
    }

    fn ident(name: &str) -> TypedIdent<String> {
        TypedIdent::new(name.to_string())
    }

    fn op(
        lhs: Box<SpannedExpr<String>>,
        op_str: &str,
        rhs: Box<SpannedExpr<String>>,
    ) -> Box<SpannedExpr<String>> {
        Box::new(no_loc(Expr::Infix {
            lhs,
            op: no_loc(ident(op_str)),
            rhs,
            implicit_args: Vec::new(),
        }))
    }

    fn int(value: i64) -> Box<SpannedExpr<String>> {
        Box::new(no_loc(Expr::Literal(Literal::Int(value))))
    }

    #[test]
    fn infixes() {
        let expr = op(
            int(1),
            "+",
            op(int(2), "^", op(int(4), "*", op(int(6), "-", int(8)))),
        );

        let result: Vec<_> = Infixes::new(*expr).collect();
        let expected = vec![
            InfixToken::Arg(int(1)),
            InfixToken::Op(no_loc(ident("+"))),
            InfixToken::Arg(int(2)),
            InfixToken::Op(no_loc(ident("^"))),
            InfixToken::Arg(int(4)),
            InfixToken::Op(no_loc(ident("*"))),
            InfixToken::Arg(int(6)),
            InfixToken::Op(no_loc(ident("-"))),
            InfixToken::Arg(int(8)),
        ];

        assert_eq!(result, expected);
    }

    #[test]
    fn reparse_single() {
        let env = MockEnv::new();
        let ops = OpTable::new(vec![]);

        let expr = *op(int(1), "+", int(2));
        let expected = Ok(expr.clone());

        assert_eq!(reparse(expr, &env, &ops), expected);
    }

    #[test]
    fn reparse_less_precedence() {
        let env = MockEnv::new();
        let ops = OpTable::new(vec![
            ("*".to_string(), OpMeta::new(7, Fixity::Left)),
            ("+".to_string(), OpMeta::new(6, Fixity::Left)),
        ]);

        // 1 + (2 * 8)
        let expr = *op(int(1), "+", op(int(2), "*", int(8)));
        let expected = Ok(expr.clone());

        assert_eq!(reparse(expr, &env, &ops), expected);
    }

    #[test]
    fn reparse_greater_precedence() {
        let env = MockEnv::new();
        let ops = OpTable::new(vec![
            ("*".to_string(), OpMeta::new(7, Fixity::Left)),
            ("+".to_string(), OpMeta::new(6, Fixity::Left)),
        ]);

        // 1 * (2 + 8)
        let expr = *op(int(1), "*", op(int(2), "+", int(8)));
        // (1 * 2) + 8
        let expected = Ok(*op(op(int(1), "*", int(2)), "+", int(8)));

        assert_eq!(reparse(expr, &env, &ops), expected);
    }

    #[test]
    fn reparse_equal_precedence_left_fixity() {
        let env = MockEnv::new();
        let ops = OpTable::new(vec![
            ("-".to_string(), OpMeta::new(6, Fixity::Left)),
            ("+".to_string(), OpMeta::new(6, Fixity::Left)),
        ]);

        // 1 + (2 - 8)
        let expr = *op(int(1), "+", op(int(2), "-", int(8)));
        // (1 + 2) - 8
        let expected = Ok(*op(op(int(1), "+", int(2)), "-", int(8)));

        assert_eq!(reparse(expr, &env, &ops), expected);
    }

    #[test]
    fn reparse_equal_precedence_right_fixity() {
        let env = MockEnv::new();
        let ops = OpTable::new(vec![
            ("-".to_string(), OpMeta::new(6, Fixity::Right)),
            ("+".to_string(), OpMeta::new(6, Fixity::Right)),
        ]);

        // 1 + (2 - 8)
        let expr = *op(int(1), "+", op(int(2), "-", int(8)));
        let expected = Ok(expr.clone());

        assert_eq!(reparse(expr, &env, &ops), expected);
    }

    #[test]
    fn reparse_mixed_precedences_mixed_fixities() {
        let env = MockEnv::new();
        let ops = OpTable::new(vec![
            ("*".to_string(), OpMeta::new(7, Fixity::Left)),
            ("-".to_string(), OpMeta::new(6, Fixity::Left)),
            ("+".to_string(), OpMeta::new(6, Fixity::Left)),
        ]);

        //  1  + (2  * (6   -  8))
        let expr = *op(int(1), "+", op(int(2), "*", op(int(6), "-", int(8))));
        // (1  + (2  *  6)) -  8
        let expected = Ok(*op(op(int(1), "+", op(int(2), "*", int(6))), "-", int(8)));

        assert_eq!(reparse(expr, &env, &ops), expected);
    }

    #[test]
    fn reparse_equal_precedence_conflicting_fixities() {
        let env = MockEnv::new();
        let ops = OpTable::new(vec![
            ("|>".to_string(), OpMeta::new(5, Fixity::Left)),
            ("<|".to_string(), OpMeta::new(5, Fixity::Right)),
        ]);

        // 1 |> (2 <| 8)
        let expr = *op(int(1), "|>", op(int(2), "<|", int(8)));
        let error = ConflictingFixities(
            ("|>".to_string(), OpMeta::new(5, Fixity::Left)),
            ("<|".to_string(), OpMeta::new(5, Fixity::Right)),
        );
        let expected = Err(no_loc(error));

        assert_eq!(reparse(expr, &env, &ops), expected);
    }

    #[test]
    fn reparse_equal_precedence_conflicting_fixities_nested() {
        let env = MockEnv::new();
        let ops = OpTable::new(vec![
            ("+".to_string(), OpMeta::new(6, Fixity::Left)),
            ("|>".to_string(), OpMeta::new(5, Fixity::Left)),
            ("<|".to_string(), OpMeta::new(5, Fixity::Right)),
        ]);

        // 1 + (1 |> (2 <| 8))
        let expr = *op(int(1), "+", op(int(1), "|>", op(int(2), "<|", int(8))));
        let error = ConflictingFixities(
            ("|>".to_string(), OpMeta::new(5, Fixity::Left)),
            ("<|".to_string(), OpMeta::new(5, Fixity::Right)),
        );
        let expected = Err(no_loc(error));

        assert_eq!(reparse(expr, &env, &ops), expected);
    }
}
