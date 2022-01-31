use base::ast;
use vm::{VM, VMResult, Status, BytecodeFunction, Value, Userdata_, StackFrame, VMInt, Error};
use typecheck::{TcType, Typed, Type, UNIT_TYPE, BOOL_TYPE, INT_TYPE, FLOAT_TYPE, STRING_TYPE};
use compiler::Instruction::Call;
use compiler::VMIndex;
use std::any::Any;
use std::fmt;
use std::marker::PhantomData;


#[derive(Debug)]
pub struct IO<T>(pub T);

pub trait VMType {
    fn vm_type<'a>(vm: &'a VM) -> &'a TcType;
    fn make_type(vm: &VM) -> TcType {
        <Self as VMType>::vm_type(vm).clone()
    }
}

pub trait Pushable<'a> : VMType {
    fn push<'b>(self, vm: &VM<'a>, stack: &mut StackFrame<'a, 'b>) -> Status;
}
pub trait Getable<'a> {
    fn from_value(vm: &VM<'a>, value: Value<'a>) -> Option<Self>;
}
pub trait VMValue<'a> : Pushable<'a> + Getable<'a> { }
impl <'a, T> VMValue<'a> for T where T: Pushable<'a> + Getable<'a> { }

impl VMType for () {
    fn vm_type<'a>(_: &VM) -> &'a TcType {
        &UNIT_TYPE
    }
}
impl <'a> Pushable<'a> for () {
    fn push<'b>(self, _: &VM<'a>, _: &mut StackFrame<'a, 'b>) -> Status {
        Status::Ok
    }
}
impl <'a> Getable<'a> for () {
    fn from_value(_: &VM<'a>, _: Value) -> Option<()> {
        Some(())
    }
}

impl VMType for VMInt {
    fn vm_type<'a>(_: &'a VM) -> &'a TcType {
        &INT_TYPE
    }
}
impl <'a> Pushable<'a> for VMInt {
    fn push<'b>(self, _: &VM<'a>, stack: &mut StackFrame<'a, 'b>) -> Status {
        stack.push(Value::Int(self));
        Status::Ok
    }
}
impl <'a> Getable<'a> for VMInt {
    fn from_value(_: &VM<'a>, value: Value<'a>) -> Option<VMInt> {
        match value {
            Value::Int(i) => Some(i),
            _ => None
        }
    }
}
impl VMType for f64 {
    fn vm_type<'a>(_: &'a VM) -> &'a TcType {
        &FLOAT_TYPE
    }
}
impl <'a> Pushable<'a> for f64 {
    fn push<'b>(self, _: &VM<'a>, stack: &mut StackFrame<'a, 'b>) -> Status {
        stack.push(Value::Float(self));
        Status::Ok
    }
}
impl <'a> Getable<'a> for f64 {
    fn from_value(_: &VM<'a>, value: Value<'a>) -> Option<f64> {
        match value {
            Value::Float(f) => Some(f),
            _ => None
        }
    }
}
impl VMType for bool {
    fn vm_type<'a>(_: &'a VM) -> &'a TcType {
        &BOOL_TYPE
    }
}
impl <'a> Pushable<'a> for bool {
    fn push<'b>(self, _: &VM<'a>, stack: &mut StackFrame<'a, 'b>) -> Status {
        stack.push(Value::Int(if self { 1 } else { 0 }));
        Status::Ok
    }
}
impl <'a> Getable<'a> for bool {
    fn from_value(_: &VM<'a>, value: Value<'a>) -> Option<bool> {
        match value {
            Value::Int(1) => Some(true),
            Value::Int(0) => Some(false),
            _ => None
        }
    }
}
impl <'s> VMType for &'s str {
    fn vm_type<'a>(_: &'a VM) -> &'a TcType {
        &STRING_TYPE
    }
}
impl <'a, 's> Pushable<'a> for &'s str {
    fn push<'b>(self, vm: &VM<'a>, stack: &mut StackFrame<'a, 'b>) -> Status {
        let s = vm.alloc(&mut stack.stack.values, self);
        stack.push(Value::String(s));
        Status::Ok
    }
}
impl <'a> Getable<'a> for String {
    fn from_value(_: &VM<'a>, value: Value<'a>) -> Option<String> {
        match value {
            Value::String(i) => Some(String::from(&i[..])),
            _ => None
        }
    }
}
impl <T: Any> VMType for Box<T> {
    fn vm_type<'a>(vm: &'a VM) -> &'a TcType {
        vm.get_type::<T>()
    }
}
impl <'a, T: Any> Pushable<'a> for Box<T> {
    fn push<'b>(self, vm: &VM<'a>, stack: &mut StackFrame<'a, 'b>) -> Status {
        stack.push(Value::Userdata(Userdata_::new(vm, self)));
        Status::Ok
    }
}

