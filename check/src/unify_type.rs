use std::fmt;
use std::mem;

use base::error::Errors;
use base::types::{self, ArcType, Field, Type, TypeVariable, TypeEnv, merge};
use base::symbol::{Symbol, SymbolRef};
use base::instantiate;
use base::scoped_map::ScopedMap;

use unify;
use unify::{Error as UnifyError, Unifier, Unifiable};
use substitution::{Variable, Substitutable, Substitution};

pub type Error<I> = UnifyError<ArcType<I>, TypeError<I>>;

pub struct State<'a> {
    env: &'a (TypeEnv + 'a),
    /// A stack of which aliases are currently expanded. Used to determine when an alias is
    /// recursively expanded in which case the unification fails.
    reduced_aliases: Vec<Symbol>,
    subs: &'a Substitution<ArcType>,
    record_context: Option<(ArcType, ArcType)>,
}

impl<'a> State<'a> {
    pub fn new(env: &'a (TypeEnv + 'a), subs: &'a Substitution<ArcType>) -> State<'a> {
        State {
            env: env,
            reduced_aliases: Vec::new(),
            subs: subs,
            record_context: None,
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum TypeError<I> {
    UndefinedType(I),
    FieldMismatch(I, I),
    SelfRecursive(I),
    UnableToGeneralize(I),
    MissingFields(ArcType<I>, Vec<I>),
}

impl From<instantiate::Error> for Error<Symbol> {
    fn from(error: instantiate::Error) -> Error<Symbol> {
        UnifyError::Other(match error {
            instantiate::Error::UndefinedType(id) => TypeError::UndefinedType(id),
            instantiate::Error::SelfRecursive(id) => TypeError::SelfRecursive(id),
        })
    }
}

impl<I> fmt::Display for TypeError<I>
    where I: fmt::Display + AsRef<str>,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            TypeError::FieldMismatch(ref l, ref r) => {
                write!(f,
                       "Field names in record do not match.\n\tExpected: {}\n\tFound: {}",
                       l,
                       r)
            }
            TypeError::UndefinedType(ref id) => write!(f, "Type `{}` does not exist.", id),
            TypeError::SelfRecursive(ref id) => {
                write!(f,
                       "The use of self recursion in type `{}` could not be unified.",
                       id)
            }
            TypeError::UnableToGeneralize(ref id) => {
                write!(f,
                       "Could not generalize the variable bound to `{}` as the variable was used \
                        outside its scope",
                       id)
            }
            TypeError::MissingFields(ref typ, ref fields) => {
                try!(write!(f, "The type `{}` lacks the following fields: ", typ));
                for (i, field) in fields.iter().enumerate() {
                    let sep = match i {
                        0 => "",
                        i if i < fields.len() - 1 => ", ",
                        _ => " and ",
                    };
                    try!(write!(f, "{}{}", sep, field));

                }
                Ok(())
            }
        }
    }
}

pub type UnifierState<'a, U> = unify::UnifierState<State<'a>, U>;

impl Variable for TypeVariable {
    fn get_id(&self) -> u32 {
        self.id
    }
}

impl<I> Substitutable for ArcType<I> {
    type Variable = TypeVariable;

    fn new(id: u32) -> ArcType<I> {
        Type::variable(TypeVariable::new(id))
    }

    fn from_variable(var: TypeVariable) -> ArcType<I> {
        Type::variable(var)
    }

    fn get_var(&self) -> Option<&TypeVariable> {
        match **self {
            Type::Variable(ref var) => Some(var),
            _ => None,
        }
    }

    fn traverse<F>(&self, f: &mut F)
        where F: types::Walker<ArcType<I>>,
    {
        types::walk_type_(self, f)
    }
}

impl<'a> Unifiable<State<'a>> for ArcType {
    type Error = TypeError<Symbol>;

