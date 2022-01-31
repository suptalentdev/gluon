use std::fmt;

use codespan_reporting::Diagnostic;

use pretty::Arena;

use base::{
    ast,
    error::AsDiagnostic,
    pos::{self, BytePos, Spanned},
    types::{ArcType, Filter, ToDoc, TypeExt, TypeFormatter},
};

use crate::{
    implicits,
    kindcheck::{self, Error as KindCheckError, KindError},
    unify::Error as UnifyError,
    unify_type::{self, Error as UnifyTypeError},
};

/// Type representing a single error when checking a type
#[derive(Debug, PartialEq, Functor)]
pub enum TypeError<I, T> {
    /// Variable has not been defined before it was used
    UndefinedVariable(I),
    /// Attempt to call a type which is not a function
    NotAFunction(T),
    /// Type has not been defined before it was used
    UndefinedType(I),
    /// Type were expected to have a certain field
    UndefinedField(T, I),
    /// Constructor type was found in a pattern but did not have the expected number of arguments
    PatternError(T, usize),
    /// Errors found when trying to unify two types
    Unification(T, T, Vec<UnifyTypeError<I, T>>),
    /// Error were found when trying to unify the kinds of two types
    KindError(KindCheckError<I, T>),
    /// Error were found when checking value recursion
    RecursionCheck(crate::recursion_check::Error),
    /// Multiple types were declared with the same name in the same expression
    DuplicateTypeDefinition(I),
    /// A field was defined more than once in a record constructor or pattern match
    DuplicateField(String),
    /// Type is not a type which has any fields
    InvalidProjection(T),
    /// Expected to find a record with the following fields
    UndefinedRecord {
        fields: Vec<I>,
    },
    /// Found a case expression without any alternatives
    EmptyCase,
    Message(String),
    UnableToResolveImplicit(implicits::Error<T>),
}

impl<I, T> From<KindCheckError<I, T>> for TypeError<I, T> {
    fn from(e: KindCheckError<I, T>) -> Self {
        match e {
            UnifyError::Other(KindError::UndefinedType(name)) => TypeError::UndefinedType(name),
            UnifyError::Other(KindError::UndefinedField(typ, name)) => {
                TypeError::UndefinedField(typ, name)
            }
            e => TypeError::KindError(e),
        }
    }
}

impl<I, T> From<implicits::Error<T>> for TypeError<I, T> {
    fn from(e: implicits::Error<T>) -> Self {
        TypeError::UnableToResolveImplicit(e)
    }
}

impl<I, T> From<crate::recursion_check::Error> for TypeError<I, T> {
    fn from(e: crate::recursion_check::Error) -> Self {
        TypeError::RecursionCheck(e)
    }
}