impl <T: Any> VMType for *mut T {
    fn vm_type<'a>(vm: &'a VM) -> &'a TcType {
        vm.get_type::<T>()
    }
}
impl <'a, T: Any> Pushable<'a> for *mut T {
    fn push<'b>(self, vm: &VM<'a>, stack: &mut StackFrame<'a, 'b>) -> Status {
        stack.push(Value::Userdata(Userdata_::new(vm, self)));
        Status::Ok
    }
}
impl <'a, T: Any> Getable<'a> for *mut T {
    fn from_value(_: &VM<'a>, value: Value<'a>) -> Option<*mut T> {
        match value {
            Value::Userdata(v) => v.data.downcast_ref::<*mut T>().map(|x| *x),
            _ => None
        }
    }
}

impl <T: VMType, E> VMType for Result<T, E> {
    fn vm_type<'b>(vm: &'b VM) -> &'b TcType {
        T::vm_type(vm)
    }
}
impl <'a, T: Pushable<'a>, E: fmt::Display> Pushable<'a> for Result<T, E> {
    fn push<'b>(self, vm: &VM<'a>, stack: &mut StackFrame<'a, 'b>) -> Status {
        match self {
            Ok(value) => {
                value.push(vm, stack);
                Status::Ok
            }
            Err(err) => {
                let msg = format!("{}", err);
                let s = vm.alloc(&mut stack.stack.values, &msg[..]);
                stack.push(Value::String(s));
                Status::Error
            }
        }
    }
}

impl <T: Any + VMType> VMType for IO<T> {
    fn vm_type<'b>(vm: &'b VM) -> &'b TcType {
        vm.get_type::<IO<T>>()
    }
    fn make_type(vm: &VM) -> TcType {
        ast::Type::Data(ast::TypeConstructor::Data(vm.intern("IO")), vec![T::make_type(vm)])
    }
}
impl <'a, T: Pushable<'a> + Any> Pushable<'a> for IO<T> {
    fn push<'b>(self, vm: &VM<'a>, stack: &mut StackFrame<'a, 'b>) -> Status {
        self.0.push(vm, stack)
    }
}


fn vm_type<'a, T: ?Sized + VMType>(vm: &'a VM) -> &'a TcType {
    <T as VMType>::vm_type(vm)
}

fn make_type<T: ?Sized + VMType>(vm: &VM) -> TcType {
    <T as VMType>::make_type(vm)
}

pub trait Get<'a, 'b> {
    fn get_function(vm: &'a VM<'b>, name: &str) -> Option<Self>;
}


pub struct ArgIterator<'a> {
    pub typ: &'a TcType
}
fn arg_iter(typ: &TcType) -> ArgIterator {
    ArgIterator { typ: typ }
}
impl <'a> Iterator for ArgIterator<'a> {
    type Item = &'a TcType;
    fn next(&mut self) -> Option<&'a TcType> {
        match *self.typ {
            Type::Function(ref arg, ref return_type) => {
                self.typ = &**return_type;
                Some(&arg[0])
            }
            _ => None
        }
    }
}

macro_rules! make_get {
    ($($args:ident),*) => (
impl <'a, 'b, $($args : VMValue<'b>,)* R: VMValue<'b>> Get<'a, 'b> for Callable<'a, 'b, ($($args,)*), R> {
    fn get_function(vm: &'a VM<'b>, name: &str) -> Option<Callable<'a, 'b, ($($args,)*), R>> {
        let value = match vm.get_global(name) {
            Some(global) => {
                let mut arg_iter = arg_iter(global.type_of());
                let ok = $({
                    arg_iter.next().unwrap() == vm_type::<$args>(vm)
                    } &&)* true;
                if arg_iter.next().is_none() && ok && arg_iter.typ == vm_type::<R>(vm) {
                    Some(FunctionRef { value: global.value.get(), _marker: PhantomData })
                }
                else {
                    None
                }
            }
            None => None
        };
        match value {
            Some(value) => Some(Callable { vm: vm, value: value }),
            None => None
        }
    }
}
)}

make_get!();
make_get!(A);
make_get!(A, B);
make_get!(A, B, C);
make_get!(A, B, C, D);
make_get!(A, B, C, D, E);
make_get!(A, B, C, D, E, F);
make_get!(A, B, C, D, E, F, G);

pub struct Callable<'a, 'b: 'a , Args, R> {
    vm: &'a VM<'b>,
    value: FunctionRef<'b, Args, R>
}
struct FunctionRef<'a, Args, R> {
    value: Value<'a>,
    _marker: PhantomData<fn (Args) -> R>
}

