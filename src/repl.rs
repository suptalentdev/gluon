use std::error::Error as StdError;
use std::io;
use std::io::BufRead;

use embed_lang::typecheck::*;
use embed_lang::vm::{VM, Error, Status, typecheck_expr, run_expr, load_script};

fn print(vm: &VM) -> Status {
    println!("{:?}", vm.pop());
    Status::Ok
}

#[allow(dead_code)]
pub fn run() {
    let vm = VM::new();
    vm.extern_function("printInt", vec![Type::int()], Type::unit(), Box::new(print))
        .unwrap();
    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        match run_line(&vm, line) {
            Ok(continue_repl) => {
                if !continue_repl {
                    break
                }
            }
            Err(e) => println!("{}", e)
        }
    }
}

fn run_command(vm: &VM, command: char, args: &str) -> Result<bool, Box<StdError>> {
    match command {
        'q' => Ok(false),
        'l' => {
            try!(load_file(vm, args));
            Ok(true)
        }
        't' => {
            let (expr, _) = try!(typecheck_expr(vm, args));
            println!("{}", expr.type_of());
            Ok(true)
        }
        'i' => {
            match vm.env().find_type_info(&vm.intern(args)) {
                Some(typ) => {
                    println!("type {} = {}", args, typ);
                }
                None => println!("{} is not a type", args)
            }
            Ok(true)
        }
        _ => Err(Error::Message("Invalid command ".to_string() + &*command.to_string()).into())
    }
}

fn run_line(vm: &VM, line: io::Result<String>) -> Result<bool, Box<StdError>> {
    let expr_str = try!(line);
    match expr_str.chars().next().unwrap() {
        ':' => {
            run_command(vm, expr_str.chars().skip(1).next().unwrap(), expr_str[2..].trim())
        }
        _ =>  {
            let v = try!(run_expr(vm, &expr_str));
            println!("{:?}", v);
            Ok(true)
        }
    }
}

fn load_file(vm: &VM, filename: &str) -> Result<(), Box<StdError>> {
    use std::fs::File;
    use std::io::Read;
    use std::path::Path;
    let path = Path::new(filename);
    let mut file = try!(File::open(path));
    let mut buffer = String::new();
    try!(file.read_to_string(&mut buffer));
    let name = path.file_stem().and_then(|f| f.to_str()).expect("filename");
    load_script(vm, name, &buffer)
}

