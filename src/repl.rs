use std::error::Error as StdError;
use base::ast::Typed;
use base::types::{Kind, TypeEnv};
use vm::vm::{VM, RootStr};
use vm::api::{IO, Function, WithVM};

use embed_lang::{Compiler, new_vm};

fn type_of_expr(args: WithVM<RootStr>) -> IO<String> {
    let WithVM { vm, value: args } = args;
    let mut compiler = Compiler::new().implicit_prelude(false);
    IO::Value(match compiler.typecheck_expr(vm, "<repl>", &args) {
        Ok((expr, _)) => {
            let env = vm.get_env();
            format!("{}", expr.env_type_of(&*env))
        }
        Err(msg) => format!("{}", msg),
    })
}

fn find_kind(args: WithVM<RootStr>) -> IO<String> {
    let vm = args.vm;
    let args = args.value.trim();
    IO::Value(match vm.find_type_info(args) {
        Ok(ref alias) => {
            let kind = alias.args.iter().rev().fold(Kind::star(), |acc, arg| {
                Kind::function(arg.kind.clone(), acc)
            });
            format!("{}", kind)
        }
        Err(err) => format!("{}", err),
    })
}

fn find_info(args: WithVM<RootStr>) -> IO<String> {
    use std::fmt::Write;
    let vm = args.vm;
    let args = args.value.trim();
    let env = vm.get_env();
    let mut buffer = String::new();
    match env.find_type_info(args) {
        Ok(alias) => {
            // Found a type alias
            let mut fmt = || -> Result<(), ::std::fmt::Error> {
                try!(write!(&mut buffer, "type {}", args));
                for g in &alias.args {
                    try!(write!(&mut buffer, " {}", g.id))
                }
                try!(write!(&mut buffer, " = "));
                match alias.typ {
                    Some(ref typ) => try!(write!(&mut buffer, "{}", typ)),
                    None => try!(write!(&mut buffer, "<abstract>")),
                }
                Ok(())
            };
            fmt().unwrap();
        }
        Err(err) => {
            // Try to find a value at `args` to print its type and documentation comment (if any)
            match env.get_binding_info(args) {
                Ok(typ) => {
                    write!(&mut buffer, "{}: {}", args, typ).unwrap();
                }
                Err(_) => return IO::Value(format!("{}", err)),
            }
        }
    }
    let maybe_comment = env.get_metadata(args)
                           .ok()
                           .and_then(|metadata| metadata.comment.as_ref());
    if let Some(comment) = maybe_comment {
        for line in comment.lines() {
            write!(&mut buffer, "\n/// {}", line).unwrap();
        }
    }
    IO::Value(buffer)
}

fn f1<A, R>(f: fn(A) -> R) -> fn(A) -> R {
    f
}

fn compile_repl(vm: &VM) -> Result<(), Box<StdError>> {
    try!(vm.define_global("repl_prim",
                          record!(
        type_of_expr => f1(type_of_expr),
        find_info => f1(find_info),
        find_kind => f1(find_kind)
    )));
    let mut compiler = Compiler::new();
    try!(compiler.load_file(vm, "std/prelude.hs"));
    try!(compiler.load_file(vm, "std/repl.hs"));
    Ok(())
}

#[allow(dead_code)]
pub fn run() -> Result<(), Box<StdError>> {
    let vm = new_vm();
    try!(compile_repl(&vm));
    let mut repl: Function<fn(()) -> IO<()>> = try!(vm.get_global("std.repl"));
    try!(repl.call(()));
    Ok(())
}

#[cfg(test)]
mod tests {
    use embed_lang::new_vm;
    use super::compile_repl;
    use vm::api::{IO, Function};

    #[test]
    fn compile_repl_test() {
        let _ = ::env_logger::init();
        let vm = new_vm();
        compile_repl(&vm).unwrap_or_else(|err| panic!("{}", err));
        let repl: Result<Function<fn(()) -> IO<()>>, _> = vm.get_global("std.repl");
        assert!(repl.is_ok());
    }

    #[test]
    fn type_of_expr() {
        let _ = ::env_logger::init();
        let vm = new_vm();
        compile_repl(&vm).unwrap_or_else(|err| panic!("{}", err));
        let mut type_of: Function<fn(&'static str) -> IO<String>> = vm.get_global("repl_prim.type_of_expr").unwrap();
        assert!(type_of.call("std.prelude.Option").is_ok());
    }

    #[test]
    fn find_kind() {
        let _ = ::env_logger::init();
        let vm = new_vm();
        compile_repl(&vm).unwrap_or_else(|err| panic!("{}", err));
        let mut find_kind: Function<fn(&'static str) -> IO<String>> = vm.get_global("repl_prim.find_kind").unwrap();
        assert_eq!(find_kind.call("std.prelude.Option"), Ok(IO::Value("* -> *".into())));
    }

    #[test]
    fn find_info() {
        let _ = ::env_logger::init();
        let vm = new_vm();
        compile_repl(&vm).unwrap_or_else(|err| panic!("{}", err));
        let mut find_info: Function<fn(&'static str) -> IO<String>> = vm.get_global("repl_prim.find_info").unwrap();
        assert!(find_info.call("std.prelude.Option").is_ok());
        assert!(find_info.call("std.prelude.id").is_ok());
    }
}