impl <'a, Args, R> Copy for FunctionRef<'a, Args, R> { }
impl <'a, Args, R> Clone for FunctionRef<'a, Args, R> { fn clone(&self) -> FunctionRef<'a, Args, R> { *self } }

impl <'b, Args, R> VMType for FunctionRef<'b, Args, R> {
    fn vm_type<'a>(vm: &'a VM) -> &'a TcType {
        vm.get_type::<&fn (Args) -> R>()
    }
}

impl <'a, Args, R> Pushable<'a> for FunctionRef<'a, Args, R> {
    fn push<'b>(self, _: &VM<'a>, stack: &mut StackFrame<'a, 'b>) -> Status {
        stack.push(self.value);
        Status::Ok
    }
}
impl <'a, Args, R> Getable<'a> for FunctionRef<'a, Args, R> {
    fn from_value(_: &VM<'a>, value: Value<'a>) -> Option<FunctionRef<'a, Args, R>> {
        Some(FunctionRef { value: value, _marker: PhantomData })//TODO not type safe
    }
}

impl <'a, 'b, A: VMValue<'b>, R: VMValue<'b>> Callable<'a, 'b, (A,), R> {
    pub fn call(&mut self, a: A) -> Result<R, Error> {
        let mut stack = StackFrame::new_empty(self.vm);
        self.value.push(self.vm, &mut stack);
        a.push(self.vm, &mut stack);
        stack = try!(self.vm.execute(stack, &[Call(1)], &BytecodeFunction::empty()));
        match R::from_value(self.vm, stack.pop()) {
            Some(x) => Ok(x),
            None => Err(Error::Message("Wrong type".to_string()))
        }
    }
}
impl <'a, 'b, A: VMValue<'b>, B: VMValue<'b>, R: VMValue<'b>> Callable<'a, 'b, (A, B), R> {
    pub fn call2(&mut self, a: A, b: B) -> Result<R, Error> {
        let mut stack = StackFrame::new_empty(self.vm);
        self.value.push(self.vm, &mut stack);
        a.push(self.vm, &mut stack);
        b.push(self.vm, &mut stack);
        stack = try!(self.vm.execute(stack, &[Call(2)], &BytecodeFunction::empty()));
        match R::from_value(self.vm, stack.pop()) {
            Some(x) => Ok(x),
            None => Err(Error::Message("Wrong type".to_string()))
        }
    }
}

pub fn get_function<'a, 'b, T: Get<'a, 'b>>(vm: &'a VM<'b>, name: &str) -> Option<T> {
    Get::get_function(vm, name)
}


pub trait VMFunction<'a> {
    fn unpack_and_call(&self, vm: &VM<'a>) -> Status;
}
macro_rules! count {
    () => { 0 };
    ($_e: ident) => { 1 };
    ($_e: ident, $($rest: ident),*) => { 1 + count!($($rest),*) }
}

macro_rules! make_vm_function {
    ($($args:ident),*) => (
impl <$($args: VMType,)* R: VMType> VMType for fn ($($args),*) -> R {
    #[allow(non_snake_case)]
    fn vm_type<'r>(vm: &'r VM) -> &'r TcType {
        vm.get_type::<fn ($($args),*) -> R>()
    }
    #[allow(non_snake_case)]
    fn make_type(vm: &VM) -> TcType {
        let args = vec![$(make_type::<$args>(vm)),*];
        Type::Function(args, box make_type::<R>(vm))
    }
}

