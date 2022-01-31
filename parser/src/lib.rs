//! The parser is a bit more complex than it needs to be as it needs to be fully specialized to
//! avoid a recompilation every time a later part of the compiler is changed. Due to this the
//! string interner and therefore also garbage collector needs to compiled before the parser.
#![doc(html_root_url = "https://docs.rs/gluon_parser/0.6.2")] // # GLUON

#[macro_use]
extern crate collect_mac;
extern crate gluon_base as base;
extern crate itertools;
extern crate lalrpop_util;
#[macro_use]
extern crate log;
extern crate pretty;
#[macro_use]
extern crate quick_error;

use std::cell::RefCell;
use std::fmt;

use base::ast::{Comment, Expr, IdentEnv, SpannedExpr, SpannedPattern, TypedIdent, ValueBinding};
use base::error::Errors;
use base::pos::{self, BytePos, Span, Spanned};
use base::symbol::Symbol;
use base::types::{ArcType, TypeCache};

use infix::{OpTable, Reparser};
use layout::Layout;
use token::{Token, Tokenizer};

pub use infix::Error as InfixError;
pub use layout::Error as LayoutError;
pub use token::Error as TokenizeError;

#[cfg_attr(rustfmt, rustfmt_skip)]
mod grammar;
mod infix;
mod layout;
mod token;

fn new_ident<Id>(type_cache: &TypeCache<Id, ArcType<Id>>, name: Id) -> TypedIdent<Id> {
    TypedIdent {
        name: name,
        typ: type_cache.hole(),
    }
}

type LalrpopError<'input> = lalrpop_util::ParseError<
    BytePos,
    Token<'input>,
    Spanned<Error, BytePos>,
>;

/// Shrink hidden spans to fit the visible expressions and flatten singleton blocks.
fn shrink_hidden_spans<Id>(mut expr: SpannedExpr<Id>) -> SpannedExpr<Id> {
    match expr.value {
        Expr::Infix(_, _, ref last)
        | Expr::IfElse(_, _, ref last)
        | Expr::LetBindings(_, ref last)
        | Expr::TypeBindings(_, ref last)
        | Expr::Do(_, _, ref last) => expr.span.end = last.span.end,
        Expr::Lambda(ref lambda) => expr.span.end = lambda.body.span.end,
        Expr::Block(ref mut exprs) => match exprs.len() {
            0 => (),
            1 => return exprs.pop().unwrap(),
            _ => expr.span.end = exprs.last().unwrap().span.end,
        },
        Expr::Match(_, ref alts) => if let Some(last_alt) = alts.last() {
            let end = last_alt.expr.span.end;
            expr.span.end = end;
        },
        Expr::App(_, _)
        | Expr::Ident(_)
        | Expr::Literal(_)
        | Expr::Projection(_, _, _)
        | Expr::Array(_)
        | Expr::Record { .. }
        | Expr::Tuple { .. }
        | Expr::Error(..) => (),
    }
    expr
}

fn transform_errors<'a, Iter>(errors: Iter) -> Errors<Spanned<Error, BytePos>>
where
    Iter: IntoIterator<Item = LalrpopError<'a>>,
{
    errors.into_iter().map(Error::from_lalrpop).collect()
}

struct Expected<'a>(&'a [String]);

impl<'a> fmt::Display for Expected<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.0.len() {
            0 => (),
            1 => write!(f, "\nExpected ")?,
            _ => write!(f, "\nExpected one of ")?,
        }
        for (i, token) in self.0.iter().enumerate() {
            let sep = match i {
                0 => "",
                i if i + 1 < self.0.len() => ",",
                _ => " or",
            };
            write!(f, "{} {}", sep, token)?;
        }
        Ok(())
    }
}

quick_error! {
    #[derive(Debug, PartialEq)]
    pub enum Error {
        Token(err: TokenizeError) {
            description(err.description())
            display("{}", err)
            from()
        }
        Layout(err: LayoutError) {
            description(err.description())
            display("{}", err)
            from()
        }
        InvalidToken {
            description("invalid token")
            display("Invalid token")
        }
        UnexpectedToken(token: String, expected: Vec<String>) {
            description("unexpected token")
            display("Unexpected token: {}{}", token, Expected(&expected))
        }
        UnexpectedEof(expected: Vec<String>) {
            description("unexpected end of file")
            display("Unexpected end of file{}", Expected(&expected))
        }
        ExtraToken(token: String) {
            description("extra token")
            display("Extra token: {}", token)
        }
        Infix(err: InfixError) {
            description(err.description())
            display("{}", err)
            from()
        }
    }
}

