use std::cell::{Cell, RefCell, Ref, RefMut};
use std::fmt;
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::cmp::Ordering;
use std::ops::{Add, Sub, Mul, Div, Deref};
use std::string::String as StdString;
use std::result::Result as StdResult;
use base::ast::{Typed, ASTType, DisplayEnv};
use base::symbol::{Name, NameBuf, Symbol, Symbols};
use base::types;
use base::types::{Type, KindEnv, TypeEnv, TcType, RcKind};
use base::macros::MacroEnv;
use types::*;
use base::fixed::{FixedMap, FixedVec};
use interner::{Interner, InternedStr};
use gc::{Gc, GcPtr, Traverseable, DataDef, Move, WriteOnly};
use array::{Array, Str};
use compiler::{CompiledFunction, Variable, CompilerEnv};
use api::{Getable, Pushable, VMType};
use lazy::Lazy;

use self::Value::{Int, Float, String, Data, Function, PartialApplication, Closure, Userdata};


use stack::{Stack, StackFrame};

#[derive(Copy, Clone, Debug)]
pub struct Userdata_ {
    pub data: GcPtr<Box<Any>>,
}

impl Userdata_ {
    pub fn new<T: Any>(vm: &VM, v: T) -> Userdata_ {
        let v: Box<Any> = Box::new(v);
        Userdata_ { data: vm.gc.borrow_mut().alloc(Move(v)) }
    }
    fn ptr(&self) -> *const () {
        let p: *const _ = &*self.data;
        p as *const ()
    }
}
impl PartialEq for Userdata_ {
    fn eq(&self, o: &Userdata_) -> bool {
        self.ptr() == o.ptr()
    }
}

#[derive(Debug)]
pub struct ClosureData<'a> {
    pub function: GcPtr<BytecodeFunction>,
    pub upvars: Array<Cell<Value<'a>>>,
}

impl<'a> PartialEq for ClosureData<'a> {
    fn eq(&self, _: &ClosureData<'a>) -> bool {
        false
    }
}

impl<'a> Traverseable for ClosureData<'a> {
    fn traverse(&self, gc: &mut Gc) {
        self.function.traverse(gc);
        self.upvars.traverse(gc);
    }
}

pub struct ClosureDataDef<'a: 'b, 'b>(pub GcPtr<BytecodeFunction>, pub &'b [Value<'a>]);
impl<'a, 'b> Traverseable for ClosureDataDef<'a, 'b> {
    fn traverse(&self, gc: &mut Gc) {
        self.0.traverse(gc);
        self.1.traverse(gc);
    }
}

unsafe impl<'a: 'b, 'b> DataDef for ClosureDataDef<'a, 'b> {
    type Value = ClosureData<'a>;
    fn size(&self) -> usize {
        use std::mem::size_of;
        size_of::<GcPtr<BytecodeFunction>>() + Array::<Cell<Value<'a>>>::size_of(self.1.len())
    }
    fn initialize<'w>(self, mut result: WriteOnly<'w, ClosureData<'a>>) -> &'w mut ClosureData<'a> {
        unsafe {
            let result = &mut *result.as_mut_ptr();
            result.function = self.0;
            result.upvars.initialize(self.1.iter().map(|v| Cell::new(v.clone())));
            result
        }
    }
}

#[derive(Debug)]
pub struct BytecodeFunction {
    pub name: Symbol,
    args: VMIndex,
    instructions: Vec<Instruction>,
    inner_functions: Vec<GcPtr<BytecodeFunction>>,
    strings: Vec<InternedStr>,
}

impl BytecodeFunction {
    pub fn new(gc: &mut Gc, f: CompiledFunction) -> GcPtr<BytecodeFunction> {
        let CompiledFunction { id, args, instructions, inner_functions, strings, .. } = f;
        let fs = inner_functions.into_iter()
                                .map(|inner| BytecodeFunction::new(gc, inner))
                                .collect();
        gc.alloc(Move(BytecodeFunction {
            name: id,
            args: args,
            instructions: instructions,
            inner_functions: fs,
            strings: strings,
        }))
    }
}

impl Traverseable for BytecodeFunction {
    fn traverse(&self, gc: &mut Gc) {
        self.inner_functions.traverse(gc);
    }
}

pub struct DataStruct<'a> {
    pub tag: VMTag,
    pub fields: Array<Cell<Value<'a>>>,
}

impl<'a> Traverseable for DataStruct<'a> {
    fn traverse(&self, gc: &mut Gc) {
        self.fields.traverse(gc);
    }
}

impl<'a> PartialEq for DataStruct<'a> {
    fn eq(&self, other: &DataStruct<'a>) -> bool {
        self.tag == other.tag && self.fields == other.fields
    }
}

pub type VMInt = isize;