    fn zip_match<U>(&self,
                    other: &Self,
                    unifier: &mut UnifierState<'a, U>)
                    -> Result<Option<Self>, Error<Symbol>>
        where U: Unifier<State<'a>, Self>,
    {
        let reduced_aliases = unifier.state.reduced_aliases.len();
        debug!("{:?} <=> {:?}", self, other);
        let (l_temp, r_temp);
        let (mut l, mut r) = (self, other);
        let mut through_alias = false;
        match find_common_alias(unifier, self, other, &mut through_alias) {
            Ok((l2, r2)) => {
                l_temp = l2;
                r_temp = r2;
                l = &l_temp;
                r = &r_temp;
            }
            Err(()) => (),
        }
        let result = do_zip_match(l, r, unifier).map(|mut unified_type| {
            // If the match was done through an alias the unified type is likely less precise than
            // `self` or `other`.
            // So just return `None` which means `self` is used as the type if necessary
            if through_alias {
                unified_type.take();
            }
            unified_type
        });
        unifier.state.reduced_aliases.truncate(reduced_aliases);
        result
    }
}

fn do_zip_match<'a, U>(self_: &ArcType,
                       other: &ArcType,
                       unifier: &mut UnifierState<'a, U>)
                       -> Result<Option<ArcType>, Error<Symbol>>
    where U: Unifier<State<'a>, ArcType>,
{
    debug!("Unifying:\n{:?} <=> {:?}", self_, other);
    match (&**self_, &**other) {
        (&Type::App(ref l, ref l_args), &Type::App(ref r, ref r_args)) => {
            use std::cmp::Ordering::*;
            match l_args.len().cmp(&r_args.len()) {
                Equal => {
                    let new_type = unifier.try_match(l, r);
                    let new_args = walk_move_types(l_args.iter().zip(r_args),
                                                   |l, r| unifier.try_match(l, r));
                    Ok(merge(l, new_type, l_args, new_args, Type::app))
                }
                Less => {
                    let offset = r_args.len() - l_args.len();
                    let new_type =
                        unifier.try_match(l, &Type::app(r.clone(), r_args[..offset].into()));
                    let new_args = walk_move_types(l_args.iter().zip(&r_args[offset..]),
                                                   |l, r| unifier.try_match(l, r));
                    Ok(merge(l, new_type, l_args, new_args, Type::app))
                }
                Greater => {
                    let offset = l_args.len() - r_args.len();
                    let new_type =
                        unifier.try_match(&Type::app(l.clone(), l_args[..offset].into()), r);
                    let new_args = walk_move_types(l_args[offset..].iter().zip(r_args),
                                                   |l, r| unifier.try_match(l, r));
                    Ok(merge(r, new_type, r_args, new_args, Type::app))
                }
            }
        }
        (&Type::Record(ref l_row), &Type::Record(ref r_row)) => {
            // Store the current records so that they can be used when displaying field errors
            let previous = mem::replace(&mut unifier.state.record_context,
                                        Some((self_.clone(), other.clone())));
            let result = unifier.try_match(l_row, r_row)
                .map(|row| ArcType::from(Type::Record(row)));
            unifier.state.record_context = previous;
            Ok(result)
        }
        (&Type::ExtendRow { types: ref l_types, fields: ref l_args, rest: ref l_rest },
         &Type::ExtendRow { types: ref r_types, fields: ref r_args, rest: ref r_rest }) => {
            // When the field names of both rows match exactly we special case
            // unification to maximize sharing through `merge` and `walk_move_type`
            if l_args.len() == r_args.len() &&
               l_args.iter().zip(r_args).all(|(l, r)| l.name.name_eq(&r.name)) &&
               l_types == r_types {
                let new_args = walk_move_types(l_args.iter().zip(r_args), |l, r| {
                    unifier.try_match(&l.typ, &r.typ)
                        .map(|typ| {
                            types::Field {
                                name: l.name.clone(),
                                typ: typ,
                            }
                        })
                });
                let new_rest = unifier.try_match(l_rest, r_rest);
                Ok(merge(l_args,
                         new_args,
                         l_rest,
                         new_rest,
                         |fields, rest| Type::extend_row(l_types.clone(), fields, rest)))
            } else if **l_rest == Type::EmptyRow && **r_rest == Type::EmptyRow {
                for l_typ in self_.type_field_iter() {
                    if let None = other.type_field_iter().find(|r_typ| *r_typ == l_typ) {
                        return Err(UnifyError::TypeMismatch(self_.clone(), other.clone()));
                    }
                }

                // HACK For non polymorphic records we need to care about field order as the
                // compiler assumes the order the fields occur in the type determines how
                // to access them
                let new_args = walk_move_types(l_args.iter().zip(r_args.iter()), |l, r| {
                    let opt_type = if !l.name.name_eq(&r.name) {
                        let err = TypeError::FieldMismatch(l.name.clone(), r.name.clone());
                        unifier.report_error(UnifyError::Other(err));
                        None
                    } else {
                        unifier.try_match(&l.typ, &r.typ)
                    };
                    opt_type.map(|typ| {
                        types::Field {
                            name: l.name.clone(),
                            typ: typ,
                        }
                    })
                });
                let new_rest = unifier.try_match(l_rest, r_rest);
                Ok(merge(l_args,
                         new_args,
                         l_rest,
                         new_rest,
                         |fields, rest| Type::extend_row(l_types.clone(), fields, rest)))
            } else {
                unify_rows(unifier, self_, other)
            }
        }
        (&Type::Ident(ref id), &Type::Alias(ref alias)) if *id == alias.name => {
            Ok(Some(other.clone()))
        }
        (&Type::Alias(ref alias), &Type::Ident(ref id)) if *id == alias.name => Ok(None),
        _ => {
            if self_ == other {
                // Successful unification
                return Ok(None);
            } else {
                Ok(try!(try_with_alias(unifier, self_, other)))
            }
        }
    }
}

fn gather_fields<'a, I, J, T>
    (l: I,
     r: J)
     -> (Vec<Field<Symbol, T>>, Vec<(&'a Field<Symbol, T>, &'a Field<Symbol, T>)>, Vec<Field<Symbol, T>>)
    where I: Clone + IntoIterator<Item = &'a Field<Symbol, T>>,
          J: Clone + IntoIterator<Item = &'a Field<Symbol, T>>,
          T: Clone + 'a,
{
    let mut both = Vec::new();
    let mut missing_from_right = Vec::new();
    let mut l_iter = l.clone().into_iter();
    for l in l_iter.by_ref() {
        match r.clone().into_iter().find(|r| l.name.name_eq(&r.name)) {
            Some(r) => both.push((l, r)),
            None => missing_from_right.push(l.clone()),
        }
    }

    let mut r_iter = r.into_iter();
    let missing_from_left: Vec<_> = r_iter.by_ref()
        .filter(|r| l.clone().into_iter().all(|l| !l.name.name_eq(&r.name)))
        .cloned()
        .collect();
    (missing_from_left, both, missing_from_right)
}

/// Do unification between two rows. Each row is either `Type::ExtendRow` or `Type::EmptyRow`.
/// Two rows will unify successfully if all fields they have in common unifies and if either
/// record have additional fields not found in the other record, the other record can be extended.
/// A record can be extended if the `rest` part of `Type::ExtendRow` is a type variable in which
/// case that variable is unified with the missing fields.
fn unify_rows<'a, U>(unifier: &mut UnifierState<'a, U>,
                     l: &ArcType,
                     r: &ArcType)
                     -> Result<Option<ArcType>, Error<Symbol>>
    where U: Unifier<State<'a>, ArcType>,
{
    let subs = unifier.state.subs;
    let (types_missing_from_left, types_both, types_missing_from_right) =
        gather_fields(l.type_field_iter(), r.type_field_iter());

    if !types_both.iter().all(|&(l, r)| l == r) {
        return Err(UnifyError::TypeMismatch(l.clone(), r.clone()));
    }

    let (missing_from_left, both, missing_from_right) = gather_fields(l.row_iter(), r.row_iter());

    let mut types: Vec<_> = types_both.iter().map(|pair| pair.0.clone()).collect();

    // Unify the fields that exists in both records
    let new_both = walk_move_types(both.iter().cloned(), |l, r| {
        unifier.try_match(&l.typ, &r.typ)
            .map(|typ| {
                types::Field {
                    name: l.name.clone(),
                    typ: typ,
                }
            })
    });

    // Pack all fields from both records into a single `Type::ExtendRow` value
    let mut fields = match new_both {
        Some(fields) => fields,
        None => both.iter().map(|pair| pair.0.clone()).collect(),
    };

    // Unify the fields missing from the left and right record with the variable (that hopefully)
    // exists as the 'extension' in the other record
    // Example:
    // `{ x : Int | $0 } <=> `{ y : String | $1 }`
    // `Row (x : Int | Fresh var) <=> $1`
    // `Row (y : String | Fresh var 2) <=> $0`

    // This default `rest` value will only be used on errors, or if both fields has the same fields
    let mut r_iter = r.row_iter();
    for _ in r_iter.by_ref() {
    }
    let mut rest = r_iter.current_type().clone();

    // No need to do anything of no fields are missing
    if !missing_from_right.is_empty() {
        // If we attempt to unify with a non-polymorphic record we intercept the unification to
        // display a better error message
        match *rest {
            Type::EmptyRow => {
                let context = unifier.state.record_context.as_ref().map_or(r, |p| &p.1).clone();
                let err = TypeError::MissingFields(context,
                                                   missing_from_right.into_iter()
                                                       .map(|field| field.name.clone())
                                                       .collect());
                unifier.report_error(UnifyError::Other(err));
            }
            _ => {
                rest = subs.new_var();
                let l_rest =
                    Type::extend_row(types_missing_from_right, missing_from_right, rest.clone());
                unifier.try_match(&l_rest, r_iter.current_type());
                types.extend(l_rest.type_field_iter().cloned());
                fields.extend(l_rest.row_iter().cloned());
            }
        }
    }

    // No need to do anything of no fields are missing
    if !missing_from_left.is_empty() {
        let mut l_iter = l.row_iter();
        for _ in l_iter.by_ref() {
        }

        match **l_iter.current_type() {
            Type::EmptyRow => {
                let context = unifier.state.record_context.as_ref().map_or(l, |p| &p.0).clone();
                let err = TypeError::MissingFields(context,
                                                   missing_from_left.into_iter()
                                                       .map(|field| field.name.clone())
                                                       .collect());
                unifier.report_error(UnifyError::Other(err));
            }
            _ => {
                rest = subs.new_var();
                let r_rest =
                    Type::extend_row(types_missing_from_left, missing_from_left, rest.clone());
                unifier.try_match(&l_iter.current_type(), &r_rest);
                types.extend(r_rest.type_field_iter().cloned());
                fields.extend(r_rest.row_iter().cloned());
            }
        }
    }

    Ok(Some(Type::extend_row(types, fields, rest)))
}