/// LALRPOP currently has an unnecessary set of `"` around each expected token
fn remove_extra_quotes(tokens: &mut [String]) {
    for token in tokens {
        if token.starts_with('"') && token.ends_with('"') {
            token.remove(0);
            token.pop();
        }
    }
}

impl Error {
    fn from_lalrpop(err: LalrpopError) -> Spanned<Error, BytePos> {
        use lalrpop_util::ParseError::*;

        match err {
            InvalidToken { location } => pos::spanned2(location, location, Error::InvalidToken),
            UnrecognizedToken {
                token: Some((lpos, token, rpos)),
                mut expected,
            } => {
                remove_extra_quotes(&mut expected);
                pos::spanned2(
                    lpos,
                    rpos,
                    Error::UnexpectedToken(token.to_string(), expected),
                )
            }
            UnrecognizedToken {
                token: None,
                mut expected,
            } => {
                remove_extra_quotes(&mut expected);
                pos::spanned2(0.into(), 0.into(), Error::UnexpectedEof(expected))
            }
            ExtraToken {
                token: (lpos, token, rpos),
            } => pos::spanned2(lpos, rpos, Error::ExtraToken(token.to_string())),
            User { error } => error,
        }
    }
}

/// An iterator which forwards only the `Ok` values. If an `Err` is found the iterator returns
/// `None` and the error can be retrieved using the `result` method.
struct ResultOkIter<I, E> {
    iter: I,
    error: Option<E>,
}

impl<I, E> ResultOkIter<I, E> {
    fn new(iter: I) -> ResultOkIter<I, E> {
        ResultOkIter {
            iter: iter,
            error: None,
        }
    }

    fn result<T>(&mut self, value: T) -> Result<T, E> {
        match self.error.take() {
            Some(err) => Err(err),
            None => Ok(value),
        }
    }
}

impl<I, T, E> Iterator for ResultOkIter<I, E>
where
    I: Iterator<Item = Result<T, E>>,
    E: ::std::fmt::Debug,
{
    type Item = T;

    fn next(&mut self) -> Option<T> {
        match self.iter.next() {
            Some(Ok(t)) => Some(t),
            Some(Err(err)) => {
                self.error = Some(err);
                None
            }
            None => None,
        }
    }
}

/// An iterator which can be shared
struct SharedIter<'a, I: 'a> {
    iter: &'a RefCell<I>,
}

impl<'a, I> Clone for SharedIter<'a, I> {
    fn clone(&self) -> SharedIter<'a, I> {
        SharedIter { iter: self.iter }
    }
}

