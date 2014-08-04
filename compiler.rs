use std::collections::HashMap;
use interner::*;
use ast::*;
use typecheck::{Typed, TcIdent, TcType, TypeInfo, Enum, Struct};

#[deriving(Show)]
pub enum Instruction {
    PushInt(int),
    PushFloat(f64),
    Push(uint),
    PushGlobal(uint),
    Store(uint),
    CallGlobal(uint),
    Construct(uint, uint),
    GetField(uint),
    SetField(uint),
    Split,
    TestTag(uint),
    Jump(uint),
    CJump(uint),
    Pop(uint),
    Slide(uint),

    AddInt,
    SubtractInt,
    MultiplyInt,
    IntLT,

    AddFloat,
    SubtractFloat,
    MultiplyFloat,
    FloatLT
}

type CExpr = Expr<TcIdent>;

pub enum Variable {
    Stack(uint),
    Global(uint),
    Constructor(uint, uint)
}

pub struct CompiledFunction {
    pub id: InternedStr,
    pub typ: TcType,
    pub instructions: Vec<Instruction>
}

pub struct Assembly {
    pub functions: Vec<CompiledFunction>,
    pub types: Vec<TypeInfo>
}

pub trait CompilerEnv {
    fn find_var(&self, id: &InternedStr) -> Option<Variable>;
    fn find_field(&self, _struct: &InternedStr, _field: &InternedStr) -> Option<uint> {
        None
    }
    fn find_tag(&self, _: &InternedStr, _: &InternedStr) -> Option<uint> {
        None
    }
}

impl CompilerEnv for () {
    fn find_var(&self, _: &InternedStr) -> Option<Variable> {
        None
    }
}

impl CompilerEnv for HashMap<InternedStr, Variable> {
    fn find_var(&self, s: &InternedStr) -> Option<Variable> {
        self.find(s).map(|x| *x)
    }
}

impl <T: CompilerEnv, U: CompilerEnv> CompilerEnv for (T, U) {
    fn find_var(&self, s: &InternedStr) -> Option<Variable> {
        let &(ref outer, ref inner) = self;
        inner.find_var(s)
            .or_else(|| outer.find_var(s))
    }
    fn find_field(&self, struct_: &InternedStr, field: &InternedStr) -> Option<uint> {
        let &(ref outer, ref inner) = self;
        inner.find_field(struct_, field)
            .or_else(|| outer.find_field(struct_, field))
    }
}

impl CompilerEnv for Module<TcIdent> {
    fn find_var(&self, id: &InternedStr) -> Option<Variable> {
        self.functions.iter()
            .enumerate()
            .find(|&(_, f)| f.name.id() == id)
            .map(|(i, _)| Global(i))
            .or_else(|| self.structs.iter()
                .find(|s| s.name.id() == id)
                .map(|s| Constructor(0, s.fields.len()))
            )
            .or_else(|| {
                for e in self.enums.iter() {
                    let x = e.constructors.iter().enumerate()
                        .find(|&(_, ctor)| ctor.name.id() == id)
                        .map(|(i, ctor)| Constructor(i, ctor.arguments.len()));
                    if x.is_some() {
                        return x
                    }
                }
                None
            })
    }
    fn find_field(&self, struct_: &InternedStr, field: &InternedStr) -> Option<uint> {
        self.structs.iter()
            .find(|s| s.name.id() == struct_)
            .map(|s| s.fields.iter()
                .enumerate()
                .find(|&(_, f)| f.name == *field)
                .map(|(i, _)| i).unwrap())
    }
    fn find_tag(&self, enum_: &InternedStr, ctor_name: &InternedStr) -> Option<uint> {
        self.enums.iter()
            .find(|e| e.name.id() == enum_)
            .map(|e| e.constructors.iter()
                .enumerate()
                .find(|&(_, c)| c.name.id() == ctor_name)
                .map(|(i, _)| i).unwrap())
    }
}

impl <'a, T: CompilerEnv> CompilerEnv for &'a T {
    fn find_var(&self, s: &InternedStr) -> Option<Variable> {
        self.find_var(s)
    }
}

pub struct Compiler<'a> {
    globals: &'a CompilerEnv,
    stack: HashMap<InternedStr, Variable>,
}

impl <'a> Compiler<'a> {

