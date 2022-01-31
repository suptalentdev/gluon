use interner::{intern, InternedStr};

#[deriving(Clone, Eq, PartialEq, Show)]
pub enum Type<Id> {
    Type(Id),
    FunctionType(Vec<Type<Id>>, Box<Type<Id>>)
}

#[deriving(Clone, PartialEq, Show)]
pub enum Literal {
    Integer(int),
    Float(f64),
    String(InternedStr),
    Bool(bool)
}

#[deriving(Clone, PartialEq, Show)]
pub enum Pattern<Id> {
    ConstructorPattern(Id, Vec<Id>),
    IdentifierPattern(Id)
}

#[deriving(Clone, PartialEq, Show)]
pub struct Alternative<Id> {
    pub pattern: Pattern<Id>,
    pub expression: Expr<Id>
}

#[deriving(Clone, PartialEq, Show)]
pub enum Expr<Id> {
    Identifier(Id),
    Literal(Literal),
    Call(Box<Expr<Id>>, Vec<Expr<Id>>),
    IfElse(Box<Expr<Id>>, Box<Expr<Id>>, Box<Expr<Id>>),
    While(Box<Expr<Id>>, Box<Expr<Id>>),
    Match(Box<Expr<Id>>, Vec<Alternative<Id>>),
    Block(Vec<Expr<Id>>),
    BinOp(Box<Expr<Id>>, Id, Box<Expr<Id>>),
    Let(Id, Box<Expr<Id>>),
    Assign(Box<Expr<Id>>, Box<Expr<Id>>),
    FieldAccess(Box<Expr<Id>>, Id)
}

#[deriving(Clone, PartialEq, Show)]
pub struct Field<Id> {
    pub name: Id,
    pub typ: Type<Id>
}

#[deriving(Clone, PartialEq, Show)]
pub struct Function<Id> {
    pub name: Id,
    pub arguments: Vec<Field<Id>>,
    pub return_type: Type<Id>,
    pub expression: Expr<Id>
}

#[deriving(Clone, PartialEq, Show)]
pub struct Struct<Id> {
    pub name: Id,
    pub fields: Vec<Field<Id>>
}

#[deriving(Clone, PartialEq, Show)]
pub struct Module<Id> {
    pub functions: Vec<Function<Id>>,
    pub structs: Vec<Struct<Id>>
}

pub fn int_type() -> Type<InternedStr> {
    Type(intern("int"))
}
pub fn float_type() -> Type<InternedStr> {
    Type(intern("float"))
}
pub fn string_type() -> Type<InternedStr> {
    Type(intern("string"))
}
pub fn bool_type() -> Type<InternedStr> {
    Type(intern("bool"))
}
pub fn unit_type() -> Type<InternedStr> {
    Type(intern("()"))
}


