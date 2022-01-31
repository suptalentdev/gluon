use std::rc::Rc;
use std::cell::{RefCell, Ref};
use std::fmt;
use std::intrinsics::{TypeId, get_tydesc};
use std::any::Any;
use std::collections::HashMap;
use ast;
use parser::Parser;
use typecheck::{Typecheck, TypeEnv, TypeInfo, TypeInfos, Typed, STRING_TYPE, INT_TYPE, UNIT_TYPE, TcIdent, TcType, Constrained, match_types};
use ast::TypeEnum::*;
use compiler::*;
use compiler::Instruction::*;
use interner::{Interner, InternedStr};
use gc::{Gc, GcPtr, Traverseable, DataDef};
use fixed::*;

use self::Named::*;
use self::Global_::*;

pub use vm::Value::{
    Int,
    Float,
    String,
    Data,
    Function,
    Closure,
    TraitObject,
    Userdata};

pub struct Userdata_<T> {
    pub data: Rc<RefCell<T>>
}
impl <T> Userdata_<T> {
    pub fn new(v: T) -> Userdata_<T> {
        Userdata_ { data: Rc::new(RefCell::new(v)) }
    }
    fn ptr(&self) -> *const () {
        (&*self.data as *const RefCell<T>) as *const ()
    }
}
impl <T> PartialEq for Userdata_<T> {
    fn eq(&self, o: &Userdata_<T>) -> bool {
        self.ptr() == o.ptr()
    }
}
impl <T> Clone for Userdata_<T> {
    fn clone(&self) -> Userdata_<T> {
        Userdata_ { data: self.data.clone() }
    }
}

pub struct DataStruct<'a> {
    value: GcPtr<Data_<'a>>
}

impl <'a> DataStruct<'a> {

    fn borrow(&self) -> &Data_<'a> {
        & **self
    }
    fn borrow_mut(&self) -> &mut Data_<'a> {
        unsafe { ::std::mem::transmute(& **self) }
    }
}

impl <'a> PartialEq for DataStruct<'a> {
    fn eq(&self, other: &DataStruct<'a>) -> bool {
        self.tag == other.tag && self.fields == other.fields
    }
}

impl <'a> Clone for DataStruct<'a> {
    fn clone(&self) -> DataStruct<'a> {
        DataStruct { value: self.value }
    }
}

impl <'a> Deref<Data_<'a>> for DataStruct<'a> {
    fn deref(&self) -> &Data_<'a> {
        &*self.value
    }
}
impl <'a> DerefMut<Data_<'a>> for DataStruct<'a> {
    fn deref_mut(&mut self) -> &mut Data_<'a> {
        &mut *self.value
    }
}

pub struct Data_<'a> {
    tag: uint,
    fields: [Value<'a>]
}

#[deriving(Clone, PartialEq)]
pub enum Value<'a> {
    Int(int),
    Float(f64),
    String(InternedStr),
    Data(DataStruct<'a>),
    Function(uint),
    Closure(DataStruct<'a>),
    TraitObject(DataStruct<'a>),
    Userdata(Userdata_<Box<Any + 'static>>)
}

impl <'a> Traverseable<Data_<'a>> for Data_<'a> {
    fn traverse(&mut self, func: |&mut Data_<'a>|) {
        self.fields.traverse(func);
    }
}

impl <'a> Traverseable<Data_<'a>> for [Value<'a>] {
    fn traverse(&mut self, func: |&mut Data_<'a>|) {
        for value in self.iter_mut() {
            match *value {
                Data(ref mut data) => func(&mut **data),
                Closure(ref mut data) => func(&mut **data),
                TraitObject(ref mut data) => func(&mut **data),
                _ => ()
            }
        }
    }
}

type Dict = Vec<uint>;

impl <'a> fmt::Show for Value<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Int(i) => write!(f, "{}", i),
            Float(x) => write!(f, "{}f", x),
            String(x) => write!(f, "\"{}\"", x),
            Data(ref data) => {
                let d = data.borrow();
                write!(f, "{{{} {}}}", d.tag, &d.fields)
            }
            Function(i) => write!(f, "<function {}>", i),
            Closure(ref closure) => write!(f, "<Closure {} {}>", closure.tag, &closure.fields),
            TraitObject(ref object) => write!(f, "<{} {}>", object.tag, &object.fields),
            Userdata(ref ptr) => write!(f, "<Userdata {}>", &*ptr.data.borrow() as *const Box<Any>)
        }
    }
}

pub type ExternFunction<'a> = for<'b> fn(&VM<'a>, StackFrame<'a, 'b>);

#[deriving(Show)]
pub struct Global<'a> {
    id: InternedStr,
    typ: Constrained<TcType>,
    value: Global_<'a>
}
enum Global_<'a> {
    Bytecode(Vec<Instruction>),
    Extern(ExternFunction<'a>)
}
impl <'a> Typed for Global<'a> {
    fn type_of(&self) -> &TcType {
        &self.typ.value
    }
}
impl <'a> fmt::Show for Global_<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self { 
            Bytecode(ref is) => write!(f, "Bytecode {}", is),
            Extern(_) => write!(f, "<extern>")
        }
    }
}