/// Attempt to unify two alias types.
/// To find a possible successful unification we walk through the alias expansions of `l` in an
/// attempt to find that `l` expands to the alias `r_id`
fn find_alias<'a, U>(unifier: &mut UnifierState<'a, U>,
                     l: ArcType,
                     r_id: &SymbolRef)
                     -> Result<Option<ArcType>, ()>
    where U: Unifier<State<'a>, ArcType>,
{
    let reduced_aliases = unifier.state.reduced_aliases.len();
    let result = find_alias_(unifier, l, r_id);
    match result {
        Ok(Some(_)) => (),
        _ => {
            // Remove any alias reductions that were added if no new type is returned
            unifier.state.reduced_aliases.truncate(reduced_aliases);
        }
    }
    result
}

fn find_alias_<'a, U>(unifier: &mut UnifierState<'a, U>,
                      mut l: ArcType,
                      r_id: &SymbolRef)
                      -> Result<Option<ArcType>, ()>
    where U: Unifier<State<'a>, ArcType>,
{
    let mut did_alias = false;
    loop {
        l = match l.name() {
            Some(l_id) => {
                if let Some((l_id, _)) = l.as_alias() {
                    if unifier.state.reduced_aliases.iter().any(|id| id == l_id) {
                        return Err(());
                    }
                }
                debug!("Looking for alias reduction from `{}` to `{}`", l_id, r_id);
                if l_id == r_id {
                    // If the aliases matched before going through an alias there is no need to
                    // return a replacement type
                    return Ok(if did_alias { Some(l.clone()) } else { None });
                }
                did_alias = true;
                match instantiate::maybe_remove_alias(unifier.state.env, &l) {
                    Ok(Some(typ)) => {
                        unifier.state
                            .reduced_aliases
                            .push(l.as_alias().expect("Alias").0.clone());
                        typ
                    }
                    Ok(None) => break,
                    Err(err) => {
                        unifier.report_error(err.into());
                        return Err(());
                    }
                }
            }
            None => break,
        }
    }
    Ok(None)
}