    pub fn new(globals: &'a CompilerEnv) -> Compiler<'a> {
        Compiler { globals: globals, stack: HashMap::new() }
    }

    fn find(&self, s: &InternedStr) -> Option<Variable> {
        self.stack.find_var(s)
            .or_else(||  self.globals.find_var(s))
    }

    fn find_field(&self, struct_: &InternedStr, field: &InternedStr) -> Option<uint> {
        self.stack.find_field(struct_, field)
            .or_else(||  self.globals.find_field(struct_, field))
    }

    fn find_tag(&self, enum_: &InternedStr, constructor: &InternedStr) -> Option<uint> {
        self.stack.find_field(enum_, constructor)
            .or_else(|| self.globals.find_tag(enum_, constructor))
    }

    fn new_stack_var(&mut self, s: InternedStr) {
        let v = Stack(self.stack.len());
        if self.stack.find(&s).is_some() {
            fail!("Variable shadowing is not allowed")
        }
        self.stack.insert(s, v);
    }

    fn stack_size(&self) -> uint {
        self.stack.len()
    }

    pub fn compile_module(&mut self, module: &Module<TcIdent>) -> Assembly {
        let functions = module.functions.iter()
            .map(|f| self.compile_function(f))
            .collect();
        let types = module.structs.iter()
            .map(|s| Struct(s.fields.clone()))
            .chain(module.enums.iter().map(|e| Enum(e.constructors.clone())))
            .collect();
        Assembly { functions: functions, types: types }
    }

    pub fn compile_function(&mut self, function: &Function<TcIdent>) -> CompiledFunction {
        for arg in function.arguments.iter() {
            self.new_stack_var(arg.name);
        }
        let mut instructions = Vec::new();
        self.compile(&function.expression, &mut instructions);
        for arg in function.arguments.iter() {
            self.stack.remove(&arg.name);
        }
        CompiledFunction {
            id: function.name.id().clone(),
            typ: function.type_of().clone(),
            instructions: instructions
        }
    }


    pub fn compile(&mut self, expr: &CExpr, instructions: &mut Vec<Instruction>) {
        match *expr {
            Literal(ref lit) => {
                match *lit {
                    Integer(i) => instructions.push(PushInt(i)),
                    Float(f) => instructions.push(PushFloat(f)),
                    Bool(b) => instructions.push(PushInt(if b { 1 } else { 0 })),
                    String(_) => fail!("String is not implemented")
                }
            }
            Identifier(ref id) => {
                match self.find(id.id()).unwrap_or_else(|| fail!("Undefined variable {}", id.id())) {
                    Stack(index) => instructions.push(Push(index)),
                    Global(index) => instructions.push(PushGlobal(index)),
                    Constructor(..) => fail!("Constructor {} is not fully applied", id)
                }
            }
            IfElse(ref pred, ref if_true, ref if_false) => {
                self.compile(&**pred, instructions);
                let jump_index = instructions.len();
                instructions.push(CJump(0));
                self.compile(&**if_false, instructions);
                let false_jump_index = instructions.len();
                instructions.push(Jump(0));
                *instructions.get_mut(jump_index) = CJump(instructions.len());
                self.compile(&**if_true, instructions);
                *instructions.get_mut(false_jump_index) = Jump(instructions.len());
            }
            Block(ref exprs) => {
                if exprs.len() != 0 {
                    for expr in exprs.slice_to(exprs.len() - 1).iter() {
                        self.compile(expr, instructions);
                        //Since this line is executed as a statement we need to remove
                        //the value from the stack
                        if *expr.type_of() != unit_type {
                            instructions.push(Pop(1));
                        }
                    }
                    let last = exprs.last().unwrap();
                    self.compile(last, instructions);
                }
                let stack_size = self.stack_size();
                for expr in exprs.iter() {
                    match expr {
                        &Let(ref id, _) => {
                            self.stack.remove(id.id());
                        }
                        _ => ()
                    }
                }
                //If the stack has changed size during the block we need to adjust
                //it back to its initial size
                let diff_size = stack_size - self.stack_size();
                if diff_size != 0 {
                    if *expr.type_of() == unit_type {
                        instructions.push(Pop(diff_size));
                    }
                    else {
                        instructions.push(Slide(diff_size));
                    }
                }
                
            }
            BinOp(ref lhs, ref op, ref rhs) => {
                self.compile(&**lhs, instructions);
                self.compile(&**rhs, instructions);
                let typ = lhs.type_of();
                let instr = if *typ == int_type {
                    match op.as_slice() {
                        "+" => AddInt,
                        "-" => SubtractInt,
                        "*" => MultiplyInt,
                        "<" => IntLT,
                        _ => fail!()
                    }
                }
                else if *typ == float_type {
                    match op.as_slice() {
                        "+" => AddFloat,
                        "-" => SubtractFloat,
                        "*" => MultiplyFloat,
                        "<" => FloatLT,
                        _ => fail!()
                    }
                }
                else {
                    fail!()
                };
                instructions.push(instr);
            }
            Let(ref id, ref expr) => {
                self.compile(&**expr, instructions);
                self.new_stack_var(*id.id());
                //unit expressions do not return a value so we need to add a dummy value
                //To make the stack correct
                if *expr.type_of() == unit_type {
                    instructions.push(PushInt(0));
                }
            }
            Call(ref func, ref args) => {
                match **func {
                    Identifier(ref id) => {
                        match self.find(id.id()).unwrap_or_else(|| fail!("Undefined variable {}", id.id())) {
                            Constructor(tag, num_args) => {
                                for arg in args.iter() {
                                    self.compile(arg, instructions);
                                }
                                instructions.push(Construct(tag, num_args));
                                return
                            }
                            _ => ()
                        }
                    }
                    _ => ()
                }
                self.compile(&**func, instructions);
                for arg in args.iter() {
                    self.compile(arg, instructions);
                }
                instructions.push(CallGlobal(args.len()));
            }
            While(ref pred, ref expr) => {
                //jump #test
                //#start:
                //[compile(expr)]
                //#test:
                //[compile(pred)]
                //cjump #start
                let pre_jump_index = instructions.len();
                instructions.push(Jump(0));
                self.compile(&**expr, instructions);
                *instructions.get_mut(pre_jump_index) = Jump(instructions.len());
                self.compile(&**pred, instructions);
                instructions.push(CJump(pre_jump_index + 1));
            }
            Assign(ref lhs, ref rhs) => {
                self.compile(&**rhs, instructions);
                match **lhs {
                    Identifier(ref id) => {
                        let var = self.find(id.id())
                            .unwrap_or_else(|| fail!("Undefined variable {}", id));
                        match var {
                            Stack(i) => instructions.push(Store(i)),
                            Global(_) => fail!("Assignment to global {}", id),
                            Constructor(..) => fail!("Assignment to constructor {}", id)
                        }
                    }
                    FieldAccess(ref expr, ref field) => {
                        self.compile(&**expr, instructions);
                        let field_index = match *expr.type_of() {
                            Type(ref id) => {
                                self.find_field(id, field.id())
                                    .unwrap()
                            }
                            _ => fail!()
                        };
                        instructions.push(SetField(field_index));
                    }
                    _ => fail!("Assignment to {}", lhs)
                }
            }
            FieldAccess(ref expr, ref field) => {
                self.compile(&**expr, instructions);
                let field_index = match *expr.type_of() {
                    Type(ref id) => {
                        self.find_field(id, field.id())
                            .unwrap()
                    }
                    _ => fail!()
                };
                instructions.push(GetField(field_index));
            }
            Match(ref expr, ref alts) => {
                self.compile(&**expr, instructions);
                let mut start_jumps = Vec::new();
                let mut end_jumps = Vec::new();
                let typename = match expr.type_of() {
                    &Type(ref id) => id,
                    _ => fail!()
                };
                for alt in alts.iter() {
                    match alt.pattern {
                        ConstructorPattern(ref id, _) => {
                            let tag = self.find_tag(typename, id.id())
                                .expect("Could not find tag");
                            instructions.push(TestTag(tag));
                            start_jumps.push(instructions.len());
                            instructions.push(CJump(0));
                        }
                        _ => ()
                    }
                }
                for (alt, &start_index) in alts.iter().zip(start_jumps.iter()) {
                    *instructions.get_mut(start_index) = CJump(instructions.len());
                    match alt.pattern {
                        ConstructorPattern(_, ref args) => {
                            instructions.push(Split);
                            for arg in args.iter() {
                                self.new_stack_var(arg.id().clone());
                            }
                        }
                        IdentifierPattern(ref id) => self.new_stack_var(id.id().clone())
                    }
                    self.compile(&alt.expression, instructions);
                    end_jumps.push(instructions.len());
                    instructions.push(Jump(0));

                }
                for &index in end_jumps.iter() {
                    *instructions.get_mut(index) = Jump(instructions.len());
                }
            }
        }
    }
}