enum Named {
    GlobalFn(uint),
    TraitFn(InternedStr, uint),
}

pub struct VM<'a> {
    globals: FixedVec<Global<'a>>,
    trait_indexes: FixedVec<TraitFunctions>,
    type_infos: RefCell<TypeInfos>,
    typeids: FixedMap<TypeId, TcType>,
    interner: RefCell<Interner>,
    names: RefCell<HashMap<InternedStr, Named>>,
    gc: Gc,
}

pub struct VMEnv<'a: 'b, 'b> {
    type_infos: Ref<'b, TypeInfos>,
    trait_indexes: &'b FixedVec<TraitFunctions>,
    globals: &'b FixedVec<Global<'a>>,
    names: Ref<'b, HashMap<InternedStr, Named>>
}

impl <'a, 'b> CompilerEnv for VMEnv<'a, 'b> {
    fn find_var(&self, id: &InternedStr) -> Option<Variable> {
        match self.names.get(id) {
            Some(&GlobalFn(index)) if index < self.globals.len() => {
                let g = &self.globals[index];
                Some(Variable::Global(index, g.typ.constraints.as_slice(), &g.typ.value))
            }
            Some(&TraitFn(trait_index, function_index)) => {
                self.type_infos.traits
                    .get(&trait_index)
                    .and_then(|functions| {
                        if function_index < functions.len() {
                            Some(Variable::TraitFunction(&functions[function_index].ref1().value))
                        }
                        else {
                            None
                        }
                    })
            }
            _ => {
                debug!("#### {} {}", id, self.type_infos.enums);
                self.type_infos.structs.get(id)
                    .map(|&(_, ref fields)| Variable::Constructor(0, fields.len()))
                    .or_else(|| {
                        self.type_infos.enums.values()
                            .flat_map(|ctors| ctors.iter().enumerate())
                            .find(|ctor| ctor.ref1().name.id() == id)
                            .map(|(i, ctor)| Variable::Constructor(i, ctor.arguments.len()))
                    })
            }
        }
    }
    fn find_field(&self, struct_: &InternedStr, field: &InternedStr) -> Option<uint> {
        (*self).find_field(struct_, field)
    }

    fn find_tag(&self, enum_: &InternedStr, ctor_name: &InternedStr) -> Option<uint> {
        match self.type_infos.enums.get(enum_) {
            Some(ctors) => {
                ctors.iter()
                    .enumerate()
                    .find(|&(_, c)| c.name.id() == ctor_name)
                    .map(|(i, _)| i)
            }
            None => None
        }
    }
    fn find_trait_offset(&self, trait_name: &InternedStr, trait_type: &TcType) -> Option<uint> {
        self.trait_indexes
            .find(|func| func.trait_name == *trait_name && match_types(&func.impl_type, trait_type))
            .map(|(_, func)| func.index)
    }
    fn find_trait_function(&self, typ: &TcType, fn_name: &InternedStr) -> Option<TypeResult<uint>> {
        self.names.get(fn_name).and_then(|named| {
            match *named {
                TraitFn(ref trait_name, _) => {
                    match (self.find_object_function(trait_name, fn_name), self.find_trait_offset(trait_name, typ)) {
                        (Some(function_offset), Some(trait_offset)) => {
                            debug!("{} {} {}", function_offset, trait_offset, self.globals.borrow().len());
                            let global_index = function_offset + trait_offset;
                            let global = &self.globals[global_index];
                            Some(TypeResult {
                                constraints: global.typ.constraints.as_slice(),
                                typ: &global.typ.value,
                                value: global_index
                            })
                        }
                        _ => None
                    }
                }
                _ =>  None
            }
        })
    }
    fn find_object_function(&self, trait_name: &InternedStr, fn_name: &InternedStr) -> Option<uint> {
        self.type_infos.traits
            .get(trait_name)
            .and_then(|trait_info| 
                trait_info.iter()
                    .enumerate()
                    .find(|&(_, tup)| tup.ref0() == fn_name)
                    .map(|(i, _)| i)
            )
    }
    fn next_function_index(&self) -> uint {
        self.globals.borrow().len()
    }
}

impl <'a, 'b> TypeEnv for VMEnv<'a, 'b> {
    fn find_type(&self, id: &InternedStr) -> Option<(&[ast::Constraints], &TcType)> {
        match self.names.get(id) {
            Some(&GlobalFn(index)) if index < self.globals.len() => {
                let g = &self.globals[index];
                Some((g.typ.constraints.as_slice(), &g.typ.value))
            }
            Some(&TraitFn(trait_index, function_index)) => {
                self.type_infos.traits
                    .get(&trait_index)
                    .and_then(|functions| {
                        if function_index < functions.len() {
                            let f = functions[function_index].ref1();
                            Some((f.constraints.as_slice(), &f.value))
                        }
                        else {
                            None
                        }
                    })
            }
            _ => {
                self.type_infos.structs.get(id)
                    .map(|type_fields| ([].as_slice(), type_fields.ref0()))
                    .or_else(|| {
                        self.type_infos.enums.values()
                            .flat_map(|ctors| ctors.iter())
                            .find(|ctor| ctor.name.id() == id)
                            .map(|ctor| ([].as_slice(), &ctor.name.typ))
                    })
            }
        }
    }
    fn find_type_info(&self, id: &InternedStr) -> Option<TypeInfo> {
        self.type_infos.find_type_info(id)
    }
}

