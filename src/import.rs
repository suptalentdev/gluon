use std::sync::RwLock;
use std::fs::File;
use std::io;
use std::io::Read;
use std::path::{Path, PathBuf};

use base::ast;
use base::symbol::Symbol;
use vm::vm::Thread;
use super::{filename_to_module, Compiler};
use base::macros::{Macro, Error as MacroError};
use base::types::TcIdent;


quick_error! {
    /// Error type for the import macro
    #[derive(Debug)]
    pub enum Error {
        /// The importer found a cyclic dependency when loading files
        CyclicDependency(module: String) {
            description("Cyclic dependency")
            display("Module '{}' occurs in a cyclic dependency", module)
        }
        /// Generic message error
        String(message: &'static str) {
            description(message)
            display("{}", message)
        }
        /// The importer could not load the imported file
        IO(err: io::Error) {
            description(err.description())
            display("{}", err)
            from()
        }
    }
}

macro_rules! std_libs {
    ($($file: expr),*) => {
        [$((concat!("std/", $file, ".hs"), include_str!(concat!("../std/", $file, ".hs")))),*]
    }
}
// Include the standard library distribution in the binary
static STD_LIBS: [(&'static str, &'static str); 7] = std_libs!("prelude",
                                                               "map",
                                                               "repl",
                                                               "string",
                                                               "state",
                                                               "test",
                                                               "writer");

/// Macro which rewrites occurances of `import "filename"` to a load of that file if it is not
/// already loaded and then a global access to the loaded module
pub struct Import {
    visited: RwLock<Vec<String>>,
    paths: RwLock<Vec<PathBuf>>,
}

impl Import {
    /// Creates a new import macro
    pub fn new() -> Import {
        Import {
            visited: RwLock::new(Vec::new()),
            paths: RwLock::new(vec![PathBuf::from(".")]),
        }
    }

    /// Adds a path to the list of paths which the importer uses to find files
    pub fn add_path<P: Into<PathBuf>>(&self, path: P) {
        self.paths.write().unwrap().push(path.into());
    }
}

impl Macro<Thread> for Import {
    fn expand(&self,
              vm: &Thread,
              arguments: &mut [ast::LExpr<TcIdent>])
              -> Result<ast::LExpr<TcIdent>, MacroError> {
        if arguments.len() != 1 {
            return Err(Error::String("Expected import to get 1 argument").into());
        }
        match *arguments[0] {
            ast::Expr::Literal(ast::LiteralEnum::String(ref filename)) => {
                let modulename = filename_to_module(filename);
                let path = Path::new(&filename[..]);
                // Only load the script if it is not already loaded
                let name = Symbol::new(&*modulename);
                debug!("Import '{}' {:?}", modulename, self.visited);
                if !vm.global_exists(&modulename) {
                    if self.visited.read().unwrap().iter().any(|m| **m == **filename) {
                        return Err(Error::CyclicDependency(filename.clone()).into());
                    }
                    self.visited.write().unwrap().push(filename.clone());
                    let mut buffer = String::new();
                    let file_contents = match STD_LIBS.iter().find(|tup| tup.0 == filename) {
                        Some(tup) => tup.1,
                        None => {
                            let file = self.paths
                                           .read().unwrap()
                                           .iter()
                                           .filter_map(|p| {
                                               let mut base = p.clone();
                                               base.push(path);
                                               match File::open(&base) {
                                                   Ok(file) => Some(file),
                                                   Err(_) => None,
                                               }
                                           })
                                           .next();
                            let mut file = try!(file.ok_or_else(|| {
                                Error::String("Could not find file")
                            }));
                            try!(file.read_to_string(&mut buffer));
                            &*buffer
                        }
                    };
                    // FIXME Remove this hack
                    let mut compiler = Compiler::new().implicit_prelude(modulename != "std.types");
                    try!(compiler.load_script(vm, &modulename, file_contents));
                    self.visited.write().unwrap().pop();
                }
                // FIXME Does not handle shadowing
                Ok(ast::located(arguments[0].location,
                                ast::Expr::Identifier(TcIdent::new(name))))
            }
            _ => return Err(Error::String("Expected a string literal to import").into()),
        }
    }

    fn clone(&self) -> Box<Macro<Thread>> {
        Box::new(Import {
            visited: RwLock::new(Vec::new()),
            paths: RwLock::new(self.paths.read().unwrap().clone()),
        })
    }
}
