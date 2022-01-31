//! Module providing the building blocks to create macros and expand them.
use std::cell::RefCell;
use std::collections::HashMap;
use std::error::Error as StdError;
use std::rc::Rc;

use ast;
use ast::MutVisitor;
use types::TcIdent;
use error::Errors;

pub type Error = Box<StdError>;

/// A trait which abstracts over macros.
///
/// A macro is similiar to a function call but is run at compile time instead of at runtime.
pub trait Macro<Env>: ::mopa::Any {
    fn expand(&self,
              env: &Env,
              arguments: &mut [ast::LExpr<TcIdent>])
              -> Result<ast::LExpr<TcIdent>, Error>;
}
mopafy!(Macro<Env>);

impl<F: ::mopa::Any, Env> Macro<Env> for F
    where F: Fn(&Env, &mut [ast::LExpr<TcIdent>]) -> Result<ast::LExpr<TcIdent>, Error>
{
    fn expand(&self,
              env: &Env,
              arguments: &mut [ast::LExpr<TcIdent>])
              -> Result<ast::LExpr<TcIdent>, Error> {
        self(env, arguments)
    }
}

/// Type containing macros bound to symbols which can be applied on an AST expression to transform
/// it.
pub struct MacroEnv<Env> {
    macros: RefCell<HashMap<String, Rc<Macro<Env>>>>,
}

impl<Env> MacroEnv<Env> {
    /// Creates a new `MacroEnv`
    pub fn new() -> MacroEnv<Env> {
        MacroEnv { macros: RefCell::new(HashMap::new()) }
    }

    /// Inserts a `Macro` which acts on any occurance of `symbol` when applied to an expression.
    pub fn insert<M>(&self, name: String, mac: M)
        where M: Macro<Env> + 'static
    {
        self.macros.borrow_mut().insert(name, Rc::new(mac));
    }

    /// Retrieves the macro bound to `symbol`
    pub fn get(&self, name: &str) -> Option<Rc<Macro<Env>>> {
        self.macros.borrow().get(name).cloned()
    }

    /// Runs the macros in this `MacroEnv` on `expr` using `env` as the context of the expansion
    pub fn run(&self, env: &Env, expr: &mut ast::LExpr<TcIdent>) -> Result<(), Errors<Error>> {
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

struct MacroExpander<'a, Env: 'a> {
    env: &'a Env,
    macros: &'a MacroEnv<Env>,
    errors: Errors<Error>,
}

impl<'a, Env> MutVisitor for MacroExpander<'a, Env> {
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