pub struct StackFrame<'a: 'b, 'b> {
    stack: &'b mut Vec<Value<'a>>,
    offset: uint,
    upvars: &'b mut [Value<'a>]
}
impl <'a: 'b, 'b> StackFrame<'a, 'b> {
    pub fn new(v: &'b mut Vec<Value<'a>>, args: uint, upvars: &'b mut [Value<'a>]) -> StackFrame<'a, 'b> {
        let offset = v.len() - args;
        StackFrame { stack: v, offset: offset, upvars: upvars }
    }

    pub fn len(&self) -> uint {
        self.stack.len() - self.offset
    }

    pub fn get(&self, i: uint) -> &Value<'a> {
        &(*self.stack)[self.offset + i]
    }
    pub fn get_mut(&mut self, i: uint) -> &mut Value<'a> {
        &mut self.stack[self.offset + i]
    }

    pub fn push(&mut self, v: Value<'a>) {
        self.stack.push(v);
    }
    pub fn top(&mut self) -> &Value<'a> {
        self.stack.last().unwrap()
    }

    pub fn pop(&mut self) -> Value<'a> {
        match self.stack.pop() {
            Some(x) => x,
            None => panic!()
        }
    }
    fn as_slice(&self) -> &[Value<'a>] {
        self.stack.slice_from(self.offset)
    }
}

impl <'a, 'b> Index<uint, Value<'a>> for StackFrame<'a, 'b> {
    fn index(&self, index: &uint) -> &Value<'a> {
        &(*self.stack)[self.offset + *index]
    }
}

struct Def<'a:'b, 'b> {
    tag: uint,
    elems: &'b [Value<'a>]
}
impl <'a, 'b> DataDef<Data_<'a>> for Def<'a, 'b> {
    fn size(&self) -> uint {
        use std::mem::size_of;
        size_of::<uint>() + size_of::<Value<'a>>() * self.elems.len()
    }
    fn initialize(&self, result: *mut Data_<'a>) {
        let result = unsafe { &mut *result };
        result.tag = self.tag;
        for (field, value) in result.fields.iter_mut().zip(self.elems.iter()) {
            unsafe {
                ::std::ptr::write(field, value.clone());
            }
        }
    }
    fn make_ptr(&self, ptr: *mut ()) -> *mut Data_<'a> {
        unsafe {
            use std::raw::Slice;
            let x = Slice { data: &*ptr, len: self.elems.len() };
            ::std::mem::transmute(x)
        }
    }
}

impl <'a> VM<'a> {
    
