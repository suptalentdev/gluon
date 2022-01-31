//! Crate which contain the virtual machine which executes embed_lang programs

#[macro_use]
extern crate log;
#[cfg(test)]
extern crate env_logger;
#[macro_use]
extern crate quick_error;
#[macro_use]
extern crate mopa;

extern crate base;
#[cfg(feature = "parser")]
extern crate parser;
#[cfg(feature = "check")]
extern crate check;

#[macro_use]
pub mod api;
pub mod compiler;
pub mod types;
pub mod vm;
pub mod thread;
pub mod interner;
pub mod gc;
pub mod stack;
pub mod primitives;
pub mod channel;
mod reference;
mod lazy;
mod array;

use api::ValueRef;
use vm::Value;

#[derive(Debug)]
pub struct Variants<'a>(&'a Value);

impl<'a> Variants<'a> {
    pub unsafe fn new(value: &Value) -> Variants {
        Variants(value)
    }

    pub fn as_ref(&self) -> ValueRef {
        ValueRef::new(self.0)
    }
}