impl <'a, $($args : VMValue<'a>,)* R: Pushable<'a>> VMFunction<'a> for fn ($($args),*) -> R {
    #[allow(non_snake_case, unused_mut, unused_assignments, unused_variables)]
    fn unpack_and_call(&self, vm: &VM<'a>) -> Status {
        let n_args = count!($($args),*);
        let mut stack = StackFrame::new(vm.stack.borrow_mut(), n_args, None);
        let mut i = 0;
        $(let $args = {
            let x = $args::from_value(vm, stack[i].clone()).unwrap();
            i += 1;
            x
        });*;
        let r = (*self)($($args),*);
        r.push(vm, &mut stack)
    }
}
impl <'a, 's, $($args: VMType,)* R: VMType> VMType for Fn($($args),*) -> R + 's {
    #[allow(non_snake_case)]
    fn vm_type<'r>(vm: &'r VM) -> &'r TcType {
        vm.get_type::<fn ($($args),*) -> R>()
    }
    #[allow(non_snake_case)]
    fn make_type(vm: &VM) -> TcType {
        let args = vec![$(make_type::<$args>(vm)),*];
        Type::Function(args, box make_type::<R>(vm))
    }
}
impl <'a, 's, $($args : VMValue<'a>,)* R: Pushable<'a>> VMFunction<'a> for Box<Fn($($args),*) -> R + 's> {
    #[allow(non_snake_case, unused_mut, unused_assignments, unused_variables)]
    fn unpack_and_call(&self, vm: &VM<'a>) -> Status {
        let n_args = count!($($args),*);
        let mut stack = StackFrame::new(vm.stack.borrow_mut(), n_args, None);
        let mut i = 0;
        $(let $args = {
            let x = $args::from_value(vm, stack[i].clone()).unwrap();
            i += 1;
            x
        });*;
        let r = (*self)($($args),*);
        r.push(vm, &mut stack)
    }
}
    )
}

make_vm_function!();
make_vm_function!(A);
make_vm_function!(A, B);
make_vm_function!(A, B, C);
make_vm_function!(A, B, C, D);
make_vm_function!(A, B, C, D, E);
make_vm_function!(A, B, C, D, E, F);
make_vm_function!(A, B, C, D, E, F, G);

#[macro_export]
macro_rules! vm_function {
    ($func: expr) => ({
        fn wrapper<'a, 'b, 'c>(vm: &VM<'a>) {
            $func.unpack_and_call(vm)
        }
        wrapper
    })
}


pub fn define_function<'a, F>(vm: &VM<'a>, name: &str, f: F) -> VMResult<()>
where F: VMFunction<'a> + VMType + 'static {
    let typ = make_type::<F>(vm);
    let args = match typ {
        Type::Function(ref args, ref return_type) => {
            let io_arg = match **return_type {
                ast::Type::Data(ast::TypeConstructor::Data(name), _) if name == "IO" => 1,
                _ => 0
            };
            io_arg + args.len() as VMIndex
        }
        _ => panic!()
    };
    vm.extern_function_io(name, args, typ, box move |vm| f.unpack_and_call(vm))
}
#[cfg(test)]
mod tests {
    use super::{Get, Callable, define_function};

    use vm::{VM, VMInt, load_script};

    #[test]
    fn call_function() {
        let add10 =
r"
let add10 : Int -> Int = \x -> x #Int+ 10 in add10
";
let mul = r"
let mul : Float -> Float -> Float = \x y -> x #Float* y in mul
";
        let mut vm = VM::new();
        load_script(&mut vm, "add10", &add10)
            .unwrap_or_else(|err| panic!("{}", err));
        load_script(&mut vm, "mul", &mul)
            .unwrap_or_else(|err| panic!("{}", err));
        {
            let mut f: Callable<(VMInt,), VMInt> = Get::get_function(&vm, "add10")
                .expect("No function");
            let result = f.call(2).unwrap();
            assert_eq!(result, 12);
        }
        let mut f: Callable<(f64, f64), f64> = Get::get_function(&vm, "mul")
            .expect("No function");
        let result = f.call2(4., 5.).unwrap();
        assert_eq!(result, 20.);
    }

    #[test]
    fn pass_userdata() {
        let s =
r"
let id : Test -> Test = \x -> x in id
";
        let mut vm = VM::new();
        fn test(test: *mut Test) -> bool {
            let test = unsafe { &mut *test };
            let x = test.x == 123;
            test.x *= 2;
            x
        }
        let test: fn (_) -> _ = test;
        struct Test {
            x: VMInt
        }
        vm.register_type::<Test>("Test")
            .unwrap_or_else(|_| panic!("Could not add type"));
        define_function(&vm, "test", test)
            .unwrap_or_else(|err| panic!("{}", err));
        load_script(&mut vm, "id", &s)
            .unwrap_or_else(|err| panic!("{}", err));

        let mut test = Test { x: 123 };
        {
            let mut f: Callable<(*mut Test,), *mut Test> = Get::get_function(&vm, "id")
                .expect("No function id");
            let result = f.call(&mut test).unwrap();
            let p: *mut _ = &mut test;
            assert!(result == p);
        }
        let mut f: Callable<(*mut Test,), bool> = Get::get_function(&vm, "test")
            .expect("No function test");
        let result = f.call(&mut test).unwrap();
        assert!(result);
        assert_eq!(test.x, 123 * 2);
    }
}