    pub fn new() -> VM<'a> {
        let vm = VM {
            globals: FixedVec::new(),
            trait_indexes: FixedVec::new(),
            type_infos: RefCell::new(TypeInfos::new()),
            typeids: FixedMap::new(),
            interner: RefCell::new(Interner::new()),
            names: RefCell::new(HashMap::new()),
            gc: Gc::new(),
        };
        let a = Generic(0);
        let array_a = ArrayType(box a.clone());
        let _ = vm.extern_function("array_length", vec![array_a.clone()], INT_TYPE.clone(), array_length);
        let _ = vm.extern_function("string_append", vec![STRING_TYPE.clone(), STRING_TYPE.clone()], STRING_TYPE.clone(), string_append);
        vm
    }

    pub fn new_functions(&self, (fns, indexes): (Vec<CompiledFunction>, Vec<TraitFunctions>)) {
        let mut names = self.names.borrow_mut();
        for trait_index in indexes.into_iter() {
            //First index of this impl's functions
            let start_index = trait_index.index - self.globals.len();
            let func = &fns[start_index];
            let is_registered = match names.get(&func.id) {
                Some(&TraitFn(..)) => true,
                None => false,
                _ => panic!()
            };
            if !is_registered {
                match self.type_infos.borrow().traits.get(&trait_index.trait_name) {
                    Some(trait_fns) => {
                        for (i, &(trait_fn, _)) in trait_fns.iter().enumerate() {
                            debug!("Register trait fn {}", trait_fn);
                            names.insert(func.id, TraitFn(trait_index.trait_name, i));
                        }
                    }
                    None => panic!()
                }
            }
            self.trait_indexes.push(trait_index);
        }
        for f in fns.into_iter() {
            let CompiledFunction { id, typ, instructions } = f;
            match names.get(&id) {
                Some(&GlobalFn(..)) => {
                    if id != self.interner.borrow_mut().intern("") {//Lambdas have the empty string as name
                        panic!("ICE: Global {} already exists", id);
                    }
                }
                Some(&TraitFn(..)) => (),
                None => {
                    debug!("Register fn {}", id);
                    names.insert(id, GlobalFn(self.globals.len()));
                }
            }
            self.globals.push(Global { id: id, typ: typ, value: Bytecode(instructions) });
        }
    }

    pub fn get_global(&self, name: &str) -> Option<(uint, &Global<'a>)> {
        let n = self.intern(name);
        self.globals.find(|g| n == g.id)
    }

    pub fn get_type<T: 'static>(&self) -> &TcType {
        let id = TypeId::of::<T>();
        self.typeids.get(&id)
            .unwrap_or_else(|| {
                let desc = unsafe { get_tydesc::<T>() };
                let name = if desc.is_not_null() {
                    unsafe { &*desc }.name
                }
                else {
                    ""
                };
                panic!("Expected type {} to be inserted before get_type call", name)
            })
    }

    pub fn run_function(&self, cf: &Global<'a>) -> Option<Value<'a>> {
        let mut stack = Vec::new();
        {
            let frame = StackFrame::new(&mut stack, 0, [].as_mut_slice());
            self.execute_function(frame, cf);
        }
        stack.pop()
    }

    pub fn execute_instructions(&self, instructions: &[Instruction]) -> Option<Value<'a>> {
        let mut stack = Vec::new();
        {
            let frame = StackFrame::new(&mut stack, 0, [].as_mut_slice());
            self.execute(frame, instructions);
        }
        stack.pop()
    }

    pub fn extern_function(&self, name: &str, args: Vec<TcType>, return_type: TcType, f: ExternFunction<'a>) -> Result<(), ::std::string::String> {
        let id = self.intern(name);
        if self.names.borrow().contains_key(&id) {
            return Err(format!("{} is already defined", name))
        }
        let global = Global {
            id: id,
            typ: Constrained { constraints: Vec::new(), value: FunctionType(args, box return_type) },
            value: Extern(f)
        };
        self.names.borrow_mut().insert(id, GlobalFn(self.globals.len()));
        self.globals.push(global);
        Ok(())
    }

    pub fn register_type<T: 'static>(&mut self, name: &str) -> Result<&TcType, ()> {
        let n = self.intern(name);
        let mut type_infos = self.type_infos.borrow_mut();
        if type_infos.structs.contains_key(&n) {
            Err(())
        }
        else {
            let id = TypeId::of::<T>();
            let typ = Type(n, Vec::new());
            try!(self.typeids.try_insert(id, typ.clone()).map_err(|_| ()));
            let t = self.typeids.get(&id).unwrap();
            type_infos.structs.insert(n, (typ, Vec::new()));
            Ok(t)
        }
    }

    pub fn intern(&self, s: &str) -> InternedStr {
        self.interner.borrow_mut().intern(s)
    }

    pub fn env<'b>(&'b self) -> VMEnv<'a, 'b> {
        VMEnv {
            type_infos: self.type_infos.borrow(),
            trait_indexes: &self.trait_indexes,
            globals: &self.globals,
            names: self.names.borrow()
        }
    }

    fn new_data(&self, tag: uint, fields: &[Value<'a>]) -> Value<'a> {
        Data(DataStruct { value: self.gc.alloc(Def { tag: tag, elems: fields })})
    }
    fn new_data_and_collect(&self, roots: &mut [Value<'a>], tag: uint, fields: &[Value<'a>]) -> DataStruct<'a> {
        DataStruct { value: self.gc.alloc_and_collect(roots, Def { tag: tag, elems: fields })}
    }

    fn execute_function<'b>(&self, stack: StackFrame<'a, 'b>, function: &Global<'a>) {
        match function.value {
            Extern(func) => {
                func(self, stack);
            }
            Bytecode(ref instructions) => {
                self.execute(stack, instructions.as_slice());
            }
        }
    }

    pub fn execute<'b>(&self, mut stack: StackFrame<'a, 'b>, instructions: &[Instruction]) {
        debug!("Enter frame with {}", stack.as_slice());
        let mut index = 0;
        while index < instructions.len() {
            let instr = instructions[index];
            debug!("{}: {}", index, instr);
            match instr {
                Push(i) => {
                    let v = stack.get(i).clone();
                    stack.push(v);
                }
                PushInt(i) => {
                    stack.push(Int(i));
                }
                PushString(s) => {
                    stack.push(String(s));
                }
                PushGlobal(i) => {
                    stack.push(Function(i));
                }
                PushFloat(f) => stack.push(Float(f)),
                Store(i) => {
                    *stack.get_mut(i) = stack.pop();
                }
                CallGlobal(args) => {
                    let function_index = stack.len() - 1 - args;
                    {
                        let mut f = stack.get(function_index).clone();
                        let (function, upvars) = match f {
                            Function(index) => {
                                (&self.globals[index], [].as_mut_slice())
                            }
                            Closure(ref mut closure) => {
                                (&self.globals[closure.tag], closure.fields.as_mut_slice())
                            }
                            x => panic!("Cannot call {}", x)
                        };
                        debug!("Call {} :: {}", function.id, function.typ);
                        let new_stack = StackFrame::new(stack.stack, args, upvars);
                        self.execute_function(new_stack, function);
                    }
                    if stack.len() > function_index + args {
                        //Value was returned
                        let result = stack.pop();
                        debug!("Return {}", result);
                        while stack.len() > function_index {
                            stack.pop();
                        }
                        stack.push(result);
                    }
                    else {
                        while stack.len() > function_index {
                            stack.pop();
                        }
                    }
                }
                Construct(tag, args) => {
                    let d = self.new_data(tag, stack.as_slice().slice_from(stack.len() - args));
                    for _ in range(0, args) {
                        stack.pop();
                    } 
                    stack.push(d);
                }
                GetField(i) => {
                    match stack.pop() {
                        Data(data) => {
                            let v = data.borrow().fields[i].clone();
                            stack.push(v);
                        }
                        x => panic!("GetField on {}", x)
                    }
                }
                SetField(i) => {
                    let value = stack.pop();
                    let data = stack.pop();
                    match data {
                        Data(data) => {
                            data.borrow_mut().fields[i] = value;
                        }
                        _ => panic!()
                    }
                }
                TestTag(tag) => {
                    let x = match *stack.top() {
                        Data(ref data) => if data.borrow().tag == tag { 1 } else { 0 },
                        _ => panic!()
                    };
                    stack.push(Int(x));
                }
                Split => {
                    match stack.pop() {
                        Data(data) => {
                            for field in data.fields.iter() {
                                stack.push(field.clone());
                            }
                        }
                        _ => panic!()
                    }
                }
                Jump(i) => {
                    index = i;
                    continue
                }
                CJump(i) => {
                    match stack.pop() {
                        Int(0) => (),
                        _ => {
                            index = i;
                            continue
                        }
                    }
                }
                Pop(n) => {
                    for _ in range(0, n) {
                        stack.pop();
                    }
                }
                Slide(n) => {
                    let v = stack.pop();
                    for _ in range(0, n) {
                        stack.pop();
                    }
                    stack.push(v);
                }
                GetIndex => {
                    let index = stack.pop();
                    let array = stack.pop();
                    match (array, index) {
                        (Data(array), Int(index)) => {
                            let v = array.borrow_mut().fields[index as uint].clone();
                            stack.push(v);
                        }
                        (x, y) => panic!("{} {}", x, y)
                    }
                }
                SetIndex => {
                    let value = stack.pop();
                    let index = stack.pop();
                    let array = stack.pop();
                    match (array, index) {
                        (Data(array), Int(index)) => {
                            array.borrow_mut().fields[index as uint] = value;
                        }
                        _ => panic!()
                    }
                }
                MakeClosure(fi, n) => {
                    let closure = {
                        let i = stack.stack.len() - n;
                        let (stack_after, args) = stack.stack.split_at_mut(i);
                        args.reverse();
                        Closure(self.new_data_and_collect(stack_after, fi, args))
                    };
                    for _ in range(0, n) {
                        stack.pop();
                    }
                    stack.push(closure);
                }
                PushUpVar(i) => {
                    let v = stack.upvars[i].clone();
                    stack.push(v);
                }
                StoreUpVar(i) => {
                    let v = stack.pop();
                    stack.upvars[i] = v;
                }
                ConstructTraitObject(i) => {
                    let v = stack.pop();
                    let object = TraitObject(self.new_data_and_collect(stack.stack.as_mut_slice(), i, ::std::slice::ref_slice(&v)));
                    stack.push(object);
                }
                PushTraitFunction(i) => {
                    let func = match stack.top() {
                        &TraitObject(ref object) => {
                            Function(object.tag + i)
                        }
                        _ => panic!()
                    };
                    stack.push(func);
                }
                Unpack => {
                    match stack.pop() {
                        TraitObject(ref obj) => stack.push(obj.fields[0].clone()),
                        _ => panic!()
                    }
                }
                PushDictionaryMember(trait_index, function_offset) => {
                    let func = match stack.upvars[0].clone()  {
                        Data(dict) => {
                            match dict.borrow().fields[trait_index] {
                                Function(i) => Function(i + function_offset),
                                _ => panic!()
                            }
                        }
                        ref x => panic!("PushDictionaryMember {}", x)
                    };
                    stack.push(func);
                }
                PushDictionary(index) => {
                    let dict = stack.upvars[0].clone();
                    let dict = match dict {
                        Data(data) => data.borrow().fields[index].clone(),
                        _ => panic!()
                    };
                    stack.push(dict);
                }
                AddInt => binop_int(&mut stack, |l, r| l + r),
                SubtractInt => binop_int(&mut stack, |l, r| l - r),
                MultiplyInt => binop_int(&mut stack, |l, r| l * r),
                IntLT => binop_int(&mut stack, |l, r| if l < r { 1 } else { 0 }),
                IntEQ => binop_int(&mut stack, |l, r| if l == r { 1 } else { 0 }),

                AddFloat => binop_float(&mut stack, |l, r| l + r),
                SubtractFloat => binop_float(&mut stack, |l, r| l - r),
                MultiplyFloat => binop_float(&mut stack, |l, r| l * r),
                FloatLT => binop(&mut stack, |l, r| {
                    match (l, r) {
                        (Float(l), Float(r)) => Int(if l < r { 1 } else { 0 }),
                        _ => panic!()
                    }
                }),
                FloatEQ => binop(&mut stack, |l, r| {
                    match (l, r) {
                        (Float(l), Float(r)) => Int(if l == r { 1 } else { 0 }),
                        _ => panic!()
                    }
                })
            }
            index += 1;
        }
        if stack.len() != 0 {
            debug!("--> {}", stack.top());
        }
        else {
            debug!("--> ()");
        }
    }
}

