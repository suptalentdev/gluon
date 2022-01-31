use std::fs::File;
use std::io::{Read, stdin};

use vm::api::IO;
use vm::{VM, VMInt, Status, Value, RootStr};
use vm::stack::StackFrame;

pub fn array_length(vm: &VM) -> Status {
    match vm.pop() {
        Value::Data(values) => {
            let i = values.fields.len();
            vm.push(Value::Int(i as VMInt));
        }
        x => panic!("{:?}", x)
    }
    Status::Ok
}
pub fn string_length(s: RootStr) -> VMInt {
    s.len() as VMInt
}
pub fn string_append(vm: &VM) -> Status {
    let mut stack = StackFrame::new(vm.stack.borrow_mut(), 2, None);
    match (&stack[0], &stack[1]) {
        (&Value::String(l), &Value::String(r)) => {
            let mut s = String::with_capacity(l.len() + r.len());
            s.push_str(&l);
            s.push_str(&r);
            stack.push(Value::String(vm.gc.borrow_mut().alloc(&s[..])));
        }
        _ => panic!()
    }
    Status::Ok
}
pub fn string_eq(vm: &VM) -> Status {
    let mut stack = StackFrame::new(vm.stack.borrow_mut(), 2, None);
    match (&stack[0], &stack[1]) {
        (&Value::String(l), &Value::String(r)) => {
            let b = if l == r { 1 } else { 0 };
            stack.pop();
            stack.pop();
            stack.push(Value::Int(b));
        }
        _ => panic!()
    }
    Status::Ok
}
pub fn string_slice(s: RootStr, start: VMInt, end: VMInt) -> String {
    String::from(&s[start as usize..end as usize])
}
pub fn print_int(i: VMInt) -> IO<()> {
    print!("{}", i);
    IO(())
}

pub fn read_file(vm: &VM) -> Status {
    let mut stack = StackFrame::new(vm.stack.borrow_mut(), 2, None);
    stack.pop();//Pop "realworld"
    match stack.pop() {
        Value::String(s) => {
            let mut buffer = String::new();
            let status = match File::open(&s[..]).and_then(|mut file| file.read_to_string(&mut buffer)) {
                Ok(_) => Status::Ok,
                Err(err) => {
                    use std::fmt::Write;
                    buffer.clear();
                    let _ = write!(&mut buffer, "{}", err);
                    Status::Error
                }
            };
            let s = vm.alloc(&mut stack.stack.values, &buffer[..]);
            stack.push(Value::String(s));
            status
        }
        x => panic!("read_file called on: {:?}", x)
    }
}

pub fn read_line(vm: &VM) -> Status  {
    let mut stack = StackFrame::new(vm.stack.borrow_mut(), 1, None);
    let mut buffer = String::new();
    let status = match stdin().read_line(&mut buffer) {
        Ok(_) => Status::Ok,
        Err(err) => {
            use std::fmt::Write;
            buffer.clear();
            let _ = write!(&mut buffer, "{}", err);
            Status::Error
        }
    };
    stack[0] = Value::String(vm.gc.borrow_mut().alloc(&buffer[..]));
    status
}

pub fn print(vm: &VM) -> Status  {
    let mut stack = StackFrame::new(vm.stack.borrow_mut(), 2, None);
    stack.pop();//Pop "realworld"
    match stack.pop() {
        Value::String(s) => {
            println!("{}", s);
            stack.push(Value::Int(0));
            Status::Ok
        }
        x => panic!("print called on: {:?}", x)
    }
}

pub fn show(vm: &VM) -> Status  {
    let mut stack = StackFrame::new(vm.stack.borrow_mut(), 1, None);
    let s = match stack[0] {
        Value::Int(i) => format!("{}", i),
        Value::Float(f) => format!("{}", f),
        x => panic!("print_int called on: {:?}", x)
    };
    stack.push(Value::String(vm.gc.borrow_mut().alloc(&s[..])));
    Status::Ok
}

pub fn error(_: &VM) -> Status {
    //We expect a string as an argument to this function but we only return Status::Error
    //and let the caller take care of printing the message
    Status::Error
}

pub fn run_expr(vm: &VM) -> Status {
    let mut stack = StackFrame::frame(vm.stack.borrow_mut(), 2, None);
    let s = stack[0];
    match s {
        Value::String(s) => {
            drop(stack);
            let run_result = ::vm::run_expr(vm, &s);
            stack = StackFrame::new(vm.stack.borrow_mut(), 2, None);
            match run_result {
                Ok(value) => {
                    let fmt = format!("{:?}", value);
                    let result = vm.alloc(&mut stack.stack.values, &fmt[..]);
                    stack.push(Value::String(result));
                }
                Err(err) => {
                    let fmt = format!("{}", err);
                    let result = vm.alloc(&mut stack.stack.values, &fmt[..]);
                    stack.push(Value::String(result));
                }
            }
            stack.exit_scope();
            Status::Ok
        }
        x => panic!("Expected string got {:?}", x)
    }
}

pub fn load_script(vm: &VM) -> Status {
    let mut stack = StackFrame::frame(vm.stack.borrow_mut(), 3, None);
    match (stack[0], stack[1]) {
        (Value::String(name), Value::String(expr)) => {
            drop(stack);
            let run_result = ::vm::load_script(vm, &name, &expr);
            stack = StackFrame::new(vm.stack.borrow_mut(), 3, None);
            match run_result {
                Ok(()) => {
                    let fmt = format!("Loaded {}", name);
                    let result = vm.alloc(&mut stack.stack.values, &fmt[..]);
                    stack.push(Value::String(result));
                }
                Err(err) => {
                    let fmt = format!("{}", err);
                    let result = vm.alloc(&mut stack.stack.values, &fmt[..]);
                    stack.push(Value::String(result));
                }
            }
            stack.exit_scope();
            Status::Ok
        }
        x => panic!("Expected 2 strings got {:?}", x)
    }
}