impl<I, T> fmt::Display for TypeError<I, T>
where
    I: fmt::Display + AsRef<str> + Clone,
    T: TypeExt<Id = I>
        + fmt::Display
        + ast::Commented
        + pos::HasSpan
        + for<'a> ToDoc<'a, Arena<'a, ()>, (), ()>,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use self::TypeError::*;
        use pretty::{Arena, DocAllocator};
        match *self {
            UndefinedVariable(ref name) => write!(f, "Undefined variable `{}`", name),
            NotAFunction(ref typ) => write!(f, "`{}` is not a function", typ),
            UndefinedType(ref name) => write!(f, "Type `{}` is not defined", name),
            UndefinedField(ref typ, ref field) => {
                let fields = [field.clone()];
                let filter = unify_type::similarity_filter(typ, &fields);
                let arena = Arena::<()>::new();
                write!(
                    f,
                    "Type `{}` does not have the field `{}`",
                    TypeFormatter::new(typ)
                        .filter(&*filter)
                        .pretty(&arena)
                        .1
                        .pretty(80),
                    field
                )?;
                Ok(())
            }
            Unification(ref expected, ref actual, ref errors) => {
                let filters = errors
                    .iter()
                    .filter_map(|err| match *err {
                        UnifyError::Other(ref err) => Some(err.make_filter()),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                let filter = move |field: &I| {
                    if filters.is_empty() {
                        Filter::Retain
                    } else {
                        filters
                            .iter()
                            .fold(Filter::Drop, move |filter, f| match filter {
                                Filter::Retain => filter,
                                _ => match f(field) {
                                    Filter::Drop => filter,
                                    Filter::RetainKey => Filter::RetainKey,
                                    Filter::Retain => Filter::Retain,
                                },
                            })
                    }
                };

                let arena = Arena::<()>::new();
                let types = chain![&arena;
                    "Expected:",
                    chain![&arena;
                        arena.space(),
                        TypeFormatter::new(expected).filter(&filter).pretty(&arena)
                    ].nest(4).group(),
                    arena.newline(),
                    "Found:",
                    chain![&arena;
                        arena.space(),
                        TypeFormatter::new(actual).filter(&filter).pretty(&arena)
                    ].nest(4).group()
                ]
                .group();
                let doc = chain![&arena;
                    "Expected the following types to be equal",
                    arena.newline(),
                    types,
                    arena.newline(),
                    arena.as_string(errors.len()),
                    " errors were found during unification:"
                ];
                writeln!(f, "{}", doc.1.pretty(80))?;
                if errors.is_empty() {
                    return Ok(());
                }
                for error in &errors[..errors.len() - 1] {
                    match *error {
                        UnifyError::Other(ref err) => {
                            err.filter_fmt(&filter, f)?;
                            writeln!(f)?;
                        }
                        _ => writeln!(f, "{}", error)?,
                    }
                }
                write!(f, "{}", errors.last().unwrap())
            }
            PatternError(ref typ, expected_len) => {
                write!(f, "Type {} has {} to few arguments", typ, expected_len)
            }
            KindError(ref err) => kindcheck::fmt_kind_error(err, f),
            RecursionCheck(ref err) => write!(f, "{}", err),
            DuplicateTypeDefinition(ref id) => write!(
                f,
                "Type '{}' has been already been defined in this module",
                id
            ),
            DuplicateField(ref id) => {
                write!(f, "The record has more than one field named '{}'", id)
            }
            InvalidProjection(ref typ) => write!(
                f,
                "Type '{}' is not a type which allows field accesses",
                typ
            ),
            UndefinedRecord { ref fields } => {
                write!(f, "No type found with the following fields: ")?;
                write!(f, "{}", fields[0])?;
                for field in &fields[1..] {
                    write!(f, ", {}", field)?;
                }
                Ok(())
            }
            EmptyCase => write!(f, "`case` expression with no alternatives"),
            Message(ref msg) => write!(f, "{}", msg),
            UnableToResolveImplicit(ref err) => write!(f, "{}", err),
        }
    }
}

impl<I, T> AsDiagnostic for TypeError<I, T>
where
    I: fmt::Display + AsRef<str> + Clone,
    T: TypeExt<Id = I>
        + fmt::Display
        + ast::Commented
        + pos::HasSpan
        + for<'a> ToDoc<'a, Arena<'a, ()>, (), ()>,
{
    fn as_diagnostic(&self) -> Diagnostic {
        use self::TypeError::*;
        match *self {
            UnableToResolveImplicit(ref err) => err.as_diagnostic(),
            _ => Diagnostic::new_error(self.to_string()),
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum Help {
    UndefinedFlatMapInDo,
    ExtraArgument(u32, u32),
}

impl fmt::Display for Help {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Help::UndefinedFlatMapInDo => write!(
                f,
                "Try bringing the `flat_map` function found in the `Monad`\
                 instance for your type into scope"
            ),
            Help::ExtraArgument(expected, actual) => {
                if expected == 0 {
                    write!(f, "Attempted to call a non-function value")
                } else {
                    write!(
                        f,
                        "Attempted to call function with {} argument{} but its type only has {}",
                        actual,
                        if actual == 1 { "" } else { "s" },
                        expected,
                    )
                }
            }
        }
    }
}

pub type HelpError<Id, T = ArcType<Id>> = crate::base::error::Help<TypeError<Id, T>, Help>;
pub type SpannedTypeError<Id, T = ArcType<Id>> = Spanned<HelpError<Id, T>, BytePos>;