#[inline]
fn binop(stack: &mut StackFrame, f: |Value, Value| -> Value) {
    let r = stack.pop();
    let l = stack.pop();
    stack.push(f(l, r));
}
#[inline]
fn binop_int(stack: &mut StackFrame, f: |int, int| -> int) {
    binop(stack, |l, r| {
        match (l, r) {
            (Int(l), Int(r)) => Int(f(l, r)),
            (l, r) => panic!("{} `intOp` {}", l, r)
        }
    })
}
#[inline]
fn binop_float(stack: &mut StackFrame, f: |f64, f64| -> f64) {
    binop(stack, |l, r| {
        match (l, r) {
            (Float(l), Float(r)) => Float(f(l, r)),
            (l, r) => panic!("{} `floatOp` {}", l, r)
        }
    })
}

fn array_length(_: &VM, mut stack: StackFrame) {
    match stack.pop() {
        Data(values) => {
            let i = values.borrow().fields.len();
            stack.push(Int(i as int));
        }
        x => panic!("{}", x)
    }
}
fn string_append(vm: &VM, mut stack: StackFrame) {
    match (&stack[0], &stack[1]) {
        (&String(l), &String(r)) => {
            let l = l.as_slice();
            let r = r.as_slice();
            let mut s = ::std::string::String::with_capacity(l.len() + r.len());
            s.push_str(l);
            s.push_str(r);
            let result = vm.interner.borrow_mut().intern(s.as_slice());
            stack.push(String(result));
        }
        _ => panic!()
    }
}