#[derive(Copy, Clone, PartialEq)]
pub enum Value<'a> {
    Int(VMInt),
    Float(f64),
    String(GcPtr<Str>),
    Data(GcPtr<DataStruct<'a>>),
    Function(GcPtr<ExternFunction<'a>>),
    Closure(GcPtr<ClosureData<'a>>),
    PartialApplication(GcPtr<PartialApplicationData<'a>>),
    Userdata(Userdata_),
    Lazy(GcPtr<Lazy<'a, Value<'a>>>),
}

#[derive(Copy, Clone, Debug)]
pub enum Callable<'a> {
    Closure(GcPtr<ClosureData<'a>>),
    Extern(GcPtr<ExternFunction<'a>>),
}

impl<'a> Callable<'a> {
    pub fn name(&self) -> Symbol {
        match *self {
            Callable::Closure(ref closure) => closure.function.name,
            Callable::Extern(ref ext) => ext.id,
        }
    }

    pub fn args(&self) -> VMIndex {
        match *self {
            Callable::Closure(ref closure) => closure.function.args,
            Callable::Extern(ref ext) => ext.args,
        }
    }
}

impl<'a> PartialEq for Callable<'a> {
    fn eq(&self, _: &Callable<'a>) -> bool {
        false
    }
}

impl<'a> Traverseable for Callable<'a> {
    fn traverse(&self, gc: &mut Gc) {
        match *self {
            Callable::Closure(ref closure) => closure.traverse(gc),
            Callable::Extern(_) => (),
        }
    }
}

#[derive(Debug)]
pub struct PartialApplicationData<'a> {
    function: Callable<'a>,
    arguments: Array<Cell<Value<'a>>>,
}

impl<'a> PartialEq for PartialApplicationData<'a> {
    fn eq(&self, _: &PartialApplicationData<'a>) -> bool {
        false
    }
}

impl<'a> Traverseable for PartialApplicationData<'a> {
    fn traverse(&self, gc: &mut Gc) {
        self.function.traverse(gc);
        self.arguments.traverse(gc);
    }
}

struct PartialApplicationDataDef<'a: 'b, 'b>(Callable<'a>, &'b [Value<'a>]);
impl<'a, 'b> Traverseable for PartialApplicationDataDef<'a, 'b> {
    fn traverse(&self, gc: &mut Gc) {
        self.0.traverse(gc);
        self.1.traverse(gc);
    }
}
unsafe impl<'a: 'b, 'b> DataDef for PartialApplicationDataDef<'a, 'b> {
    type Value = PartialApplicationData<'a>;
    fn size(&self) -> usize {
        use std::mem::size_of;
        size_of::<Callable<'a>>() + Array::<Cell<Value<'a>>>::size_of(self.1.len())
    }
    fn initialize<'w>(self,
                      mut result: WriteOnly<'w, PartialApplicationData<'a>>)
                      -> &'w mut PartialApplicationData<'a> {
        unsafe {
            let result = &mut *result.as_mut_ptr();
            result.function = self.0;
            result.arguments.initialize(self.1.iter().map(|v| Cell::new(v.clone())));
            result
        }
    }
}

impl<'a> PartialEq<Value<'a>> for Cell<Value<'a>> {
    fn eq(&self, other: &Value<'a>) -> bool {
        self.get() == *other
    }
}
impl<'a> PartialEq<Cell<Value<'a>>> for Value<'a> {
    fn eq(&self, other: &Cell<Value<'a>>) -> bool {
        *self == other.get()
    }
}

impl<'a> Traverseable for Value<'a> {
    fn traverse(&self, gc: &mut Gc) {
        match *self {
            String(ref data) => data.traverse(gc),
            Data(ref data) => data.traverse(gc),
            Function(ref data) => data.traverse(gc),
            Closure(ref data) => data.traverse(gc),
            Userdata(ref data) => data.data.traverse(gc),
            PartialApplication(ref data) => data.traverse(gc),
            Value::Lazy(ref lazy) => lazy.traverse(gc),
            Int(_) | Float(_) => (),
        }
    }
}

impl<'a> fmt::Debug for Value<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        struct Level<'a: 'b, 'b>(i32, &'b Value<'a>);
        struct LevelSlice<'a: 'b, 'b>(i32, &'b [Cell<Value<'a>>]);
        impl<'a, 'b> fmt::Debug for LevelSlice<'a, 'b> {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                let level = self.0;
                if level <= 0 {
                    return Ok(());
                }
                for v in self.1 {
                    try!(write!(f, "{:?}", Level(level - 1, &v.get())));
                }
                Ok(())
            }
        }
        impl<'a, 'b> fmt::Debug for Level<'a, 'b> {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                let level = self.0;
                if level <= 0 {
                    return Ok(());
                }
                match *self.1 {
                    Int(i) => write!(f, "{:?}", i),
                    Float(x) => write!(f, "{:?}f", x),
                    String(x) => write!(f, "{:?}", &*x),
                    Data(ref data) => {
                        write!(f,
                               "{{{:?} {:?}}}",
                               data.tag,
                               LevelSlice(level - 1, &data.fields))
                    }
                    Function(ref func) => write!(f, "<EXTERN {:?}>", &**func),
                    Closure(ref closure) => {
                        let p: *const _ = &*closure.function;
                        write!(f,
                               "<{:?} {:?} {:?}>",
                               closure.function.name,
                               p,
                               LevelSlice(level - 1, &closure.upvars))
                    }
                    PartialApplication(ref app) => {
                        let name = match app.function {
                            Callable::Closure(_) => "<CLOSURE>",
                            Callable::Extern(_) => "<EXTERN>",
                        };
                        write!(f,
                               "<App {:?} {:?}>",
                               name,
                               LevelSlice(level - 1, &app.arguments))
                    }
                    Userdata(ref data) => write!(f, "<Userdata {:?}>", data.ptr()),
                    Value::Lazy(_) => write!(f, "<lazy>"),
                }
            }
        }
        write!(f, "{:?}", Level(3, self))
    }
}

macro_rules! get_global {
    ($vm: ident, $i: expr) => (
        match $vm.globals[$i].value.get() {
            x => x
        }
    )
}

/// A rooted value
#[derive(Clone)]
pub struct RootedValue<'a: 'vm, 'vm> {
    vm: &'vm VM<'a>,
    value: Value<'a>,
}

impl<'a, 'vm> Drop for RootedValue<'a, 'vm> {
    fn drop(&mut self) {
        // TODO not safe if the root changes order of being dropped with another root
        self.vm.rooted_values.borrow_mut().pop();
    }
}

impl<'a, 'vm> fmt::Debug for RootedValue<'a, 'vm> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.value)
    }
}

impl<'a, 'vm> Deref for RootedValue<'a, 'vm> {
    type Target = Value<'a>;
    fn deref(&self) -> &Value<'a> {
        &self.value
    }
}

impl<'a, 'vm> RootedValue<'a, 'vm> {
    pub fn vm(&self) -> &'vm VM<'a> {
        self.vm
    }
}

/// A rooted userdata value
pub struct Root<'a, T: ?Sized + 'a> {
    roots: &'a RefCell<Vec<GcPtr<Traverseable + 'static>>>,
    ptr: *const T,
}

impl<'a, T: ?Sized> Drop for Root<'a, T> {
    fn drop(&mut self) {
        // TODO not safe if the root changes order of being dropped with another root
        self.roots.borrow_mut().pop();
    }
}

impl<'a, T: ?Sized> Deref for Root<'a, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.ptr }
    }
}

/// A rooted string
pub struct RootStr<'a>(Root<'a, Str>);

impl<'a> Deref for RootStr<'a> {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}


