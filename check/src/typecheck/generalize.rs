use crate::base::{
    ast::{self, MutVisitor, SpannedExpr, SpannedIdent},
    fnv::{FnvMap, FnvSet},
    pos::{BytePos, Span},
    symbol::Symbol,
    types::{self, AppVec, ArcType, BuiltinType, Generic, Type},
};

use crate::{
    substitution::{is_variable_unified, Substitution},
    typecheck::{TypeError, Typecheck},
};

pub(crate) struct TypeGeneralizer<'a, 'b: 'a> {
    level: u32,
    unbound_variables: FnvMap<Symbol, Generic<Symbol>>,
    variable_generator: TypeVariableGenerator,
    tc: &'a mut Typecheck<'b>,
    context: ArcType,
    span: Span<BytePos>,
    in_generic_replace: bool,
}

impl<'a, 'b> ::std::ops::Deref for TypeGeneralizer<'a, 'b> {
    type Target = Typecheck<'b>;
    fn deref(&self) -> &Typecheck<'b> {
        self.tc
    }
}

impl<'a, 'b> ::std::ops::DerefMut for TypeGeneralizer<'a, 'b> {
    fn deref_mut(&mut self) -> &mut Typecheck<'b> {
        self.tc
    }
}

impl<'a, 'b> TypeGeneralizer<'a, 'b> {
    pub(crate) fn new(
        level: u32,
        tc: &'a mut Typecheck<'b>,
        typ: &ArcType,
        span: Span<BytePos>,
    ) -> TypeGeneralizer<'a, 'b> {
        TypeGeneralizer {
            level,
            unbound_variables: FnvMap::default(),
            variable_generator: TypeVariableGenerator::new(&tc.subs, typ),
            tc,
            context: typ.clone(),
            span,
            in_generic_replace: false,
        }
    }