macro_rules! tryf(
    ($e:expr) => (try!(($e).map_err(|e| format!("{}", e))))
)

pub fn parse_expr(buffer: &mut Buffer, vm: &VM) -> Result<::ast::LExpr<TcIdent>, ::std::string::String> {
    let mut interner = vm.interner.borrow_mut();
    let mut parser = Parser::new(&mut *interner, buffer, |s| TcIdent { name: s, typ: UNIT_TYPE.clone() });
    parser.expression().map_err(|err| format!("{}", err))
}

pub fn load_script(vm: &VM, buffer: &mut Buffer) -> Result<(), ::std::string::String> {
    use parser::Parser;

    let mut module = {
        let mut cell = vm.interner.borrow_mut();
        let mut parser = Parser::new(&mut*cell, buffer, |s| TcIdent { typ: UNIT_TYPE.clone(), name: s });
        tryf!(parser.module())
    };
    let (type_infos, functions) = {
        let mut tc = Typecheck::new();
        let env = vm.env();
        tc.add_environment(&env);
        tryf!(tc.typecheck_module(&mut module));
        let env = (vm.env(), &module);
        let mut compiler = Compiler::new(&env);
        (tc.type_infos, compiler.compile_module(&module))
    };
    vm.type_infos.borrow_mut().extend(type_infos);
    vm.new_functions(functions);
    Ok(())
}

pub fn run_main<'a>(vm: &VM<'a>, s: &str) -> Result<Option<Value<'a>>, ::std::string::String> {
    use std::io::BufReader;
    let mut buffer = BufReader::new(s.as_bytes());
    run_buffer_main(vm, &mut buffer)
}
pub fn run_buffer_main<'a>(vm: &VM<'a>, buffer: &mut Buffer) -> Result<Option<Value<'a>>, ::std::string::String> {
    try!(load_script(vm, buffer));
    let v = try!(run_function(vm, "main"));
    Ok(v)
}

pub fn run_function<'a: 'b, 'b>(vm: &'b VM<'a>, name: &str) -> Result<Option<Value<'a>>, ::std::string::String> {
    let globals = vm.globals.borrow();
    let func = match globals.iter().find(|g| g.id.as_slice() == name) {
        Some(f) => &**f,
        None => return Err(format!("Undefined function {}", name))
    };
    Ok(vm.run_function(func))
}

