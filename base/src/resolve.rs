use std::borrow::Cow;

use crate::{
    fnv::FnvMap,
    symbol::Symbol,
    types::{AliasData, AliasRef, Type, TypeContext, TypeEnv, TypeExt},
};

quick_error! {
    #[derive(Debug, PartialEq)]
    pub enum Error {
        UndefinedType(id: Symbol) {
            description("undefined type")
            display("Type `{}` does not exist.", id)
        }
        SelfRecursiveAlias(id: Symbol) {
            description("undefined type")
            display("Tried to remove self recursive alias `{}`.", id)
        }
    }
}

#[derive(Debug)]
pub struct AliasRemover<T> {
    reduced_aliases: Vec<Symbol>,
    pub named_variables: FnvMap<Symbol, T>,
}

impl<T> Default for AliasRemover<T> {
    fn default() -> Self {
        AliasRemover {
            reduced_aliases: Default::default(),
            named_variables: Default::default(),
        }
    }
}

impl<T> AliasRemover<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.reduced_aliases.len()
    }

    pub fn is_empty(&self) -> bool {
        self.reduced_aliases.is_empty()
    }

    pub fn reset(&mut self, to: usize) {
        self.reduced_aliases.truncate(to)
    }

    pub fn clear(&mut self) {
        self.reduced_aliases.clear();
        self.named_variables.clear();
    }

    pub fn canonical_alias<'t, F>(
        &mut self,
        env: &dyn TypeEnv<Type = T>,
        interner: &mut impl TypeContext<Symbol, T>,
        typ: &'t T,
        mut canonical: F,
    ) -> Result<Cow<'t, T>, Error>
    where
        F: FnMut(&AliasRef<Symbol, T>) -> bool,
        T: TypeExt<Id = Symbol> + Clone + ::std::fmt::Display,
    {
        Ok(match peek_alias(env, typ) {
            Ok(Some(alias)) => {
                if self.reduced_aliases.contains(&alias.name) {
                    return Err(Error::SelfRecursiveAlias(alias.name.clone()));
                }
                self.reduced_aliases.push(alias.name.clone());

                if canonical(alias) {
                    Cow::Borrowed(typ)
                } else {
                    match alias.typ(interner).apply_args(
                        alias.params(),
                        &typ.unapplied_args(),
                        interner,
                        &mut self.named_variables,
                    ) {
                        Some(typ) => Cow::Owned(
                            self.canonical_alias(env, interner, &typ, canonical)?
                                .into_owned(),
                        ),
                        None => Cow::Borrowed(typ),
                    }
                }
            }
            _ => Cow::Borrowed(typ),
        })
    }

    pub fn remove_aliases_to_concrete<'a>(
        &mut self,
        env: &dyn TypeEnv<Type = T>,
        interner: &mut impl TypeContext<Symbol, T>,
        mut typ: T,
    ) -> Result<T, Error>
    where
        T: TypeExt<Id = Symbol> + ::std::fmt::Display,
    {
        loop {
            typ = match self.remove_alias_to_concrete(env, interner, &typ, |_| true)? {
                Some((typ, args)) => match *typ {
                    Type::Builtin(..)
                    | Type::Function(..)
                    | Type::Function(..)
                    | Type::Record(..)
                    | Type::Variant(..)
                    | Type::Effect(..)
                    | Type::EmptyRow
                    | Type::ExtendRow { .. }
                    | Type::ExtendTypeRow { .. }
                        if args.is_empty() =>
                    {
                        return Ok(typ)
                    }
                    _ => {
                        let typ = typ
                            .replace_generics(interner, &mut self.named_variables)
                            .unwrap_or_else(|| typ);

                        interner.app(typ, args.iter().cloned().collect())
                    }
                },
                None => return Ok(typ),
            };
        }
    }

    pub fn remove_aliases(
        &mut self,
        env: &dyn TypeEnv<Type = T>,
        interner: &mut impl TypeContext<Symbol, T>,
        typ: T,
    ) -> Result<T, Error>
    where
        T: TypeExt<Id = Symbol> + ::std::fmt::Display,
    {
        self.remove_aliases_predicate(env, interner, typ, |_| true)
    }

    pub fn remove_aliases_predicate(
        &mut self,
        env: &dyn TypeEnv<Type = T>,
        interner: &mut impl TypeContext<Symbol, T>,
        mut typ: T,
        mut predicate: impl FnMut(&AliasData<Symbol, T>) -> bool,
    ) -> Result<T, Error>
    where
        T: TypeExt<Id = Symbol> + ::std::fmt::Display,
    {
        loop {
            typ = match self.remove_alias(env, interner, &typ, &mut predicate)? {
                Some(typ) => typ,
                None => return Ok(typ),
            };
        }
    }

    pub fn remove_alias(
        &mut self,
        env: &dyn TypeEnv<Type = T>,
        interner: &mut impl TypeContext<Symbol, T>,
        typ: &T,
        predicate: impl FnOnce(&AliasData<Symbol, T>) -> bool,
    ) -> Result<Option<T>, Error>
    where
        T: TypeExt<Id = Symbol> + ::std::fmt::Display,
    {
        Ok(self
            .remove_alias_to_concrete(env, interner, typ, predicate)?
            .map(|(non_replaced_type, unapplied_args)| {
                let non_replaced_type = non_replaced_type
                    .replace_generics(interner, &mut self.named_variables)
                    .unwrap_or_else(|| non_replaced_type.clone());

                interner.app(non_replaced_type, unapplied_args.iter().cloned().collect())
            }))
    }

    pub fn remove_alias_to_concrete<'a>(
        &mut self,
        env: &'a dyn TypeEnv<Type = T>,
        interner: &mut impl TypeContext<Symbol, T>,
        typ: &'a T,
        predicate: impl FnOnce(&AliasData<Symbol, T>) -> bool,
    ) -> Result<Option<(T, Cow<'a, [T]>)>, Error>
    where
        T: TypeExt<Id = Symbol> + ::std::fmt::Display,
    {
        match peek_alias(env, &typ)? {
            Some(alias) if predicate(alias) => {
                self.remove_alias_to_concrete_inner(interner, typ, alias)
            }
            _ => Ok(None),
        }
    }

    pub fn remove_alias_to_concrete_inner<'a>(
        &mut self,
        interner: &mut impl TypeContext<Symbol, T>,
        typ: &'a T,
        alias: &'a AliasRef<Symbol, T>,
    ) -> Result<Option<(T, Cow<'a, [T]>)>, Error>
    where
        T: TypeExt<Id = Symbol> + ::std::fmt::Display,
    {
        if self.reduced_aliases.iter().any(|name| *name == alias.name) {
            return Err(Error::SelfRecursiveAlias(alias.name.clone()));
        }
        self.reduced_aliases.push(alias.name.clone());
        // Opaque types should only exist as the alias itself
        if let Type::Opaque = **alias.unresolved_type() {
            return Ok(None);
        }

        let unapplied_args = typ.unapplied_args();

        let opt = alias.typ(interner).arg_application(
            alias.params(),
            &unapplied_args,
            interner,
            &mut self.named_variables,
        );
        match opt {
            Some((t, a)) => {
                let l = unapplied_args.len() - a.len();
                Ok(Some((
                    t,
                    match unapplied_args {
                        Cow::Borrowed(slice) => Cow::Borrowed(&slice[l..]),
                        Cow::Owned(mut vec) => {
                            vec.drain(l..);
                            Cow::Owned(vec)
                        }
                    },
                )))
            }
            None => Ok(None),
        }
    }
}

