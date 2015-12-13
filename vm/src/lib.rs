#[macro_use]
extern crate log;
#[cfg(test)]
extern crate env_logger;

extern crate base;
extern crate parser;
extern crate check;

#[macro_use]
pub mod api;
pub mod compiler;
pub mod types;
pub mod vm;
mod stack;
mod primitives;
mod lazy;
mod import;
