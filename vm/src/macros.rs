//! Module providing the building blocks to create macros and expand them.
use std::sync::RwLock;
use std::collections::HashMap;
use std::error::Error as StdError;

use base::ast;
use base::ast::MutVisitor;
use base::types::TcIdent;
use base::error::Errors;

use thread::Thread;

pub type Error = Box<StdError + Send + Sync>;

/// A trait which abstracts over macros.
///
/// A macro is similiar to a function call but is run at compile time instead of at runtime.
pub trait Macro: ::mopa::Any + Send + Sync {
    fn expand(&self,
              env: &Thread,
              arguments: &mut [ast::LExpr<TcIdent>])
              -> Result<ast::LExpr<TcIdent>, Error>;
    fn clone(&self) -> Box<Macro>;
}
mopafy!(Macro);

impl<F: ::mopa::Any + Clone + Send + Sync> Macro for F
    where F: Fn(&Thread, &mut [ast::LExpr<TcIdent>]) -> Result<ast::LExpr<TcIdent>, Error>
{
    fn expand(&self,
              env: &Thread,
              arguments: &mut [ast::LExpr<TcIdent>])
              -> Result<ast::LExpr<TcIdent>, Error> {
        self(env, arguments)
    }
    fn clone(&self) -> Box<Macro> {
        Box::new(Clone::clone(self))
    }
}

/// Type containing macros bound to symbols which can be applied on an AST expression to transform
/// it.
pub struct MacroEnv {
    macros: RwLock<HashMap<String, Box<Macro>>>,
}

impl MacroEnv {
    /// Creates a new `MacroEnv`
    pub fn new() -> MacroEnv {
        MacroEnv { macros: RwLock::new(HashMap::new()) }
    }

    /// Inserts a `Macro` which acts on any occurance of `symbol` when applied to an expression.
    pub fn insert<M>(&self, name: String, mac: M)
        where M: Macro + 'static
    {
        self.macros.write().unwrap().insert(name, Box::new(mac));
    }

    /// Retrieves the macro bound to `symbol`
    pub fn get(&self, name: &str) -> Option<Box<Macro>> {
        self.macros.read().unwrap().get(name).map(|x| (**x).clone())
    }

    /// Runs the macros in this `MacroEnv` on `expr` using `env` as the context of the expansion
    pub fn run(&self, env: &Thread, expr: &mut ast::LExpr<TcIdent>) -> Result<(), Errors<Error>> {
        let mut expander = MacroExpander {
            env: env,
            macros: self,
            errors: Errors::new(),
        };
        expander.visit_expr(expr);
        if expander.errors.has_errors() {
            Err(expander.errors)
        } else {
            Ok(())
        }
    }
}

struct MacroExpander<'a> {
    env: &'a Thread,
    macros: &'a MacroEnv,
    errors: Errors<Error>,
}

impl<'a> MutVisitor for MacroExpander<'a> {
    type T = TcIdent;

    fn visit_expr(&mut self, expr: &mut ast::LExpr<TcIdent>) {
        let replacement = match expr.value {
            ast::Expr::Call(ref mut id, ref mut args) => {
                match ***id {
                    ast::Expr::Identifier(ref id) => {
                        match self.macros.get(id.name.as_ref()) {
                            Some(m) => {
                                match m.expand(self.env, args) {
                                    Ok(e) => Some(e),
                                    Err(err) => {
                                        self.errors.error(err);
                                        None
                                    }
                                }
                            }
                            None => None,
                        }
                    }
                    _ => None,
                }
            }
            _ => None,
        };
        if let Some(mut e) = replacement {
            e.location = expr.location;
            *expr = e;
        }
        ast::walk_mut_expr(self, expr);
    }
}