/// Enum signaling a successful or unsuccess ful call to an extern function.
/// If an error occured the error message is expected to be on the top of the stack.
#[derive(Eq, PartialEq)]
#[repr(C)]
pub enum Status {
    Ok,
    Error,
}

pub struct ExternFunction<'a> {
    pub id: Symbol,
    pub args: VMIndex,
    pub function: Box<Fn(&VM<'a>) -> Status + 'static>,
}

impl<'a> PartialEq for ExternFunction<'a> {
    fn eq(&self, _: &ExternFunction<'a>) -> bool {
        false
    }
}

impl<'a> fmt::Debug for ExternFunction<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // read the v-table pointer of the Fn(..) type and print that
        let p: *const () = unsafe { ::std::mem::transmute_copy(&&*self.function) };
        write!(f, "{:?}", p)
    }
}

impl<'a> Traverseable for ExternFunction<'a> {
    fn traverse(&self, _: &mut Gc) {}
}

#[derive(Debug)]
struct Global<'a> {
    id: Symbol,
    typ: TcType,
    value: Cell<Value<'a>>,
}

impl<'a> Traverseable for Global<'a> {
    fn traverse(&self, gc: &mut Gc) {
        self.value.traverse(gc);
    }
}

impl<'a> Typed for Global<'a> {
    type Id = Symbol;
    fn env_type_of(&self, _: &TypeEnv) -> ASTType<Symbol> {
        self.typ.clone()
    }
}

pub struct GlobalVMState<'a> {
    globals: FixedVec<Global<'a>>,
    type_infos: RefCell<TypeInfos>,
    typeids: FixedMap<TypeId, TcType>,
    pub interner: RefCell<Interner>,
    symbols: RefCell<Symbols>,
    names: RefCell<HashMap<Symbol, usize>>,
    pub gc: RefCell<Gc>,
    macros: MacroEnv<VM<'a>>,
}

/// Representation of the virtual machine
pub struct VM<'a> {
    global_state: GlobalVMState<'a>,
    roots: RefCell<Vec<GcPtr<Traverseable>>>,
    rooted_values: RefCell<Vec<Value<'a>>>,
    stack: RefCell<Stack<'a>>,
}

impl<'a> Deref for VM<'a> {
    type Target = GlobalVMState<'a>;
    fn deref(&self) -> &GlobalVMState<'a> {
        &self.global_state
    }
}

/// Type returned from vm functions which may fail
pub type Result<T> = StdResult<T, Error>;

/// A borrowed structure which implements `CompilerEnv`, `TypeEnv` and `KindEnv` allowing the
/// typechecker and compiler to lookup things in the virtual machine.
#[derive(Debug)]
pub struct VMEnv<'a: 'b, 'b> {
    type_infos: Ref<'b, TypeInfos>,
    globals: &'b FixedVec<Global<'a>>,
    names: Ref<'b, HashMap<Symbol, usize>>,
    io_symbol: Symbol,
    io_arg: [types::Generic<Symbol>; 1],
}

impl<'a, 'b> CompilerEnv for VMEnv<'a, 'b> {
    fn find_var(&self, id: &Symbol) -> Option<Variable> {
        match self.names.get(id) {
            Some(&index) if index < self.globals.len() => {
                let g = &self.globals[index];
                Some(Variable::Global(index as VMIndex, &g.typ))
            }
            _ => self.type_infos.find_var(id),
        }
    }
}

impl<'a, 'b> KindEnv for VMEnv<'a, 'b> {
    fn find_kind(&self, type_name: Symbol) -> Option<RcKind> {
        self.type_infos
            .find_kind(type_name)
            .or_else(|| {
                if type_name == self.io_symbol {
                    Some(types::Kind::function(types::Kind::star(), types::Kind::star()))
                } else {
                    None
                }
            })
    }
}
impl<'a, 'b> TypeEnv for VMEnv<'a, 'b> {
    fn find_type(&self, id: &Symbol) -> Option<&TcType> {
        match self.names.get(id) {
            Some(&index) if index < self.globals.len() => {
                let g = &self.globals[index];
                Some(&g.typ)
            }
            _ => {
                self.type_infos
                    .id_to_type
                    .values()
                    .filter_map(|tuple| {
                        match *tuple.1 {
                            Type::Variants(ref ctors) => {
                                ctors.iter().find(|ctor| ctor.0 == *id).map(|t| &t.1)
                            }
                            _ => None,
                        }
                    })
                    .next()
                    .map(|ctor| ctor)
            }
        }
    }
    fn find_type_info(&self, id: &Symbol) -> Option<(&[types::Generic<Symbol>], Option<&TcType>)> {
        self.type_infos
            .find_type_info(id)
            .or_else(|| {
                if *id == self.io_symbol {
                    Some((&self.io_arg, None))
                } else {
                    None
                }
            })
    }
    fn find_record(&self, fields: &[Symbol]) -> Option<(&TcType, &TcType)> {
        self.type_infos.find_record(fields)
    }
}

/// Definition for data values in the VM
pub struct Def<'a: 'b, 'b> {
    pub tag: VMTag,
    pub elems: &'b [Value<'a>],
}
unsafe impl<'a, 'b> DataDef for Def<'a, 'b> {
    type Value = DataStruct<'a>;
    fn size(&self) -> usize {
        use std::mem::size_of;
        size_of::<usize>() + Array::<Value<'a>>::size_of(self.elems.len())
    }
    fn initialize<'w>(self, mut result: WriteOnly<'w, DataStruct<'a>>) -> &'w mut DataStruct<'a> {
        unsafe {
            let result = &mut *result.as_mut_ptr();
            result.tag = self.tag;
            result.fields.initialize(self.elems.iter().map(|v| Cell::new(v.clone())));
            result
        }
    }
}

impl<'a, 'b> Traverseable for Def<'a, 'b> {
    fn traverse(&self, gc: &mut Gc) {
        self.elems.traverse(gc);
    }
}

