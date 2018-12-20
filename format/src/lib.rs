//! Code formatter.
#![doc(html_root_url = "https://docs.rs/gluon_formatter/0.9.4")] // # GLUON

extern crate codespan;
#[macro_use]
extern crate gluon_base as base;
extern crate itertools;
extern crate pretty;

use base::{ast::SpannedExpr, symbol::Symbol};

mod pretty_print;

pub fn pretty_expr(input: &str, expr: &SpannedExpr<Symbol>) -> String {
    Formatter::default().pretty_expr(input, expr)
}

#[derive(Default, Debug, Clone)]
pub struct Formatter {
    /// Prints the source code after macro expansion
    ///
    /// NOTE: This is only provided for debug purposes and is likely to have have bugs
    pub expanded: bool,
}

impl Formatter {
    pub fn pretty_expr(&self, input: &str, expr: &SpannedExpr<Symbol>) -> String {
        let newline = match input.find(|c: char| c == '\n' || c == '\r') {
            Some(i) => {
                if input[i..].starts_with("\r\n") {
                    "\r\n"
                } else if input[i..].starts_with("\r") {
                    "\r"
                } else {
                    "\n"
                }
            }
            None => "\n",
        };

        let source = codespan::FileMap::new("test".into(), input.into());
        let arena = pretty::Arena::<()>::new();
        let printer = pretty_print::Printer::new(&arena, &source, self.clone());
        printer.format(100, newline, &expr)
    }
}