/// Attempt to find a common alias between two types. If the function is successful it returns
/// either the same types that were passed in or two types which have the same alias in their spine
///
/// Example:
/// ```
/// type Test a = | Test a Int
/// type Test2 = Test String
///
/// // find_common_alias(Test2, Test 0) => Ok((Test String, Test 0))
/// // find_common_alias(Float, Test 0) => Ok((Float, Test 0))
/// ```
fn find_common_alias<'a, U>(unifier: &mut UnifierState<'a, U>,
                            expected: &ArcType,
                            actual: &ArcType,
                            through_alias: &mut bool)
                            -> Result<(ArcType, ArcType), ()>
    where U: Unifier<State<'a>, ArcType>,
{
    let mut l = expected.clone();
    if let Some(r_id) = actual.name() {
        l = match try!(find_alias(unifier, l.clone(), r_id)) {
            None => l,
            Some(typ) => {
                *through_alias = true;
                return Ok((typ, actual.clone()));
            }
        };
    }
    let mut r = actual.clone();
    if let Some(l_id) = expected.name() {
        r = match try!(find_alias(unifier, r.clone(), l_id)) {
            None => r,
            Some(typ) => {
                *through_alias = true;
                typ
            }
        };
    }
    Ok((l, r))
}

/// As a last ditch effort attempt to unify the types again by expanding the aliases (if the types
/// are alias types).
fn try_with_alias<'a, U>(unifier: &mut UnifierState<'a, U>,
                         expected: &ArcType,
                         actual: &ArcType)
                         -> Result<Option<ArcType>, Error<Symbol>>
    where U: Unifier<State<'a>, ArcType>,
{
    let l = try!(instantiate::remove_aliases_checked(&mut unifier.state.reduced_aliases,
                                                     unifier.state.env,
                                                     expected));
    let r = try!(instantiate::remove_aliases_checked(&mut unifier.state.reduced_aliases,
                                                     unifier.state.env,
                                                     actual));
    match (&l, &r) {
        (&None, &None) => {
            debug!("Unify error: {} <=> {}", expected, actual);
            Err(UnifyError::TypeMismatch(expected.clone(), actual.clone()))
        }
        _ => {
            let l = l.as_ref().unwrap_or(expected);
            let r = r.as_ref().unwrap_or(actual);
            unifier.try_match(l, r);
            Ok(None)
        }
    }
}

