use std::string::String as StdString;
use std::io::{Read, stdin};
use std::fmt;
use std::fs::File;
use std::sync::Mutex;

use vm::{Result, Variants};
use vm::gc::{Gc, Traverseable};
use vm::types::*;
use vm::thread::ThreadInternal;
use vm::thread::{Thread, Status, RootStr};
use vm::api::{Array, Generic, VmType, Getable, Pushable, IO, WithVM, Userdata, primitive};
use vm::api::generic::{A, B};

use vm::internal::Value;

use super::Compiler;

fn print_int(i: VmInt) -> IO<()> {
    print!("{}", i);
    IO::Value(())
}

fn print(s: RootStr) -> IO<()> {
    println!("{}", &*s);
    IO::Value(())
}

struct GluonFile(Mutex<File>);

impl Userdata for GluonFile { }

impl fmt::Debug for GluonFile {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "File")
    }
}

impl VmType for GluonFile {
    type Type = GluonFile;
}

impl Traverseable for GluonFile {
    fn traverse(&self, _: &mut Gc) {}
}

fn open_file(s: &str) -> IO<GluonFile> {
    match File::open(s) {
        Ok(f) => IO::Value(GluonFile(Mutex::new(f))),
        Err(err) => IO::Exception(format!("{}", err)),
    }
}

fn read_file<'vm>(file: WithVM<'vm, &GluonFile>, count: usize) -> IO<Array<'vm, u8>> {
    let WithVM { vm, value: file } = file;
    let mut file = file.0.lock().unwrap();
    let mut buffer = Vec::with_capacity(count);
    unsafe {
        buffer.set_len(count);
        match file.read(&mut *buffer) {
            Ok(bytes_read) => {
                let value = {
                    let stack = vm.get_stack();
                    vm.alloc(&stack, &buffer[..bytes_read])
                };
                IO::Value(Getable::from_value(vm, Variants::new(&Value::Array(value)))
                    .expect("Array"))
            }
            Err(err) => IO::Exception(format!("{}", err)),
        }
    }
}

fn read_file_to_string(s: &str) -> IO<String> {
    let mut buffer = String::new();
    match File::open(s).and_then(|mut file| file.read_to_string(&mut buffer)) {
        Ok(_) => IO::Value(buffer),
        Err(err) => {
            use std::fmt::Write;
            buffer.clear();
            let _ = write!(&mut buffer, "{}", err);
            IO::Exception(buffer)
        }
    }
}

fn read_char() -> IO<char> {
    match stdin().bytes().next() {
        Some(result) => {
            match result {
                Ok(b) => {
                    ::std::char::from_u32(b as u32)
                        .map(IO::Value)
                        .unwrap_or_else(|| IO::Exception("Not a valid char".into()))
                }
                Err(err) => IO::Exception(format!("{}", err)),
            }
        }
        None => IO::Exception("No read".into()),
    }
}

fn read_line() -> IO<String> {
    let mut buffer = String::new();
    match stdin().read_line(&mut buffer) {
        Ok(_) => IO::Value(buffer),
        Err(err) => {
            use std::fmt::Write;
            buffer.clear();
            let _ = write!(&mut buffer, "{}", err);
            IO::Exception(buffer)
        }
    }
}

/// IO a -> (String -> IO a) -> IO a
fn catch_io(vm: &Thread) -> Status {
    let mut stack = vm.current_frame();
    let frame_level = stack.stack.get_frames().len();
    let action = stack[0];
    stack.push(action);
    0.push(vm, &mut stack.stack);
    match vm.call_function(stack, 1) {
        Ok(_) => Status::Ok,
        Err(err) => {
            stack = vm.current_frame();
            while stack.stack.get_frames().len() > frame_level {
                match stack.exit_scope() {
                    Some(new_stack) => stack = new_stack,
                    None => return Status::Error,
                }
            }
            let callback = stack[1];
            stack.push(callback);
            let fmt = format!("{}", err);
            fmt.push(vm, &mut stack.stack);
            0.push(vm, &mut stack.stack);
            match vm.call_function(stack, 2) {
                Ok(_) => Status::Ok,
                Err(err) => {
                    stack = vm.current_frame();
                    let fmt = format!("{}", err);
                    fmt.push(vm, &mut stack.stack);
                    Status::Error
                }
            }
        }
    }
}

fn run_expr(expr: WithVM<RootStr>) -> IO<String> {
    let WithVM { vm, value: expr } = expr;
    let mut stack = vm.current_frame();
    let frame_level = stack.stack.get_frames().len();
    drop(stack);
    let run_result = Compiler::new().run_expr::<Generic<A>>(vm, "<top>", &expr);
    stack = vm.current_frame();
    match run_result {
        Ok((value, typ)) => IO::Value(format!("{:?} : {}", value.0, typ)),
        Err(err) => {
            let trace = stack.stacktrace(frame_level);
            let fmt = format!("{}\n{}", err, trace);
            while stack.stack.get_frames().len() > frame_level {
                match stack.exit_scope() {
                    Some(new_stack) => stack = new_stack,
                    None => return IO::Exception(fmt),
                }
            }
            IO::Exception(fmt)
        }
    }
}

fn load_script(name: WithVM<RootStr>, expr: RootStr) -> IO<String> {
    let WithVM { vm, value: name } = name;
    let mut stack = vm.current_frame();
    let frame_level = stack.stack.get_frames().len();
    drop(stack);
    let run_result = Compiler::new().load_script(vm, &name[..], &expr);
    stack = vm.current_frame();
    match run_result {
        Ok(()) => IO::Value(format!("Loaded {}", &name[..])),
        Err(err) => {
            let trace = stack.stacktrace(frame_level);
            let fmt = format!("{}\n{}", err, trace);
            while stack.stack.get_frames().len() > frame_level {
                match stack.exit_scope() {
                    Some(new_stack) => stack = new_stack,
                    None => return IO::Exception(fmt),
                }
            }
            IO::Exception(fmt)
        }
    }
}

pub fn load(vm: &Thread) -> Result<()> {

    try!(vm.register_type::<GluonFile>("File", &[]));

    // io_flat_map f m : (a -> IO b) -> IO a -> IO b
    //     = f (m ())
    let io_flat_map = vec![
                        // [f, m, ()]       Initial stack
        Call(1),        // [f, m_ret]       Call m ()
        PushInt(0),     // [f, m_ret, ()]   Add a dummy argument ()
        TailCall(2),    // [f_ret]          Call f m_ret ()
    ];
    let io_flat_map_type = <fn (fn (A) -> IO<B>, IO<A>) -> IO<B> as VmType>::make_type(vm);
    vm.add_bytecode("io_flat_map", io_flat_map_type, 3, io_flat_map);


    vm.add_bytecode("io_pure",
                    <fn(A) -> IO<A> as VmType>::make_type(vm),
                    2,
                    vec![Pop(1)]);
    // IO functions
    try!(vm.define_global("io",
                          record!(
        print_int => primitive!(1 print_int),
        open_file => primitive!(1 open_file),
        read_file => primitive!(2 read_file),
        read_file_to_string => primitive!(1 read_file_to_string),
        read_char => primitive!(0 read_char),
        read_line => primitive!(0 read_line),
        print => primitive!(1 print),
        catch =>
            primitive::<fn (IO<A>, fn (StdString) -> IO<A>) -> IO<A>>("io.catch", catch_io),
        run_expr => primitive!(1 run_expr),
        load_script => primitive!(2 load_script)
    )));
    Ok(())
}
