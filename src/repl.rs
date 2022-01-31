use std::error::Error as StdError;
use check::typecheck::TypeEnv;
use check::Typed;
use vm::vm::{VM, RootStr, Status, typecheck_expr, load_file};
use vm::api::{VMFunction, IO, Get, Callable, primitive};

fn type_of_expr(vm: &VM) -> Status {
    let closure: &Fn(_) -> _ = &|args: RootStr| -> IO<String> {
        IO::Value(match typecheck_expr(vm, &args) {
            Ok((expr, _)) => {
                let ref env = vm.env();
                format!("{}", expr.env_type_of(env))
            }
            Err(msg) => format!("{}", msg),
        })
    };
    closure.unpack_and_call(vm)
}

fn find_type_info(vm: &VM) -> Status {
    let closure: &Fn(RootStr) -> IO<String> = &|args| {
        let args = args.trim();
        IO::Value(match vm.env().find_type_info(&vm.intern(args)) {
            Some((generic_args, typ)) => {
                let fmt = || -> Result<String, ::std::fmt::Error> {
                    use std::fmt::Write;
                    let mut buffer = String::new();
                    try!(write!(&mut buffer, "type {}", args));
                    for g in generic_args {
                        try!(write!(&mut buffer, " {}", g))
                    }
                    try!(write!(&mut buffer, " = "));
                    match typ {
                        Some(typ) => try!(write!(&mut buffer, "{}", typ)),
                        None => try!(write!(&mut buffer, "<abstract>")),
                    }
                    Ok(buffer)
                };
                fmt().unwrap()
            }
            None => format!("'{}' is not a type", args),
        })
    };
    closure.unpack_and_call(vm)
}

fn compile_repl(vm: &VM) -> Result<(), Box<StdError>> {
    try!(vm.define_global("repl_prim",
                          record!(
        type_of_expr => primitive::<fn (String) -> IO<String>>(type_of_expr),
        find_type_info => primitive::<fn (String) -> IO<String>>(find_type_info)
    )));
    try!(load_file(vm, "std/prelude.hs"));
    try!(load_file(vm, "std/repl.hs"));
    Ok(())
}

#[allow(dead_code)]
pub fn run() -> Result<(), Box<StdError>> {
    let vm = VM::new();
    try!(compile_repl(&vm));
    let mut repl: Callable<((),), IO<()>> = Get::get_function(&vm, "repl").expect("repl function");
    try!(repl.call(()));
    Ok(())
}

#[cfg(test)]
mod tests {
    use vm::vm::VM;
    use super::compile_repl;

    #[test]
    fn compile_repl_test() {
        let vm = VM::new();
        compile_repl(&vm).unwrap_or_else(|err| panic!("{}", err));
    }
}
