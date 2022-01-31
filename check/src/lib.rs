//! The `check` crate is responsible for ensuring that an AST expression is actually a valid
//! program. This currently consits of three larger parts, typechecking, kindchecking and renaming.
//! If an AST passes the checks in `Typecheck::typecheck_expr` (which runs all of theses checks
//! the expression is expected to compile succesfully (if it does not it should be considered an
//! internal compiler error.
#![doc(html_root_url = "https://docs.rs/gluon_check/0.10.1")] // # GLUON

extern crate codespan;
extern crate codespan_reporting;
#[macro_use]
extern crate collect_mac;
#[cfg(test)]
extern crate env_logger;
extern crate itertools;
#[macro_use]
extern crate log;
extern crate pretty;
extern crate rpds;
extern crate smallvec;
extern crate stable_deref_trait;
extern crate strsim;
extern crate union_find;

#[macro_use]
extern crate gluon_base as base;
#[macro_use]
extern crate gluon_codegen;

pub mod kindcheck;
pub mod metadata;
mod recursion_check;
pub mod rename;
pub mod substitution;
mod typ;
pub mod typecheck;
pub mod unify;
pub mod unify_type;

mod implicits;

use crate::base::{
    fixed::FixedMap,
    kind::{ArcKind, KindEnv},
    metadata::{Metadata, MetadataEnv},
    symbol::{Symbol, SymbolRef},
    types::{
        translate_alias, translate_type, Alias, ArcType, PrimitiveEnv, TypeCache, TypeEnv, TypeExt,
    },
};

use crate::typ::RcType;

/// Checks if `actual` can be assigned to a binding with the type signature `signature`
pub fn check_signature(
    env: &TypecheckEnv<Type = ArcType>,
    signature: &ArcType,
    actual: &ArcType,
) -> bool {
    let env = ArcTypeCacher::new(env);
    let signature = translate_type(&env.type_cache, signature);
    let actual = translate_type(&env.type_cache, actual);
    check_signature_(&env, &signature, &actual)
}

fn check_signature_(env: &TypeEnv<Type = RcType>, signature: &RcType, actual: &RcType) -> bool {
    use crate::base::{fnv::FnvMap, kind::Kind};

    use crate::substitution::Substitution;

    let subs = Substitution::new(Kind::typ());
    let type_cache = TypeCache::new();
    let state = unify_type::State::new(env, &subs, &type_cache);
    let actual = unify_type::new_skolem_scope(&subs, actual);
    let actual = actual.instantiate_generics(&mut FnvMap::default());
    let result = unify_type::subsumes(&subs, state, signature, &actual);
    if let Err((_, ref err)) = result {
        warn!("Check signature error: {}", err);
    }
    result.is_ok()
}

pub trait TypecheckEnv: PrimitiveEnv + MetadataEnv {}

impl<T> TypecheckEnv for T where T: PrimitiveEnv + MetadataEnv {}

struct ArcTypeCacher<'a> {
    environment: &'a (TypecheckEnv<Type = ArcType> + 'a),
    types: FixedMap<String, Box<RcType>>,
    aliases: FixedMap<String, Box<Alias<Symbol, RcType>>>,
    type_cache: TypeCache<Symbol, RcType>,
}

impl<'a> ArcTypeCacher<'a> {
    fn new(environment: &'a (TypecheckEnv<Type = ArcType> + 'a)) -> ArcTypeCacher<'a> {
        ArcTypeCacher {
            environment,
            types: Default::default(),
            aliases: Default::default(),
            type_cache: Default::default(),
        }
    }
}

impl<'a> KindEnv for ArcTypeCacher<'a> {
    fn find_kind(&self, type_name: &SymbolRef) -> Option<ArcKind> {
        if let Some(k) = self.aliases.get(type_name.as_str()) {
            return Some(k.kind().into_owned());
        }
        self.environment.find_type_info(type_name).map(|alias| {
            let rc_alias = Alias::from(translate_alias(alias, |t| {
                translate_type(&self.type_cache, t)
            }));
            self.aliases
                .try_insert(type_name.as_str().into(), Box::new(rc_alias.clone()))
                .unwrap();
            self.find_kind(type_name).unwrap()
        })
    }
}

impl<'a> TypeEnv for ArcTypeCacher<'a> {
    type Type = RcType;
    fn find_type(&self, id: &SymbolRef) -> Option<&RcType> {
        if let Some(t) = self.types.get(id.as_str()) {
            return Some(t);
        }
        self.environment.find_type(id).map(|arc_type| {
            let rc_type = translate_type(&self.type_cache, arc_type);
            self.types
                .try_insert(id.as_str().into(), Box::new(rc_type.clone()))
                .unwrap();
            self.find_type(id).unwrap()
        })
    }

    fn find_type_info(&self, id: &SymbolRef) -> Option<&Alias<Symbol, RcType>> {
        if let Some(t) = self.aliases.get(id.as_str()) {
            return Some(t);
        }
        self.environment.find_type_info(id).map(|alias| {
            let rc_alias = Alias::from(translate_alias(alias, |t| {
                translate_type(&self.type_cache, t)
            }));
            self.aliases
                .try_insert(id.as_str().into(), Box::new(rc_alias.clone()))
                .unwrap();
            self.find_type_info(id).unwrap()
        })
    }
}

impl<'a> PrimitiveEnv for ArcTypeCacher<'a> {
    fn get_bool(&self) -> &ArcType {
        self.environment.get_bool()
    }
}

impl<'a> MetadataEnv for ArcTypeCacher<'a> {
    fn get_metadata(&self, id: &SymbolRef) -> Option<&Metadata> {
        self.environment.get_metadata(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::cell::RefCell;
    use std::rc::Rc;

    use crate::base::kind::{ArcKind, KindEnv};
    use crate::base::symbol::{Symbol, SymbolModule, SymbolRef, Symbols};
    use crate::base::types::{Alias, TypeEnv};

    pub struct MockEnv;

    impl KindEnv for MockEnv {
        fn find_kind(&self, _type_name: &SymbolRef) -> Option<ArcKind> {
            None
        }
    }

    impl TypeEnv for MockEnv {
        type Type = RcType;
        fn find_type(&self, _id: &SymbolRef) -> Option<&RcType> {
            None
        }
        fn find_type_info(&self, _id: &SymbolRef) -> Option<&Alias<Symbol, RcType>> {
            None
        }
    }

    /// Returns a reference to the interner stored in TLD
    pub fn get_local_interner() -> Rc<RefCell<Symbols>> {
        thread_local!(static INTERNER: Rc<RefCell<Symbols>>
        = Rc::new(RefCell::new(Symbols::new())));
        INTERNER.with(|interner| interner.clone())
    }

    pub fn intern(s: &str) -> Symbol {
        let interner = get_local_interner();
        let mut interner = interner.borrow_mut();

        if s.starts_with(char::is_lowercase) {
            interner.symbol(s)
        } else {
            SymbolModule::new("test".into(), &mut interner).scoped_symbol(s)
        }
    }
}