    /// Generalizing updates all variables which are above `level` into "generic variables". A
    /// generic variable is instantiated with a fresh type variable each time it is refered to.
    ///
    /// Generalzing is a crucial part when inferring types as types will otherwise be less general
    /// than they need to. Consider the following expression.
    /// ```f#
    /// let id x = x
    /// in id 2
    /// ```
    /// If the variable in `id` was not generalized before the `id 2` call the variable would be
    /// unified to an `Int` which is not what we want. Generalazing before checking the body of the
    /// `let` basically infers that the variables in `id` does not refer to anything outside the
    /// `let` scope and can thus be "generalized" into `a -> a` which is instantiated with a fresh
    /// type variable in the `id 2` call.
    pub(crate) fn generalize_variables<'i>(
        &mut self,
        args: &mut Iterator<Item = &'i mut SpannedIdent<Symbol>>,
        expr: &mut SpannedExpr<Symbol>,
    ) {
        self.tc.type_variables.enter_scope();

        // Replaces all type variables with their inferred types
        struct ReplaceVisitor<'a: 'c, 'b: 'a, 'c> {
            generalizer: &'c mut TypeGeneralizer<'a, 'b>,
        }

        impl<'a, 'b, 'c, 'd> MutVisitor<'d> for ReplaceVisitor<'a, 'b, 'c> {
            type Ident = Symbol;

            fn visit_expr(&mut self, e: &'d mut SpannedExpr<Self::Ident>) {
                self.generalizer.tc.type_variables.enter_scope();
                self.generalizer.span = e.span;
                ast::walk_mut_expr(self, e);
                self.generalizer.tc.type_variables.exit_scope();
            }

            fn visit_spanned_typed_ident(&mut self, id: &mut SpannedIdent<Symbol>) {
                self.generalizer.span = id.span;
                self.visit_ident(&mut id.value)
            }
            fn visit_typ(&mut self, typ: &mut ArcType) {
                {
                    let type_cache = &self.generalizer.tc.type_cache;
                    self.generalizer.tc.type_variables.extend(
                        typ.forall_params()
                            .map(|param| (param.id.clone(), type_cache.hole())),
                    );
                }
                if let Some(finished) = self.generalizer.generalize_type(typ) {
                    *typ = finished;
                }
            }
        }

        {
            let mut visitor = ReplaceVisitor { generalizer: self };
            visitor.visit_expr(expr);
            for arg in &mut *args {
                visitor.visit_typ(&mut arg.value.typ)
            }
        }

        self.tc.type_variables.exit_scope();
    }

    pub(crate) fn generalize_type_top(&mut self, typ: &mut ArcType) {
        self.tc.type_variables.enter_scope();

        let mut result_type = self.generalize_type(typ);

        self.tc.type_variables.exit_scope();

        if result_type.is_none() && !self.unbound_variables.is_empty() {
            result_type = Some(typ.clone());
        }

        result_type = result_type.map(|typ| {
            let mut params = self
                .unbound_variables
                .drain()
                .map(|(_, generic)| generic)
                .collect::<Vec<_>>();
            params.sort_unstable_by(|l, r| l.id.declared_name().cmp(r.id.declared_name()));

            Type::forall(params, typ)
        });
        if let Some(finished) = result_type {
            *typ = finished;
        }
        debug!("End generalize {}", typ);
    }

    pub(crate) fn generalize_type(&mut self, typ: &ArcType) -> Option<ArcType> {
        let replacement = self.subs.replace_variable(typ);
        let mut typ = typ;
        if let Some(ref t) = replacement {
            typ = t;
        }
        trace!("GEN {}", typ);
        match **typ {
            Type::Variable(ref var) if self.subs.get_level(var.id) >= self.level => {
                // Create a prefix if none exists
                let id = self.variable_generator.next_variable(self.tc);

                let gen: ArcType = Type::generic(Generic::new(id.clone(), var.kind.clone()));
                debug!("Gen {} to {}", var.id, gen);
                self.subs.insert(var.id, gen.clone());

                self.unbound_variables
                    .insert(id.clone(), Generic::new(id, var.kind.clone()));

                Some(gen)
            }

            Type::Forall(ref params, ref typ, Some(ref vars)) => {
                trace!("Generalize `{}` {:?}", typ, vars);

                self.type_variables.enter_scope();
                self.type_variables.extend(
                    params
                        .iter()
                        .zip(vars)
                        .map(|(param, var)| (param.id.clone(), var.clone())),
                );

                trace!("Generalize forall => `{}`", typ);

                let new_type = self.generalize_type(&typ);
                self.type_variables.exit_scope();

                Some(Type::forall(
                    params
                        .iter()
                        .zip(vars)
                        .filter(|&(_, var)| !is_variable_unified(&self.subs, var))
                        .map(|(param, _)| param.clone())
                        .collect(),
                    new_type.unwrap_or_else(|| typ.clone()),
                ))
            }

            Type::Skolem(ref skolem) => {
                let in_scope = self.type_variables.contains_key(&skolem.name);
                if !in_scope {
                    self.tc.error(
                        self.span,
                        TypeError::Message(format!(
                            "Skolem variable `{}` would escape as it is not bound in `{}`",
                            skolem.name, self.context
                        )),
                    );
                }
                if self.subs.get_level(skolem.id) >= self.level {
                    let generic = Generic {
                        id: skolem.name.clone(),
                        kind: skolem.kind.clone(),
                    };

                    if !in_scope {
                        self.unbound_variables
                            .insert(generic.id.clone(), generic.clone());
                    }

                    Some(Type::generic(generic))
                } else {
                    replacement
                }
            }

            Type::Generic(ref generic) => match self.type_variables.get(&generic.id).cloned() {
                Some(var) => {
                    trace!("GEN2 {} => {}", generic.id, var);
                    if !self.in_generic_replace {
                        self.in_generic_replace = true;
                        let new = self.generalize_type(&var).or(replacement);
                        self.in_generic_replace = false;
                        new
                    } else {
                        replacement
                    }
                }
                None => {
                    self.unbound_variables
                        .insert(generic.id.clone(), generic.clone());
                    replacement
                }
            },

            _ => {
                if let Type::Forall(ref params, _, None) = **typ {
                    let type_cache = &self.tc.type_cache;
                    self.tc.type_variables.extend(
                        params
                            .iter()
                            .map(|param| (param.id.clone(), type_cache.hole())),
                    );
                }

                types::walk_move_type_opt(
                    typ,
                    &mut types::ControlVisitation(|typ: &ArcType| self.generalize_type(typ)),
                )
                .map(|t| unroll_typ(&t).unwrap_or(t))
                .or_else(|| replacement.clone())
            }
        }
    }
}

struct TypeVariableGenerator {
    map: FnvSet<Symbol>,
    name: String,
    i: u32,
}