fn walk_move_types<'a, I, F, T>(types: I, mut f: F) -> Option<Vec<T>>
    where I: Iterator<Item = (&'a T, &'a T)>,
          F: FnMut(&'a T, &'a T) -> Option<T>,
          T: Clone + 'a,
{
    let mut out = Vec::new();
    walk_move_types2(types, false, &mut out, &mut f);
    if out.is_empty() {
        None
    } else {
        out.reverse();
        Some(out)
    }
}
fn walk_move_types2<'a, I, F, T>(mut types: I, replaced: bool, output: &mut Vec<T>, f: &mut F)
    where I: Iterator<Item = (&'a T, &'a T)>,
          F: FnMut(&'a T, &'a T) -> Option<T>,
          T: Clone + 'a,
{
    if let Some((l, r)) = types.next() {
        let new = f(l, r);
        walk_move_types2(types, replaced || new.is_some(), output, f);
        match new {
            Some(typ) => {
                output.push(typ);
            }
            None if replaced || !output.is_empty() => {
                output.push(l.clone());
            }
            None => (),
        }
    }
}

pub fn merge_signature(subs: &Substitution<ArcType>,
                       variables: &mut ScopedMap<Symbol, ArcType>,
                       level: u32,
                       state: State,
                       l: &ArcType,
                       r: &ArcType)
                       -> Result<ArcType, Errors<Error<Symbol>>> {
    let mut unifier = UnifierState {
        state: state,
        unifier: Merge {
            subs: subs,
            variables: variables,
            errors: Errors::new(),
            level: level,
        },
    };

    let typ = unifier.try_match(l, r);
    if unifier.unifier.errors.has_errors() {
        Err(unifier.unifier.errors)
    } else {
        Ok(typ.unwrap_or_else(|| l.clone()))
    }
}

struct Merge<'e> {
    subs: &'e Substitution<ArcType>,
    variables: &'e ScopedMap<Symbol, ArcType>,
    errors: Errors<Error<Symbol>>,
    level: u32,
}

impl<'a, 'e> Unifier<State<'a>, ArcType> for Merge<'e> {
    fn report_error(unifier: &mut UnifierState<Self>,
                    error: UnifyError<ArcType, TypeError<Symbol>>) {
        unifier.unifier.errors.error(error);
    }

