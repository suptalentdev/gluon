//! The `check` crate is responsible for ensuring that an AST expression is actually a valid
//! program. This currently consits of three larger parts, typechecking, kindchecking and renaming.
//! If an AST passes the checks in `Typecheck::typecheck_expr` (which runs all of theses checks
//! the expression is expected to compile succesfully (if it does not it should be considered an
//! internal compiler error.

#[macro_use]
extern crate log;
#[cfg(test)]
extern crate env_logger;
extern crate union_find;

extern crate gluon_base as base;

pub mod typecheck;
pub mod unify_type;
pub mod unify;
pub mod kindcheck;
mod substitution;
mod rename;
pub mod completion;
pub mod metadata;

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use base::types::{Alias, KindEnv, ArcKind, ArcType, TypeEnv};
    use base::symbol::{Symbol, Symbols, SymbolModule, SymbolRef};

    pub struct MockEnv;

    impl KindEnv for MockEnv {
        fn find_kind(&self, _type_name: &SymbolRef) -> Option<ArcKind> {
            None
        }
    }

    impl TypeEnv for MockEnv {
        fn find_type(&self, _id: &SymbolRef) -> Option<&ArcType> {
            None
        }
        fn find_type_info(&self, _id: &SymbolRef) -> Option<&Alias<Symbol, ArcType>> {
            None
        }
        fn find_record(&self, _fields: &[Symbol]) -> Option<(&ArcType, &ArcType)> {
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