#[cfg(test)]
mod tests {
    use super::{VM, Data, Int, String, run_main, run_function, load_script};
    use std::io::BufReader;
    ///Test that the stack is adjusted correctly after executing expressions as statements
    #[test]
    fn stack_for_block() {
        let text =
r"
fn main() -> int {
    10 + 2;
    let y = {
        let a = 1000;
        let b = 1000;
    };
    let x = {
        let z = 1;
        z + 2
    };
    x = x * 2 + 2;
    x
}
";
        let mut vm = VM::new();
        let value = run_main(&mut vm, text)
            .unwrap_or_else(|err| panic!("{}", err));
        assert_eq!(value, Some(Int(8)));
    }

    #[test]
    fn unpack_enum() {
        let text =
r"
fn main() -> int {
    match A(8) {
        A(x) => { x }
        B(y) => { 0 }
    }
}
enum AB {
    A(int),
    B(float)
}
";
        let mut vm = VM::new();
        let value = run_main(&mut vm, text)
            .unwrap_or_else(|err| panic!("{}", err));
        assert_eq!(value, Some(Int(8)));
    }
    #[test]
    fn call_trait_function() {
        let text =
r"
fn main() -> Vec {
    let x = Vec(1, 2);
    x = add(x, Vec(10, 0));
    x.y = add(x.y, 3);
    x
}
struct Vec {
    x: int,
    y: int
}

trait Add {
    fn add(l: Self, r: Self) -> Self;
}

impl Add for Vec {
    fn add(l: Vec, r: Vec) -> Vec {
        Vec(l.x + r.x, l.y + r.y)
    }
}
impl Add for int {
    fn add(l: int, r: int) -> int {
        l + r
    }
}
";
        let mut vm = VM::new();
        let value = run_main(&mut vm, text)
            .unwrap_or_else(|err| panic!("{}", err));
        match value {
            Some(Data(ref data)) => {
                assert_eq!(&data.fields, [Int(11), Int(5)].as_slice());
            }
            _ => panic!()
        }
    }
    #[test]
    fn pass_function_value() {
        let text = 
r"
fn main() -> int {
    test(lazy)
}
fn lazy() -> int {
    42
}

fn test(f: fn () -> int) -> int {
    f() + 10
}
";
        let mut vm = VM::new();
        let value = run_main(&mut vm, text)
            .unwrap_or_else(|err| panic!("{}", err));
        assert_eq!(value, Some(Int(52)));
    }
    #[test]
    fn arrays() {
        let text = 
r"
fn main() -> [int] {
    let x = [10, 20, 30];
    [1,2, x[2] + 3]
}
";
        let mut vm = VM::new();
        let value = run_main(&mut vm, text)
            .unwrap_or_else(|err| panic!("{}", err));
        match value {
            Some(Data(ref data)) => {
                assert_eq!(&data.fields, [Int(1), Int(2), Int(33)].as_slice());
            }
            _ => panic!()
        }
    }
    #[test]
    fn array_assign() {
        let text = 
r"
fn main() -> int {
    let x = [10, 20, 30];
    x[2] = x[2] + 10;
    x[2]
}
";
        let mut vm = VM::new();
        let value = run_main(&mut vm, text)
            .unwrap_or_else(|err| panic!("{}", err));
        assert_eq!(value, Some(Int(40)));
    }
    #[test]
    fn lambda() {
        let text = 
r"
fn main() -> int {
    let y = 100;
    let f = \x -> {
        y = y + x;
        y + 1
    };
    f(22)
}
";
        let mut vm = VM::new();
        let value = run_main(&mut vm, text)
            .unwrap_or_else(|err| panic!("{}", err));
        assert_eq!(value, Some(Int(123)));
    }

    #[test]
    fn trait_object() {
        let text = 
r"

trait Collection {
    fn len(x: Self) -> int;
}
impl Collection for [int] {
    fn len(x: [int]) -> int {
        array_length(x)
    }
}

fn test(c: Collection) -> int {
    len(c)
}

fn main() -> int {
    test([0, 0, 0])
}
";
        let mut vm = VM::new();
        let value = run_main(&mut vm, text)
            .unwrap_or_else(|err| panic!("{}", err));
        assert_eq!(value, Some(Int(3)));
    }

    #[test]
    fn upvar_index() {
        let text = 
r"
fn main() -> int {
    let x = 100;
    let f = \y -> x + y;
    f(10)
}
";
        let mut vm = VM::new();
        let value = run_main(&mut vm, text)
            .unwrap_or_else(|err| panic!("{}", err));
        assert_eq!(value, Some(Int(110)));
    }