impl<'a, I> SharedIter<'a, I> {
    fn new(iter: &'a RefCell<I>) -> SharedIter<'a, I> {
        SharedIter { iter: iter }
    }
}

impl<'a, I> Iterator for SharedIter<'a, I>
where
    I: Iterator,
{
    type Item = I::Item;

    fn next(&mut self) -> Option<I::Item> {
        self.iter.borrow_mut().next()
    }
}

pub enum FieldPattern<Id> {
    Type(Spanned<Id, BytePos>, Option<Id>),
    Value(Spanned<Id, BytePos>, Option<SpannedPattern<Id>>),
}

pub enum FieldExpr<Id> {
    Type(Option<Comment>, Spanned<Id, BytePos>, Option<ArcType<Id>>),
    Value(
        Option<Comment>,
        Spanned<Id, BytePos>,
        Option<SpannedExpr<Id>>,
    ),
}

// Hack around LALRPOP's limited type syntax
type MutIdentEnv<'env, Id> = &'env mut IdentEnv<Ident = Id>;
type ErrorEnv<'err, 'input> = &'err mut Errors<LalrpopError<'input>>;

pub type ParseErrors = Errors<Spanned<Error, BytePos>>;

macro_rules! layout {
    ($result_ok_iter: ident, $input: expr) => { {
        let tokenizer = Tokenizer::new($input);
        $result_ok_iter = RefCell::new(ResultOkIter::new(tokenizer));

        Layout::new(SharedIter::new(&$result_ok_iter)).map(|token| {
            // Return the tokenizer error if one exists
            $result_ok_iter.borrow_mut()
                .result(())
                .map_err(|err| {
                    pos::spanned2(err.span.start.absolute,
                                err.span.end.absolute,
                                err.value.into())
                })?;
            let token = token.map_err(|err| pos::spanned(err.span, err.value.into()))?;
            debug!("Lex {:?}", token.value);
            let Span { start, end, .. } = token.span;
            Ok((start.absolute, token.value, end.absolute))
        })
    } }
}

pub fn parse_partial_expr<Id>(
    symbols: &mut IdentEnv<Ident = Id>,
    type_cache: &TypeCache<Id, ArcType<Id>>,
    input: &str,
) -> Result<SpannedExpr<Id>, (Option<SpannedExpr<Id>>, ParseErrors)>
where
    Id: Clone,
{
    let result_ok_iter;
    let layout = layout!(result_ok_iter, input);

    let mut parse_errors = Errors::new();

    let result = grammar::parse_TopExpr(input, type_cache, symbols, &mut parse_errors, layout);

    // If there is a tokenizer error it may still exist in the result iterator wrapper.
    // If that is the case we return that error instead of the unexpected EOF error that lalrpop
    // emitted
    if let Err(err) = result_ok_iter.borrow_mut().result(()) {
        parse_errors.pop(); // Remove the EOF error
        parse_errors.push(lalrpop_util::ParseError::User {
            error: pos::spanned2(
                err.span.start.absolute,
                err.span.end.absolute,
                err.value.into(),
            ),
        });
    }

    match result {
        Ok(mut expr) => {
            let mut errors = transform_errors(parse_errors);
            let mut reparser = Reparser::new(OpTable::default(), symbols);
            if let Err(reparse_errors) = reparser.reparse(&mut expr) {
                errors.extend(reparse_errors.into_iter().map(|err| err.map(Error::Infix)));
            }

            if errors.has_errors() {
                Err((Some(expr), errors))
            } else {
                Ok(expr)
            }
        }
        Err(err) => {
            parse_errors.push(err);
            Err((None, transform_errors(parse_errors)))
        }
    }
}

pub fn parse_expr(
    symbols: &mut IdentEnv<Ident = Symbol>,
    type_cache: &TypeCache<Symbol, ArcType>,
    input: &str,
) -> Result<SpannedExpr<Symbol>, ParseErrors> {
    parse_partial_expr(symbols, type_cache, input).map_err(|t| t.1)
}

pub type LetOrExpr<Id> = Result<SpannedExpr<Id>, ValueBinding<Id>>;

pub fn parse_partial_let_or_expr<Id>(
    symbols: &mut IdentEnv<Ident = Id>,
    input: &str,
) -> Result<LetOrExpr<Id>, (Option<LetOrExpr<Id>>, ParseErrors)>
where
    Id: Clone,
{
    let result_ok_iter;
    let layout = layout!(result_ok_iter, input);

    let mut parse_errors = Errors::new();

    let type_cache = TypeCache::new();

    let result = grammar::parse_LetOrExpr(input, &type_cache, symbols, &mut parse_errors, layout);

    // If there is a tokenizer error it may still exist in the result iterator wrapper.
    // If that is the case we return that error instead of the unexpected EOF error that lalrpop
    // emitted
    if let Err(err) = result_ok_iter.borrow_mut().result(()) {
        parse_errors.pop(); // Remove the EOF error
        parse_errors.push(lalrpop_util::ParseError::User {
            error: pos::spanned2(
                err.span.start.absolute,
                err.span.end.absolute,
                err.value.into(),
            ),
        });
    }

    match result {
        Ok(mut let_or_expr) => {
            let mut errors = transform_errors(parse_errors);
            let mut reparser = Reparser::new(OpTable::default(), symbols);
            let result = match let_or_expr {
                Ok(ref mut expr) => reparser.reparse(expr),
                Err(ref mut let_binding) => reparser.reparse(&mut let_binding.expr),
            };
            if let Err(reparse_errors) = result {
                errors.extend(reparse_errors.into_iter().map(|err| err.map(Error::Infix)));
            }

            if errors.has_errors() {
                Err((Some(let_or_expr), errors))
            } else {
                Ok(let_or_expr)
            }
        }
        Err(err) => {
            parse_errors.push(err);
            Err((None, transform_errors(parse_errors)))
        }
    }
}

#[cfg(feature = "test")]
pub fn parse_string<'env, 'input>(
    symbols: &'env mut IdentEnv<Ident = String>,
    input: &'input str,
) -> Result<SpannedExpr<String>, (Option<SpannedExpr<String>>, ParseErrors)> {
    parse_partial_expr(symbols, &TypeCache::new(), input)
}