impl TypeVariableGenerator {
    fn new(subs: &Substitution<ArcType>, typ: &ArcType) -> TypeVariableGenerator {
        fn gather_foralls(map: &mut FnvSet<Symbol>, subs: &Substitution<ArcType>, typ: &ArcType) {
            let typ = subs.real(typ);
            if let Type::Forall(ref params, _, _) = **typ {
                map.extend(params.iter().map(|param| param.id.clone()));
            }
            types::walk_move_type_opt(
                typ,
                &mut types::ControlVisitation(|typ: &ArcType| {
                    gather_foralls(map, subs, typ);
                    None
                }),
            );
        }
        let mut map = FnvSet::default();
        gather_foralls(&mut map, subs, typ);
        TypeVariableGenerator {
            map,
            name: "".to_string(),
            i: 0,
        }
    }
    /// Generate a generic variable name which is not used in the current scope
    fn next_variable(&mut self, tc: &mut Typecheck) -> Symbol {
        let symbol = if self.name.is_empty() {
            self.next_variable_(tc)
        } else {
            let name = format!("{}{}", self.name, self.i);
            self.i += 1;
            tc.symbols.symbol(&name[..])
        };
        self.map.insert(symbol.clone());
        tc.type_variables.insert(symbol.clone(), Type::hole());
        symbol
    }

    fn next_variable_(&mut self, tc: &mut Typecheck) -> Symbol {
        for c in b'a'..(b'z' + 1) {
            self.name.push(c as char);
            let symbol = tc.symbols.symbol(&self.name[..]);
            if !self.map.contains(&symbol) && tc.type_variables.get(&symbol).is_none() {
                return symbol;
            }
            self.name.pop();
        }
        self.name.push('a');
        self.next_variable_(tc)
    }
}

fn unroll_typ(typ: &ArcType) -> Option<ArcType> {
    let mut args = AppVec::new();
    let mut current = match **typ {
        Type::App(ref l, ref rest) => {
            // No need to unroll if `l` is not an application as that will just result in returning
            // an application that is identical to `typ`
            match **l {
                Type::App(..) => (),
                _ => return None,
            }
            args.extend(rest.iter().rev().cloned());
            l
        }
        _ => return unroll_record(typ),
    };
    while let Type::App(ref l, ref rest) = **current {
        args.extend(rest.iter().rev().cloned());
        current = l;
    }
    if args.is_empty() {
        None
    } else {
        args.reverse();
        match **current {
            Type::Builtin(BuiltinType::Function) if args.len() == 2 => {
                let ret = args.pop().unwrap();
                Some(Type::function(args.into_iter().collect(), ret))
            }
            _ => Some(Type::app(current.clone(), args)),
        }
    }
}

fn unroll_record(typ: &Type<Symbol>) -> Option<ArcType> {
    let mut new_types = Vec::new();
    let mut new_fields = Vec::new();
    let mut current = match *typ {
        Type::ExtendRow {
            ref types,
            ref fields,
            ref rest,
        } => match **rest {
            Type::ExtendRow { .. } => {
                new_types.extend_from_slice(types);
                new_fields.extend_from_slice(fields);
                rest
            }
            _ => return None,
        },
        _ => return None,
    };
    while let Type::ExtendRow {
        ref types,
        ref fields,
        ref rest,
    } = **current
    {
        new_types.extend_from_slice(types);
        new_fields.extend_from_slice(fields);
        current = rest;
    }
    if new_types.is_empty() && new_fields.is_empty() {
        None
    } else {
        Some(Type::extend_row(new_types, new_fields, current.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unroll_typ_test() {
        let i: ArcType = Type::int();
        let s: ArcType = Type::string();
        assert_eq!(
            unroll_typ(&Type::app(
                Type::app(i.clone(), collect![s.clone()]),
                collect![i.clone()]
            )),
            Some(Type::app(i.clone(), collect![s.clone(), i.clone()]))
        );
        assert_eq!(
            unroll_typ(&Type::app(
                Type::app(i.clone(), collect![i.clone()]),
                collect![s.clone()]
            )),
            Some(Type::app(i.clone(), collect![i.clone(), s.clone()]))
        );
        let f: ArcType = Type::builtin(BuiltinType::Function);
        assert_eq!(
            unroll_typ(&Type::app(
                Type::app(f.clone(), collect![i.clone()]),
                collect![s.clone()]
            )),
            Some(Type::function(collect![i.clone()], s.clone()))
        );
    }
}