struct Roots<'a: 'b, 'b> {
    globals: &'b FixedVec<Global<'a>>,
    stack: &'b Stack<'a>,
    interner: &'b mut Interner,
    roots: Ref<'b, Vec<GcPtr<Traverseable>>>,
    rooted_values: Ref<'b, Vec<Value<'a>>>,
}
impl<'a, 'b> Traverseable for Roots<'a, 'b> {
    fn traverse(&self, gc: &mut Gc) {
        for g in self.globals.borrow().iter() {
            g.traverse(gc);
        }
        self.stack.values.traverse(gc);
        // Also need to check the interned string table
        self.interner.traverse(gc);
        self.roots.traverse(gc);
        self.rooted_values.traverse(gc);
    }
}

impl<'a> GlobalVMState<'a> {
    /// Creates a new virtual machine
    pub fn new() -> GlobalVMState<'a> {
        let vm = GlobalVMState {
            globals: FixedVec::new(),
            type_infos: RefCell::new(TypeInfos::new()),
            typeids: FixedMap::new(),
            symbols: RefCell::new(Symbols::new()),
            interner: RefCell::new(Interner::new()),
            names: RefCell::new(HashMap::new()),
            gc: RefCell::new(Gc::new()),
            macros: MacroEnv::new(),
        };
        vm.add_types()
          .unwrap();
        vm
    }

    fn add_types(&self) -> StdResult<(), (TypeId, TcType)> {
        use api::generic::A;
        use api::Generic;
        let ref ids = self.typeids;
        try!(ids.try_insert(TypeId::of::<()>(), Type::unit()));
        try!(ids.try_insert(TypeId::of::<bool>(), Type::bool()));
        try!(ids.try_insert(TypeId::of::<VMInt>(), Type::int()));
        try!(ids.try_insert(TypeId::of::<f64>(), Type::float()));
        try!(ids.try_insert(TypeId::of::<::std::string::String>(), Type::string()));
        try!(ids.try_insert(TypeId::of::<char>(), Type::char()));
        let ordering = Type::data(types::TypeConstructor::Data(self.symbol("Ordering")),
                                  Vec::new());
        try!(ids.try_insert(TypeId::of::<Ordering>(), ordering));
        let args = vec![types::Generic {
                            id: self.symbol("a"),
                            kind: types::Kind::star(),
                        }];
        let _ = self.register_type::<Lazy<Generic<A>>>("Lazy", args);
        Ok(())
    }

    pub fn new_function(&self, f: CompiledFunction) -> GcPtr<BytecodeFunction> {
        BytecodeFunction::new(&mut self.gc.borrow_mut(), f)
    }

    pub fn get_type<T: ?Sized + Any>(&self) -> &TcType {
        let id = TypeId::of::<T>();
        self.typeids
            .get(&id)
            .unwrap_or_else(|| panic!("Expected type to be inserted before get_type call"))
    }

    /// Checks if a global exists called `name`
    pub fn global_exists(&self, name: &str) -> bool {
        let n = self.symbol(name);
        self.names.borrow().get(&n).is_some()
    }

    /// TODO dont expose this directly
    pub fn set_global(&self, id: Symbol, typ: TcType, value: Value<'a>) -> Result<()> {
        if self.names.borrow().contains_key(&id) {
            return Err(Error::Message(format!("{} is already defined",
                                              self.symbols.borrow().string(&id))));
        }
        let global = Global {
            id: id,
            typ: typ,
            value: Cell::new(value),
        };
        self.names.borrow_mut().insert(id, self.globals.len());
        self.globals.push(global);
        Ok(())
    }

    /// Registers a new type called `name`
    pub fn register_type<T: ?Sized + Any>(&self,
                                          name: &str,
                                          args: Vec<types::Generic<Symbol>>)
                                          -> Result<&TcType> {
        let n = self.symbol(name);
        let mut type_infos = self.type_infos.borrow_mut();
        if type_infos.id_to_type.contains_key(&n) {
            Err(Error::Message(format!("Type '{}' has already been registered", name)))
        } else {
            let id = TypeId::of::<T>();
            let arg_types = args.iter().map(|g| Type::generic(g.clone())).collect();
            let typ: TcType = Type::data(types::TypeConstructor::Data(n), arg_types);
            self.typeids
                .try_insert(id, typ.clone())
                .expect("Id not inserted");
            let t = self.typeids.get(&id).unwrap();
            let ctor = Type::variants(vec![(n, typ.clone())]);
            type_infos.id_to_type.insert(n, (args, ctor.clone()));
            type_infos.type_to_id.insert(ctor, typ);
            Ok(t)
        }
    }

    pub fn get_macros(&self) -> &MacroEnv<VM<'a>> {
        &self.macros
    }

    pub fn get_symbols(&self) -> Ref<Symbols> {
        self.symbols.borrow()
    }

    pub fn get_mut_symbols(&self) -> RefMut<Symbols> {
        self.symbols.borrow_mut()
    }

    pub fn symbol_string(&self, s: Symbol) -> StdString {
        let symbols = self.symbols.borrow();
        StdString::from(symbols.string(&s))
    }

    pub fn symbol<N>(&self, name: N) -> Symbol
        where N: Into<NameBuf> + AsRef<Name>
    {
        let mut symbols = self.symbols.borrow_mut();
        symbols.symbol(name)
    }

    pub fn intern(&self, s: &str) -> InternedStr {
        self.interner.borrow_mut().intern(&mut *self.gc.borrow_mut(), s)
    }

    /// Returns a borrowed structure which implements `CompilerEnv`
    pub fn env<'b>(&'b self) -> VMEnv<'a, 'b> {
        VMEnv {
            type_infos: self.type_infos.borrow(),
            globals: &self.globals,
            names: self.names.borrow(),
            io_symbol: self.symbol("IO"),
            io_arg: [types::Generic {
                         id: self.symbol("a"),
                         kind: types::Kind::star(),
                     }],
        }
    }

    pub fn new_data(&self, tag: VMTag, fields: &[Value<'a>]) -> Value<'a> {
        Data(self.gc.borrow_mut().alloc(Def {
            tag: tag,
            elems: fields,
        }))
    }
}

impl<'a> VM<'a> {
    pub fn new() -> VM<'a> {
        let vm = VM {
            global_state: GlobalVMState::new(),
            stack: RefCell::new(Stack::new()),
            roots: RefCell::new(Vec::new()),
            rooted_values: RefCell::new(Vec::new()),
        };
        // Enter the top level scope
        StackFrame::frame(vm.stack.borrow_mut(), 0, None);
        vm
    }

    /// Creates a new global value at `name`.
    /// Fails if a global called `name` already exists.
    pub fn define_global<T>(&self, name: &str, value: T) -> Result<()>
        where T: Pushable<'a>
    {
        let id = self.symbol(name);
        if self.names.borrow().contains_key(&id) {
            return Err(Error::Message(format!("{} is already defined", name)));
        }
        let (status, value) = {
            let mut stack = self.current_frame();
            let status = value.push(self, &mut stack);
            (status, stack.pop())
        };
        if status == Status::Error {
            return Err(Error::Message(format!("{:?}", value)));
        }
        self.set_global(id, T::make_type(self), value)
    }

    /// Retrieves the global called `name`.
    /// Fails if the global does not exist or it does not have the correct type.
    pub fn get_global<'vm, T>(&'vm self, name: &str) -> Result<T>
        where T: Getable<'a, 'vm> + VMType
    {
        let mut components = Name::new(name).components();
        let global = match components.next() {
            Some(comp) => {
                let comp_id = self.symbol(comp);
                let names = self.names
                                .borrow();
                try!(names.get(&comp_id)
                          .or_else(|| {
                              // We access by the the full name so no components should be left
                              // to walk through
                              for _ in components.by_ref() {
                              }
                              names.get(&self.symbol(name))
                          })
                          .map(|&i| &self.globals[i])
                          .ok_or_else(|| {
                              Error::Message(format!("Could not retrieve global `{}`", name))
                          }))
            }
            None => return Err(Error::Message(format!("'{}' is not a valid name", name))),
        };
        let mut typ = &global.typ;
        let mut value = global.value.get();
        // If there are any remaining components iterate through them, accessing each field
        for comp in components {
            let next = match **typ {
                Type::Record { ref fields, .. } => {
                    let field_name = self.symbol(comp);
                    fields.iter()
                          .enumerate()
                          .find(|&(_, field)| field.name == field_name)
                          .map(|(offset, field)| (offset, &field.typ))
                }
                _ => None,
            };
            let (offset, next_type) = try!(next.ok_or_else(|| {
                Error::Message(format!("'{}' cannot be accessed by the field '{}'",
                                       types::display_type(&*self.symbols.borrow(), &typ),
                                       comp))
            }));
            typ = next_type;
            value = match value {
                Value::Data(data) => data.fields[offset].get(),
                _ => panic!(),
            };
        }

        // Finally check that type of the returned value is correct
        if *typ == T::make_type(self) {
            T::from_value(self, value)
                .ok_or_else(|| Error::Message(format!("Could not retrieve global `{}`", name)))
        } else {
            Err(Error::Message(format!("Could not retrieve global `{}` as the types did not \
                                        match",
                                       name)))
        }
    }

    pub fn find_type_info(&self,
                          name: &str)
                          -> Result<(&[types::Generic<Symbol>], Option<&TcType>)> {
        let name = Name::new(name);
        let mut components = name.module().components();
        let global = match components.next() {
            Some(comp) => {
                let comp_id = self.symbol(comp);
                let names = self.names
                                .borrow();
                try!(names.get(&comp_id)
                          .or_else(|| {
                              // We access by the the full name so no components should be left
                              // to walk through
                              for _ in components.by_ref() {
                              }
                              names.get(&self.symbol(name.module()))
                          })
                          .map(|&i| &self.globals[i])
                          .ok_or_else(|| {
                              Error::Message(format!("Could not retrieve global `{}`", name))
                          }))
            }
            None => return Err(Error::Message(format!("'{}' is not a valid name", name))),
        };

        let mut typ = &global.typ;
        for comp in components {
            let next = match **typ {
                Type::Record { ref fields, .. } => {
                    let field_name = self.symbol(comp);
                    fields.iter()
                          .find(|field| field.name == field_name)
                          .map(|field| &field.typ)
                }
                _ => None,
            };
            typ = try!(next.ok_or_else(|| {
                Error::Message(format!("'{}' cannot be accessed by the field '{}'",
                                       types::display_type(&*self.symbols.borrow(), &typ),
                                       comp))
            }));
        }
        let maybe_type_info = match **typ {
            Type::Record { ref types, .. } => {
                let field_name = self.symbol(name.name());
                types.iter()
                     .find(|field| field.name == field_name)
                     .map(|field| ((&*field.typ.args, Some(&field.typ.typ))))
            }
            _ => None,
        };
        maybe_type_info.ok_or_else(|| {
            Error::Message(format!("'{}' cannot be accessed by the field '{}'",
                                   types::display_type(&*self.symbols.borrow(), typ),
                                   name.name()))
        })
    }


    /// Returns the current stackframe
    pub fn current_frame<'vm>(&'vm self) -> StackFrame<'a, 'vm> {
        let stack = self.stack.borrow_mut();
        StackFrame {
            frame: stack.frames.last().expect("Frame").clone(),
            stack: stack,
        }
    }

    /// Runs a garbage collection.
    pub fn collect(&self) {
        let stack = self.stack.borrow();
        self.with_roots(&stack, |gc, roots| {
            unsafe {
                gc.collect(roots);
            }
        })
    }

    /// Roots a userdata
    pub fn root<T: Any>(&self, v: GcPtr<Box<Any>>) -> Option<Root<T>> {
        match v.downcast_ref::<T>().or_else(|| v.downcast_ref::<Box<T>>().map(|p| &**p)) {
            Some(ptr) => {
                self.roots.borrow_mut().push(v.as_traverseable());
                Some(Root {
                    roots: &self.roots,
                    ptr: ptr,
                })
            }
            None => None,
        }
    }

    /// Roots a string
    pub fn root_string(&self, ptr: GcPtr<Str>) -> RootStr {
        self.roots.borrow_mut().push(ptr.as_traverseable());
        RootStr(Root {
            roots: &self.roots,
            ptr: &*ptr,
        })
    }

    /// Roots a value
    pub fn root_value<'vm>(&'vm self, value: Value<'a>) -> RootedValue<'a, 'vm> {
        self.rooted_values.borrow_mut().push(value);
        RootedValue {
            vm: self,
            value: value,
        }
    }

    /// Allocates a new value from a given `DataDef`.
    /// Takes the stack as it may collect if the collection limit has been reached.
    pub fn alloc<D>(&self, stack: &Stack<'a>, def: D) -> GcPtr<D::Value>
        where D: DataDef + Traverseable
    {
        self.with_roots(stack,
                        |gc, roots| unsafe { gc.alloc_and_collect(roots, def) })
    }

    fn with_roots<F, R>(&self, stack: &Stack<'a>, f: F) -> R
        where F: for<'b> FnOnce(&mut Gc, Roots<'a, 'b>) -> R
    {
        // For this to be safe we require that the received stack is the same one that is in this
        // VM
        assert!(unsafe {
            stack as *const _ as usize >= &self.stack as *const _ as usize &&
            stack as *const _ as usize <= (&self.stack as *const _).offset(1) as usize
        });
        let mut interner = self.interner.borrow_mut();
        let roots = Roots {
            globals: &self.globals,
            stack: stack,
            interner: &mut *interner,
            roots: self.roots.borrow(),
            rooted_values: self.rooted_values.borrow(),
        };
        let mut gc = self.gc.borrow_mut();
        f(&mut gc, roots)
    }

    pub fn add_bytecode(&self,
                        name: &str,
                        typ: TcType,
                        args: VMIndex,
                        instructions: Vec<Instruction>)
                        -> VMIndex {
        let id = self.symbol(name);
        let compiled_fn = CompiledFunction {
            args: args,
            id: id,
            typ: typ.clone(),
            instructions: instructions,
            inner_functions: vec![],
            strings: vec![],
        };
        let f = self.new_function(compiled_fn);
        let closure = self.alloc(&self.stack.borrow(), ClosureDataDef(f, &[]));
        self.names.borrow_mut().insert(id, self.globals.len());
        self.globals.push(Global {
            id: id,
            typ: typ,
            value: Cell::new(Closure(closure)),
        });
        self.globals.len() as VMIndex - 1
    }

    /// Pushes a value to the top of the stack
    pub fn push(&self, v: Value<'a>) {
        self.stack.borrow_mut().push(v)
    }

    /// Removes the top value from the stack
    pub fn pop(&self) -> Value<'a> {
        self.stack
            .borrow_mut()
            .pop()
    }

    ///Calls a module, allowed to to run IO expressions
    pub fn call_module(&self, typ: &TcType, closure: GcPtr<ClosureData<'a>>) -> Result<Value<'a>> {
        let value = try!(self.call_bytecode(closure));
        if let Type::Data(types::TypeConstructor::Data(id), _) = **typ {
            if id == self.symbol("IO") {
                debug!("Run IO {:?}", value);
                self.push(Int(0));// Dummy value to fill the place of the function for TailCall
                self.push(value);
                self.push(Int(0));
                let mut stack = StackFrame::frame(self.stack.borrow_mut(), 2, None);
                stack = try!(self.call_function(stack, 1))
                            .expect("call_module to have the stack remaining");
                let result = stack.pop();
                while stack.len() > 0 {
                    stack.pop();
                }
                stack.exit_scope();
                return Ok(result);
            }
        }
        Ok(value)
    }

    /// Calls a function on the stack.
    /// When this function is called it is expected that the function exists at
    /// `stack.len() - args - 1` and that the arguments are of the correct type
    pub fn call_function<'b>(&'b self,
                             mut stack: StackFrame<'a, 'b>,
                             args: VMIndex)
                             -> Result<Option<StackFrame<'a, 'b>>> {
        stack = try!(self.do_call(stack, args));
        self.execute(stack)
    }

    fn call_bytecode(&self, closure: GcPtr<ClosureData<'a>>) -> Result<Value<'a>> {
        self.push(Closure(closure));
        let stack = StackFrame::frame(self.stack.borrow_mut(), 0, Some(Callable::Closure(closure)));
        try!(self.execute(stack));
        let mut stack = self.stack.borrow_mut();
        Ok(stack.pop())
    }

    fn execute_callable<'b>(&'b self,
                            mut stack: StackFrame<'a, 'b>,
                            function: &Callable<'a>,
                            excess: bool)
                            -> Result<StackFrame<'a, 'b>> {
        match *function {
            Callable::Closure(closure) => {
                stack = stack.enter_scope(closure.function.args, Some(Callable::Closure(closure)));
                stack.frame.excess = excess;
                Ok(stack)
            }
            Callable::Extern(ref ext) => {
                assert!(stack.len() >= ext.args + 1);
                let function_index = stack.len() - ext.args - 1;
                debug!("------- {} {:?}", function_index, &stack[..]);
                Ok(stack.enter_scope(ext.args, Some(Callable::Extern(*ext))))
            }
        }
    }

    fn execute_function<'b>(&'b self,
                            mut stack: StackFrame<'a, 'b>,
                            function: &ExternFunction<'a>)
                            -> Result<StackFrame<'a, 'b>> {
        debug!("CALL EXTERN {}", self.symbols.borrow().string(&function.id));
        // Make sure that the stack is not borrowed during the external function call
        // Necessary since we do not know what will happen during the function call
        drop(stack);
        let status = (function.function)(self);
        stack = self.current_frame();
        let result = stack.pop();
        while stack.len() > 0 {
            debug!("{} {:?}", stack.len(), &stack[..]);
            stack.pop();
        }
        stack = try!(stack.exit_scope()
                          .ok_or_else(|| {
                              Error::Message(StdString::from("Poped the last frame in \
                                                              execute_function"))
                          }));
        stack.pop();// Pop function
        stack.push(result);
        match status {
            Status::Ok => Ok(stack),
            Status::Error => {
                match stack.pop() {
                    String(s) => Err(Error::Message(s.to_string())),
                    _ => Err(Error::Message("Unexpected panic in VM".to_string())),
                }
            }
        }
    }

    fn call_function_with_upvars<'b>(&'b self,
                                     mut stack: StackFrame<'a, 'b>,
                                     args: VMIndex,
                                     required_args: VMIndex,
                                     callable: Callable<'a>)
                                     -> Result<StackFrame<'a, 'b>> {
        debug!("cmp {} {} {:?} {:?}", args, required_args, callable, {
            let function_index = stack.len() - 1 - args;
            &(*stack)[(function_index + 1) as usize..]
        });
        match args.cmp(&required_args) {
            Ordering::Equal => self.execute_callable(stack, &callable, false),
            Ordering::Less => {
                let app = {
                    let fields = &stack[stack.len() - args..];
                    let def = PartialApplicationDataDef(callable, fields);
                    PartialApplication(self.alloc(&stack.stack, def))
                };
                for _ in 0..(args + 1) {
                    stack.pop();
                }
                stack.push(app);
                Ok(stack)
            }
            Ordering::Greater => {
                let excess_args = args - required_args;
                let d = {
                    let fields = &stack[stack.len() - excess_args..];
                    self.alloc(&stack.stack,
                               Def {
                                   tag: 0,
                                   elems: fields,
                               })
                };
                for _ in 0..excess_args {
                    stack.pop();
                }
                // Insert the excess args before the actual closure so it does not get
                // collected
                let offset = stack.len() - required_args - 1;
                stack.insert_slice(offset, &[Cell::new(Data(d))]);
                debug!("xxxxxx {:?}\n{:?}", &(*stack)[..], stack.stack.frames);
                self.execute_callable(stack, &callable, true)
            }
        }
    }

    fn do_call<'b>(&'b self,
                   mut stack: StackFrame<'a, 'b>,
                   args: VMIndex)
                   -> Result<StackFrame<'a, 'b>> {
        let function_index = stack.len() - 1 - args;
        debug!("Do call {:?} {:?}",
               stack[function_index],
               &(*stack)[(function_index + 1) as usize..]);
        match stack[function_index].clone() {
            Function(ref f) => {
                let callable = Callable::Extern(f.clone());
                self.call_function_with_upvars(stack, args, f.args, callable)
            }
            Closure(ref closure) => {
                let callable = Callable::Closure(closure.clone());
                self.call_function_with_upvars(stack, args, closure.function.args, callable)
            }
            PartialApplication(app) => {
                let total_args = app.arguments.len() as VMIndex + args;
                let offset = stack.len() - args;
                stack.insert_slice(offset, &app.arguments);
                self.call_function_with_upvars(stack, total_args, app.function.args(), app.function)
            }
            x => return Err(Error::Message(format!("Cannot call {:?}", x))),
        }
    }

    fn execute<'b>(&'b self, stack: StackFrame<'a, 'b>) -> Result<Option<StackFrame<'a, 'b>>> {
        let mut maybe_stack = Some(stack);
        while let Some(mut stack) = maybe_stack {
            debug!("STACK\n{:?}", stack.stack.frames);
            maybe_stack = match stack.frame.function {
                None => return Ok(Some(stack)),
                Some(Callable::Extern(ext)) => {
                    if stack.frame.instruction_index != 0 {
                        // This function was already called
                        return Ok(Some(stack));
                    } else {
                        stack.frame.instruction_index = 1;
                        Some(try!(self.execute_function(stack, &ext)))
                    }
                }
                Some(Callable::Closure(closure)) => {
                    // Tail calls into extern functions at the top level will drop the last
                    // stackframe so just return immedietly
                    if stack.stack.frames.len() == 0 {
                        return Ok(Some(stack));
                    }
                    let instruction_index = stack.frame.instruction_index;
                    {
                        let symbols = self.symbols.borrow();
                        debug!("Continue with {}\nAt: {}/{}",
                               symbols.string(&closure.function.name),
                               instruction_index,
                               closure.function.instructions.len());
                    }
                    let new_stack = try!(self.execute_(stack,
                                                       instruction_index,
                                                       &closure.function.instructions,
                                                       &closure.function));
                    new_stack
                }
            };
        }
        Ok(maybe_stack)
    }

    fn execute_<'b>(&'b self,
                    mut stack: StackFrame<'a, 'b>,
                    mut index: usize,
                    instructions: &[Instruction],
                    function: &BytecodeFunction)
                    -> Result<Option<StackFrame<'a, 'b>>> {
        {
            let symbols = self.symbols.borrow();
            debug!(">>>\nEnter frame {}: {:?}\n{:?}",
                   symbols.string(&function.name),
                   &stack[..],
                   stack.frame);
        }
        while let Some(&instr) = instructions.get(index) {
            debug_instruction(&stack, index, instr);
            match instr {
                Push(i) => {
                    let v = stack[i].clone();
                    stack.push(v);
                }
                PushInt(i) => {
                    stack.push(Int(i));
                }
                PushString(string_index) => {
                    stack.push(String(function.strings[string_index as usize].inner()));
                }
                PushGlobal(i) => {
                    let x = get_global!(self, i as usize);
                    stack.push(x);
                }
                PushFloat(f) => stack.push(Float(f)),
                Call(args) => {
                    stack.frame.instruction_index = index + 1;
                    return self.do_call(stack, args).map(Some);
                }
                TailCall(mut args) => {
                    let mut amount = stack.len() - args;
                    if stack.frame.excess {
                        amount += 1;
                        let i = stack.stack.values.len() - stack.len() as usize - 2;
                        match stack.stack.values[i] {
                            Data(excess) => {
                                debug!("TailCall: Push excess args {:?}", &excess.fields);
                                for value in &excess.fields {
                                    stack.push(value.get());
                                }
                                args += excess.fields.len() as VMIndex;
                            }
                            _ => panic!("Expected excess args"),
                        }
                    }
                    stack = match stack.exit_scope() {
                        Some(stack) => stack,
                        None => return Ok(None),
                    };
                    debug!("{} {} {:?}", stack.len(), amount, &stack[..]);
                    let end = stack.len() - args - 1;
                    stack.remove_range(end - amount, end);
                    debug!("{:?}", &stack[..]);
                    return self.do_call(stack, args).map(Some);
                }
                Construct(tag, args) => {
                    let d = {
                        let fields = &stack[stack.len() - args..];
                        self.alloc(&stack.stack,
                                   Def {
                                       tag: tag,
                                       elems: fields,
                                   })
                    };
                    for _ in 0..args {
                        stack.pop();
                    }
                    stack.push(Data(d));
                }
                GetField(i) => {
                    match stack.pop() {
                        Data(data) => {
                            let v = data.fields[i as usize].get();
                            stack.push(v);
                        }
                        x => return Err(Error::Message(format!("GetField on {:?}", x))),
                    }
                }
                TestTag(tag) => {
                    let x = match stack.top() {
                        Data(ref data) => {
                            if data.tag == tag {
                                1
                            } else {
                                0
                            }
                        }
                        _ => {
                            return Err(Error::Message("Op TestTag called on non data type"
                                                          .to_string()))
                        }
                    };
                    stack.push(Int(x));
                }
                Split => {
                    match stack.pop() {
                        Data(data) => {
                            for field in data.fields.iter().map(|x| x.get()) {
                                stack.push(field.clone());
                            }
                        }
                        _ => {
                            return Err(Error::Message("Op Split called on non data type"
                                                          .to_string()))
                        }
                    }
                }
                Jump(i) => {
                    index = i as usize;
                    continue;
                }
                CJump(i) => {
                    match stack.pop() {
                        Int(0) => (),
                        _ => {
                            index = i as usize;
                            continue;
                        }
                    }
                }
                Pop(n) => {
                    for _ in 0..n {
                        stack.pop();
                    }
                }
                Slide(n) => {
                    debug!("{:?}", &stack[..]);
                    let v = stack.pop();
                    for _ in 0..n {
                        stack.pop();
                    }
                    stack.push(v);
                }
                GetIndex => {
                    let index = stack.pop();
                    let array = stack.pop();
                    match (array, index) {
                        (Data(array), Int(index)) => {
                            let v = array.fields[index as usize].get();
                            stack.push(v);
                        }
                        (x, y) => {
                            return Err(Error::Message(format!("Op GetIndex called on invalid \
                                                               types {:?} {:?}",
                                                              x,
                                                              y)))
                        }
                    }
                }
                SetIndex => {
                    let value = stack.pop();
                    let index = stack.pop();
                    let array = stack.pop();
                    match (array, index) {
                        (Data(array), Int(index)) => {
                            array.fields[index as usize].set(value);
                        }
                        (x, y) => {
                            return Err(Error::Message(format!("Op SetIndex called on invalid \
                                                               types {:?} {:?}",
                                                              x,
                                                              y)))
                        }
                    }
                }
                MakeClosure(fi, n) => {
                    let closure = {
                        let args = &stack[stack.len() - n..];
                        let func = function.inner_functions[fi as usize];
                        Closure(self.alloc(&stack.stack, ClosureDataDef(func, args)))
                    };
                    for _ in 0..n {
                        stack.pop();
                    }
                    stack.push(closure);
                }
                NewClosure(fi, n) => {
                    let closure = {
                        // Use dummy variables until it is filled
                        let args = [Int(0); 128];
                        let func = function.inner_functions[fi as usize];
                        Closure(self.alloc(&stack.stack, ClosureDataDef(func, &args[..n as usize])))
                    };
                    stack.push(closure);
                }
                CloseClosure(n) => {
                    let i = stack.len() - n - 1;
                    match stack[i] {
                        Closure(closure) => {
                            for var in closure.upvars.iter().rev() {
                                var.set(stack.pop());
                            }
                            stack.pop();//Remove the closure
                        }
                        x => panic!("Expected closure, got {:?}", x),
                    }
                }
                PushUpVar(i) => {
                    let v = stack.get_upvar(i).clone();
                    stack.push(v);
                }
                AddInt => binop(self, &mut stack, VMInt::add),
                SubtractInt => binop(self, &mut stack, VMInt::sub),
                MultiplyInt => binop(self, &mut stack, VMInt::mul),
                DivideInt => binop(self, &mut stack, VMInt::div),
                IntLT => binop(self, &mut stack, |l: VMInt, r| l < r),
                IntEQ => binop(self, &mut stack, |l: VMInt, r| l == r),
                AddFloat => binop(self, &mut stack, f64::add),
                SubtractFloat => binop(self, &mut stack, f64::sub),
                MultiplyFloat => binop(self, &mut stack, f64::mul),
                DivideFloat => binop(self, &mut stack, f64::div),
                FloatLT => binop(self, &mut stack, |l: f64, r| l < r),
                FloatEQ => binop(self, &mut stack, |l: f64, r| l == r),
            }
            index += 1;
        }
        if stack.len() != 0 {
            debug!("--> {:?}", stack.top());
        } else {
            debug!("--> ()");
        }
        let result = stack.pop();
        debug!("Return {:?}", result);
        let len = stack.len();
        for _ in 0..(len + 1) {
            stack.pop();
        }
        if stack.frame.excess {
            match stack.pop() {
                Data(excess) => {
                    debug!("Push excess args {:?}", &excess.fields);
                    stack.push(result);
                    for value in &excess.fields {
                        stack.push(value.get());
                    }
                    stack = match stack.exit_scope() {
                        Some(stack) => stack,
                        None => return Ok(None),
                    };
                    self.do_call(stack, excess.fields.len() as VMIndex).map(Some)
                }
                x => panic!("Expected excess arguments found {:?}", x),
            }
        } else {
            stack.push(result);
            Ok(stack.exit_scope())
        }
    }
}

#[inline]
fn binop<'a, 'b, F, T, R>(vm: &'b VM<'a>, stack: &mut StackFrame<'a, 'b>, f: F)
    where F: FnOnce(T, T) -> R,
          T: Getable<'a, 'b> + fmt::Debug,
          R: Pushable<'a>
{
    let r = stack.pop();
    let l = stack.pop();
    match (T::from_value(vm, l), T::from_value(vm, r)) {
        (Some(l), Some(r)) => {
            let result = f(l, r);
            result.push(vm, stack);
        }
        (l, r) => panic!("{:?} `op` {:?}", l, r),
    }
}

fn debug_instruction(stack: &StackFrame, index: usize, instr: Instruction) {
    debug!("{:?}: {:?} {:?}",
           index,
           instr,
           match instr {
               Push(i) => stack.get(i as usize).cloned(),
               NewClosure(..) => Some(Int(stack.len() as isize)),
               MakeClosure(..) => Some(Int(stack.len() as isize)),
               _ => None,
           });
}

quick_error! {
    #[derive(Debug, PartialEq)]
    pub enum Error {
        Message(err: StdString) {
            display("{}", err)
        }
    }
}