    fn try_match(unifier: &mut UnifierState<Self>, l: &ArcType, r: &ArcType) -> Option<ArcType> {
        let subs = unifier.unifier.subs;
        // Retrieve the 'real' types by resolving
        let l = subs.real(l);
        let r = subs.real(r);
        // `l` and `r` must have the same type, if one is a variable that variable is
        // unified with whatever the other type is
        let result = match (&**l, &**r) {
            (&Type::Variable(ref l), &Type::Variable(ref r)) if l.id == r.id => Ok(None),
            (&Type::Generic(ref l_gen), &Type::Variable(ref r_var)) => {
                let left = match unifier.unifier.variables.get(&l_gen.id) {
                    Some(generic_bound_var) => {
                        match **generic_bound_var {
                            // The generic variable is defined outside the current scope. Use the
                            // type variable instantiated from the generic and unify with that
                            Type::Variable(ref var) if var.id < unifier.unifier.level => {
                                generic_bound_var
                            }
                            // `r_var` is outside the scope of the generic variable.
                            Type::Variable(ref var) if var.id > r_var.id => {
                                let error = UnifyError::Other(TypeError::UnableToGeneralize(l_gen.id
                                    .clone()));
                                unifier.unifier.errors.error(error);
                                return None;
                            }
                            _ => l,
                        }
                    }
                    None => l,
                };
                match subs.union(r_var, left) {
                    Ok(()) => Ok(None),
                    Err(()) => Err(UnifyError::Occurs(r_var.clone(), left.clone())),
                }

            }
            (_, &Type::Variable(ref r)) => {
                match subs.union(r, l) {
                    Ok(()) => Ok(None),
                    Err(()) => Err(UnifyError::Occurs(r.clone(), l.clone())),
                }
            }
            (&Type::Variable(ref l), _) => {
                match subs.union(l, r) {
                    Ok(()) => Ok(Some(r.clone())),
                    Err(()) => Err(UnifyError::Occurs(l.clone(), r.clone())),
                }
            }
            _ => {
                // Both sides are concrete types, the only way they can be equal is if
                // the matcher finds their top level to be equal (and their sub-terms
                // unify)
                l.zip_match(r, unifier)
            }
        };
        match result {
            Ok(typ) => typ,
            Err(error) => {
                unifier.unifier.errors.error(error);
                Some(subs.new_var())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base::error::Errors;

    use unify::Error::*;
    use unify::unify;
    use substitution::Substitution;
    use base::types::{self, ArcType, Type};
    use tests::*;

    #[test]
    fn detect_multiple_type_errors_in_single_type() {
        let _ = ::env_logger::init();
        let (x, y) = (intern("x"), intern("y"));
        let l: ArcType = Type::record(vec![],
                                      vec![types::Field {
                                               name: x.clone(),
                                               typ: Type::int(),
                                           },
                                           types::Field {
                                               name: y.clone(),
                                               typ: Type::string(),
                                           }]);
        let r = Type::record(vec![],
                             vec![types::Field {
                                      name: x.clone(),
                                      typ: Type::string(),
                                  },
                                  types::Field {
                                      name: y.clone(),
                                      typ: Type::int(),
                                  }]);
        let subs = Substitution::new();
        let env = MockEnv;
        let state = State::new(&env, &subs);
        let result = unify(&subs, state, &l, &r);
        assert_eq!(result,
                   Err(Errors {
                       errors: vec![TypeMismatch(Type::int(), Type::string()),
                                    TypeMismatch(Type::string(), Type::int())],
                   }));
    }

    #[test]
    fn unify_row_polymorphism() {
        let _ = ::env_logger::init();
        let x = types::Field {
            name: intern("x"),
            typ: Type::int(),
        };
        let y = types::Field {
            name: intern("y"),
            typ: Type::int(),
        };
        let subs = Substitution::new();
        let l: ArcType = Type::poly_record(vec![], vec![x.clone()], subs.new_var());
        let r = Type::poly_record(vec![], vec![y.clone()], subs.new_var());

        let env = MockEnv;
        let state = State::new(&env, &subs);
        let result = unify(&subs, state, &l, &r);
        match result {
            Ok(result) => {
                // Get the row variable at the end of the resulting type so we can compare the types
                let mut iter = result.row_iter();
                for _ in iter.by_ref() {
                }
                let row_variable = iter.current_type().clone();
                let expected = Type::poly_record(vec![], vec![x.clone(), y.clone()], row_variable);
                assert_eq!(result, expected);
            }
            Err(err) => panic!("{}", err),
        }
    }
}