/// Removes type aliases from `typ` until it is an actual type
pub fn remove_aliases<T>(
    env: &dyn TypeEnv<Type = T>,
    interner: &mut impl TypeContext<Symbol, T>,
    mut typ: T,
) -> T
where
    T: TypeExt<Id = Symbol> + ::std::fmt::Display,
{
    while let Ok(Some(new)) = remove_alias(env, interner, &typ) {
        typ = new;
    }
    typ
}

pub fn remove_aliases_cow<'t, T>(
    env: &dyn TypeEnv<Type = T>,

    interner: &mut impl TypeContext<Symbol, T>,
    typ: &'t T,
) -> Cow<'t, T>
where
    T: TypeExt<Id = Symbol> + ::std::fmt::Display,
{
    match remove_alias(env, interner, typ) {
        Ok(Some(typ)) => Cow::Owned(remove_aliases(env, interner, typ)),
        _ => Cow::Borrowed(typ),
    }
}

/// Resolves aliases until `canonical` returns `true` for an alias in which case it returns the
/// type that directly contains that alias
pub fn canonical_alias<'t, F, T>(
    env: &dyn TypeEnv<Type = T>,
    interner: &mut impl TypeContext<Symbol, T>,
    typ: &'t T,
    mut canonical: F,
) -> Cow<'t, T>
where
    F: FnMut(&AliasRef<Symbol, T>) -> bool,
    T: TypeExt<Id = Symbol> + Clone + ::std::fmt::Display,
{
    match peek_alias(env, typ) {
        Ok(Some(alias)) => {
            if canonical(alias) {
                Cow::Borrowed(typ)
            } else {
                alias
                    .typ(interner)
                    .apply_args(
                        alias.params(),
                        &typ.unapplied_args(),
                        interner,
                        &mut Default::default(),
                    )
                    .map(|typ| {
                        Cow::Owned(canonical_alias(env, interner, &typ, canonical).into_owned())
                    })
                    .unwrap_or_else(|| Cow::Borrowed(typ))
            }
        }
        _ => Cow::Borrowed(typ),
    }
}

/// Expand `typ` if it is an alias that can be expanded and return the expanded type.
/// Returns `None` if the type is not an alias or the alias could not be expanded.
pub fn remove_alias<T>(
    env: &dyn TypeEnv<Type = T>,
    interner: &mut impl TypeContext<Symbol, T>,
    typ: &T,
) -> Result<Option<T>, Error>
where
    T: TypeExt<Id = Symbol> + ::std::fmt::Display,
{
    Ok(peek_alias(env, &typ)?.and_then(|alias| {
        // Opaque types should only exist as the alias itself
        if let Type::Opaque = **alias.unresolved_type() {
            return None;
        }
        alias.typ(interner).apply_args(
            alias.params(),
            &typ.unapplied_args(),
            interner,
            &mut Default::default(),
        )
    }))
}

pub fn peek_alias<'t, T>(
    env: &'t dyn TypeEnv<Type = T>,
    typ: &'t T,
) -> Result<Option<&'t AliasRef<Symbol, T>>, Error>
where
    T: TypeExt<Id = Symbol> + ::std::fmt::Display,
{
    let maybe_alias = typ.applied_alias();

    match typ.alias_ident() {
        Some(id) => {
            let alias = match maybe_alias {
                Some(alias) => Some(alias),
                None => env.find_type_info(id).map(|a| &**a),
            };
            Ok(alias)
        }
        None => Ok(None),
    }
}