    #[test]
    fn call_generic_constrained_function() {
        let text = 
r"
trait Eq {
    fn eq(l: Self, r: Self) -> bool;
}
enum Option<T> {
    Some(T),
    None()
}
impl Eq for int {
    fn eq(l: int, r: int) -> bool {
        l == r
    }
}
impl <T:Eq> Eq for Option<T> {
    fn eq(l: Option<T>, r: Option<T>) -> bool {
        match l {
            Some(l_val) => {
                match r {
                    Some(r_val) => { eq(l_val, r_val) }
                    None() => { false }
                }
            }
            None() => {
                match r {
                    Some(_) => { false }
                    None() => { true }
                }
            }
        }
    }
}
struct Pair {
    x: bool,
    y: bool
}
fn main() -> Pair {
    let x = eq(Some(2), None());
    let y = eq(Some(1), Some(1));
    Pair(x, y)
}
";
        let mut vm = VM::new();
        let value = run_main(&mut vm, text)
            .unwrap_or_else(|err| panic!("{}", err));
        match value {
            Some(Data(ref data)) => {
                assert_eq!(&data.fields, [Int(0), Int(1)].as_slice());
            }
            _ => panic!()
        }
    }
    #[test]
    fn call_generic_constrained_multi_parameters_function() {
        let text = 
r"
trait Eq {
    fn eq(l: Self, r: Self) -> bool;
}
enum Option<T> {
    Some(T),
    None()
}
impl Eq for int {
    fn eq(l: int, r: int) -> bool {
        l == r
    }
}
impl Eq for float {
    fn eq(l: float, r: float) -> bool {
        l == r
    }
}
impl <T:Eq> Eq for Option<T> {
    fn eq(l: Option<T>, r: Option<T>) -> bool {
        match l {
            Some(l_val) => {
                match r {
                    Some(r_val) => { eq(l_val, r_val) }
                    None() => { false }
                }
            }
            None() => {
                match r {
                    Some(_) => { false }
                    None() => { true }
                }
            }
        }
    }
}
fn test<T: Eq, U: Eq>(opt: Option<T>, x: U, y: U) -> bool {
    if eq(x, y) {
        eq(opt, None())
    }
    else {
        false
    }
}
struct Pair {
    x: bool,
    y: bool
}
fn main() -> Pair {
    let a = None();
    eq(a, Some(1));
    let x = test(a, 1.0, 1.0);
    let y = test(Some(2), 1.0, 1.0);
    Pair(x, y)
}
";
        let mut vm = VM::new();
        let value = run_main(&mut vm, text)
            .unwrap_or_else(|err| panic!("{}", err));
        match value {
            Some(Data(ref data)) => {
                assert_eq!(&data.fields, [Int(1), Int(0)].as_slice());
            }
            _ => panic!()
        }
    }

    #[test]
    fn strings() {
        let text = 
r#"fn main() -> string {
    string_append("Hello", " World")
}"#;
        let mut vm = VM::new();
        let hello_world = vm.intern("Hello World");
        let value = run_main(&mut vm, text)
            .unwrap_or_else(|err| panic!("{}", err));
        assert_eq!(value, Some(String(hello_world)));
    }
    #[test]
    fn call_trait_from_another_script() {
        let mut vm = VM::new();
        {
            let text = 
r"
trait Eq {
    fn eq(l: Self, r: Self) -> bool;
}
impl Eq for int {
    fn eq(l: int, r: int) -> bool {
        l == r
    }
}
impl Eq for float {
    fn eq(l: float, r: float) -> bool {
        l == r
    }
}
";
            let mut buffer = BufReader::new(text.as_bytes());
            load_script(&mut vm, &mut buffer)
                .unwrap_or_else(|e| panic!("{}", e));
        }
        {
            let text = 
r"
fn test<T: Eq>(x: T, y: T) -> bool {
    eq(x, y)
}
fn main() -> bool {
    if eq(1.0, 1.0) {
        test(13, 13)
    }
    else {
        false
    }
}
";
            let mut buffer = BufReader::new(text.as_bytes());
            load_script(&mut vm, &mut buffer)
                .unwrap_or_else(|e| panic!("{}", e));
        }
        let value = run_function(&vm, "main")
            .unwrap_or_else(|err| panic!("{}", err));
        assert_eq!(value, Some(Int(1)));
    }

    #[test]
    fn use_type_from_another_script() {
        let mut vm = VM::new();
        {
            let text = 
r"
enum IntOrFloat {
    I(int),
    F(float)
}
";
            let mut buffer = BufReader::new(text.as_bytes());
            load_script(&mut vm, &mut buffer)
                .unwrap_or_else(|e| panic!("{}", e));
        }
        {
            let text = 
r"
fn main() -> int {
    match F(2.0) {
        I(x) => { x }
        F(x) => { 1 }
    }
}
";
            let mut buffer = BufReader::new(text.as_bytes());
            load_script(&mut vm, &mut buffer)
                .unwrap_or_else(|e| panic!("{}", e));
        }
        let value = run_function(&vm, "main")
            .unwrap_or_else(|err| panic!("{}", err));
        assert_eq!(value, Some(Int(1)));
    }

    #[test]
    fn and_operator() {
        let text = 
r#"
fn main() -> int {
    let x = 0;
    if false && { x = 100; true } {
    }
    else if 0 < x || false {
        x = 200;
    }
    x
}"#;
        let mut vm = VM::new();
        let value = run_main(&mut vm, text)
            .unwrap_or_else(|err| panic!("{}", err));
        assert_eq!(value, Some(Int(0)));
    }
}

