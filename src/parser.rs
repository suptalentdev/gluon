use std::fmt;
use std::io::BufRead;
use std::marker::PhantomData;
use base::ast::*;
use base::gc::Gc;
use base::interner::{Interner, InternedStr};
use lexer::{Lexer, Token};
use lexer::Token::{
    TInteger,
    TFloat,
    TString,
    TTrue,
    TFalse,
    TIf,
    TElse,
    TWhile,
    TFor,
    TMatch,
    TData,
    TTrait,
    TImpl,
    TVariable,
    TConstructor,
    TOpenBrace,
    TCloseBrace,
    TOpenParen,
    TCloseParen,
    TOpenBracket,
    TCloseBracket,
    TOperator,
    TSemicolon,
    TDot,
    TComma,
    TColon,
    TLet,
    TAssign,
    TRArrow,
    TMatchArrow,
    TLambda,
};

use self::ParseError::*;

macro_rules! expect {
    ($e: expr, $p: ident (..)) => ({
        match *$e.lexer.next() {
            x@$p(..) => x,
            actual => unexpected!($e, actual, $p)
        }
    });
    ($e: expr, $p: ident) => ({
        match *$e.lexer.next() {
            x@$p => x,
            actual => unexpected!($e, actual, $p)
        }
    })
}

macro_rules! expect1 {
    ($e: expr, $p: ident ($x: ident)) => ({
        match *$e.lexer.next() {
            $p($x) => $x,
            actual => unexpected!($e, actual, $p)
        }
    })
}

macro_rules! match_token {
    ($parser: expr { $($p: ident => $e: expr)+ }) => {
        match *$parser.lexer.peek() {
            $($p => $e)+
            token => unexpected!($parser, token, $($p),+)
        }
    }
}

macro_rules! matches {
    ($e: expr, $p: pat) => (
        match $e {
            $p => true,
            _ => false
        }
    )
}

macro_rules! unexpected (
    ($parser: expr, $token: expr, $($expected: expr),+) => { {
        $parser.lexer.backtrack();
        static EXPECTED: &'static [&'static str] = &[$(stringify!($expected)),+];
        return Err($parser.unexpected_token(EXPECTED, $token))
    } }
);

fn precedence(s : &str) -> i32 {
    match s {
        "&&" | "||" => 0,
        "+" => 1,
        "-" => 1,
        "*" => 3,
        "/" => 3,
        "%" => 3,
        "==" => 1,
        "/=" => 1,
        "<" => 1,
        ">" => 1,
        "<=" => 1,
        ">=" => 1,
        _ => 9
    }
}

fn is_statement<T: AstId>(e: &Expr<T>) -> bool {
    match *e {
        Expr::IfElse(..) | Expr::Match(..) | Expr::Block(..) | Expr::While(..) => true,
        _ => false
    }
}

fn is_lvalue<T: AstId>(e: &Expr<T>) -> bool {
    match *e {
        Expr::Identifier(..) | Expr::FieldAccess(..) | Expr::ArrayAccess(..) => true,
        _ => false
    }
}

type PString = InternedStr;

#[derive(Debug)]
enum ParseError {
    UnexpectedToken(&'static [&'static str], Token)
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            UnexpectedToken(expected, actual) => {
                //Type "one of" if there is more than one expected token
                let multiple_tokens = if expected.len() == 1 { "" } else { "one of " };
                try!(write!(f, "Unexpected token {:?}, expected {}", actual, multiple_tokens));
                for (i, token) in expected.iter().enumerate() {
                    //Writes the expected tokens in a list like "tok1" "tok1 or tok2" "tok1, tok2 or tok3"
                    let sep = if i == 0 { "" }
                              else if i == expected.len() - 1 { " or " }
                              else { ", " };
                    try!(write!(f, "{}{}", sep, token));
                }
                Ok(())
            }
        }
    }
}

pub type ParseResult<T> = Result<T, Located<ParseError>>;

pub struct Parser<'a, 'b, PString> {
    lexer: Lexer<'a, 'b>,
    type_variables: Vec<InternedStr>,
    _marker: PhantomData<fn (InternedStr) -> PString>
}

impl <'a, 'b, PString> Parser<'a, 'b, PString>
    where PString: AstId {
    pub fn new(interner: &'a mut Interner, gc: &'a mut Gc, input: &'b mut BufRead) -> Parser<'a, 'b, PString>  {
        Parser {
            lexer: Lexer::new(interner, gc, input),
            type_variables: Vec::new(),
            _marker: PhantomData
        }
    }

    fn make_id(&mut self, s: InternedStr) -> PString {
        AstId::from_str(s)
    }
    fn make_untyped_id(&mut self, s: InternedStr) -> PString::Untyped {
        self.make_id(s).to_id()
    }

    pub fn module(&mut self) -> ParseResult<Module<PString>> {
        let mut globals = Vec::new();
        let mut datas = Vec::new();
        let mut traits = Vec::new();
        let mut impls = Vec::new();
        loop {
            match *self.lexer.peek() {
                TVariable(..) => globals.push(try!(self.global())),
                TData => datas.push(try!(self.data())),
                TTrait => traits.push(try!(self.trait_())),
                TImpl => impls.push(try!(self.impl_())),
                _ => break
            }
            self.type_variables.clear();
        }
        Ok(Module { datas: datas, globals: globals, traits: traits, impls: impls })
    }

    fn statement(&mut self) -> ParseResult<(LExpr<PString>, bool)> {
        let location = self.lexer.location();
        let (expr, is_stm) = try!(match *self.lexer.peek() {
            TLet => {
                self.lexer.next();
                let id = expect1!(self, TVariable(x));
                expect!(self, TAssign);
                let expr = try!(self.expression());
                expect!(self, TSemicolon);
                Ok((Expr::Let(self.make_id(id), box expr, None), true))
            }
            _ => {
                match self.expression() {
                    Ok(e) => {
                        if is_lvalue(&e.value) && matches!(self.lexer.peek(), &TAssign) {
                            self.lexer.next();
                            let rhs = try!(self.expression());
                            expect!(self, TSemicolon);
                            Ok((Expr::Assign(box e, box rhs), true))
                        }
                        else if is_statement(&e.value) {
                            Ok((e.value, true))
                        }
                        else if matches!(self.lexer.peek(), &TSemicolon) {
                            self.lexer.next();
                            Ok((e.value, true))
                        }
                        else {
                            Ok((e.value, false))
                        }
                    }
                    Err(e) => Err(e)
                }
            }
        });
        Ok((Located { location: location, value: expr }, is_stm))
    }

    pub fn expression(&mut self) -> ParseResult<LExpr<PString>> {
        let e = try!(self.located(|this| this.sub_expression()));
        self.binary_expression(e, 0)
    }

    fn sub_expression(&mut self) -> ParseResult<Expr<PString>> {
        let e = try!(self.sub_expression_());
        match self.lexer.peek() {
            &TOpenParen => {
                let args = try!(self.parens(|this|
                    this.sep_by(|t| matches!(t, &TComma), |this| this.expression())
                ));
                Ok(Expr::Call(box e, args))
            }
            &TDot => {
                self.lexer.next();
                let id = expect1!(self, TVariable(x));
                Ok(Expr::FieldAccess(box e, self.make_id(id.clone())))
            }
            &TOpenBracket => {
                self.lexer.next();
                let index = box try!(self.expression());
                expect!(self, TCloseBracket);
                Ok(Expr::ArrayAccess(box e, index))
            }
            _ => Ok(e.value)
        }
    }
    
    fn sub_expression_(&mut self) -> ParseResult<LExpr<PString>> {
        self.located(|this| this.sub_expression_2())
    }
    fn sub_expression_2(&mut self) -> ParseResult<Expr<PString>> {
        match *self.lexer.next() {
            TVariable(id) => {
                Ok(Expr::Identifier(self.make_id(id)))
            }
            TConstructor(id) => {
                Ok(Expr::Identifier(self.make_id(id)))
            }
            TOpenParen => {
                let e = try!(self.expression());
                expect!(self, TCloseParen);
                Ok(e.value)
            }
            TOpenBrace => {
                self.lexer.backtrack();
                self.block().map(|e| e.value)
            }
            TInteger(i) => {
                Ok(Expr::Literal(Integer(i)))
            }
            TFloat(f) => {
                Ok(Expr::Literal(Float(f)))
            }
            TString(s) => {
                Ok(Expr::Literal(String(s)))
            }
            TTrue => Ok(Expr::Literal(Bool(true))),
            TFalse => Ok(Expr::Literal(Bool(false))),
            TIf => {
                let pred = box try!(self.expression());
                let if_true = box try!(self.block());
                let if_false = match *self.lexer.peek() {
                    TElse => {
                        self.lexer.next();
                        Some(box match *self.lexer.peek() {
                            TOpenBrace => try!(self.block()),
                            TIf => try!(self.expression()),
                            x => {
                                static EXPECTED: &'static [&'static str] = &["{", "if"];
                                return Err(self.unexpected_token(EXPECTED, x))
                            }
                        })
                    }
                    _ => None
                };
                Ok(Expr::IfElse(pred, if_true, if_false))
            }
            TWhile => {
                let pred = box try!(self.expression());
                let b = box try!(self.block());
                Ok(Expr::While(pred, b))
            }
            TMatch => {
                let expr = box try!(self.expression());
                let alternatives = try!(self.braces(
                    |this| this.many(|t| *t == TCloseBrace, |this| this.alternative())
                ));
                Ok(Expr::Match(expr, alternatives))
            }
            TOpenBracket => {
                let args = try!(self.sep_by(|t| *t == TComma, |this| this.expression()));
                expect!(self, TCloseBracket);
                let dummy = self.lexer.intern("[]");
                Ok(Expr::Array(ArrayStruct { id: self.make_id(dummy), expressions: args }))
            }
            TLambda => { self.lexer.backtrack(); self.lambda() }
            x => {
                self.lexer.backtrack();
                static EXPECTED: &'static [&'static str] = &["TVariable", "(", "{", "TInteger", "TFloat", "TString", "true", "false", "if", "while", "match", "[", "\\"];
                Err(self.unexpected_token(EXPECTED, x))
            }
        }
    }

    fn lambda(&mut self) -> ParseResult<Expr<PString>> {
        expect!(self, TLambda);
        let args = try!(self.many(|t| *t == TRArrow, |this| {
            let id = expect1!(this, TVariable(x));
            Ok(this.make_id(id))
        }));
        expect!(self, TRArrow);
        let body = box try!(self.expression());
        let s = self.lexer.intern("");
        Ok(Expr::Lambda(LambdaStruct { id: self.make_id(s), free_vars: Vec::new(), arguments: args, body: body }))
    }

    fn block(&mut self) -> ParseResult<LExpr<PString>> {
        self.located(|this| this.block_())
    }
    fn block_(&mut self) -> ParseResult<Expr<PString>> {
        expect!(self, TOpenBrace);
        let mut exprs = Vec::new();
        while !matches!(self.lexer.peek(), &TCloseBrace) {
            let (expr, is_stm) = try!(self.statement());
            exprs.push(expr);
            if !is_stm {
                break
            }
        }
        expect!(self, TCloseBrace);
        Ok(Expr::Block(exprs))
    }

    fn binary_expression(&mut self, mut lhs: LExpr<PString>, min_precedence : i32) -> ParseResult<LExpr<PString>> {
        self.lexer.next();
        loop {
            let location = self.lexer.location();
            let lhs_op;
            let lhs_prec;
            match *self.lexer.current() {
                TOperator(op) => {
                    lhs_prec = precedence(&op);
                    lhs_op = op;
                    if lhs_prec < min_precedence {
                        break
                    }
                }
                _ => break
            };
            debug!("Op {}", lhs_op);

            let mut rhs = try!(self.located(|this| this.sub_expression()));
            self.lexer.next();
            loop {
                let lookahead;
                match *self.lexer.current() {
                    TOperator(op) => {
                        lookahead = precedence(&op);
                        if lookahead < lhs_prec {
                            break
                        }
                        debug!("Inner op {}", op);
                    }
                    _ => break
                }
                self.lexer.backtrack();
                rhs = try!(self.binary_expression(rhs, lookahead));
                self.lexer.next();
            }
            lhs = Located {
                location: location,
                value: Expr::BinOp(box lhs, self.make_id(lhs_op.clone()), box rhs)
            };
        }
        self.lexer.backtrack();
        Ok(lhs)
    }

    fn alternative(&mut self) -> ParseResult<Alternative<PString>> {
        let pattern = try!(self.pattern());
        expect!(self, TMatchArrow);
        let expression = try!(self.block());
        Ok(Alternative { pattern: pattern, expression: expression })
    }

    fn pattern(&mut self) -> ParseResult<Pattern<PString>> {
        match *self.lexer.next() {
            TConstructor(name) => {
                let args = try!(self.parens(|this|
                    this.sep_by(|t| *t == TComma, |this| {
                        let arg = expect1!(this, TVariable(x));
                        Ok(this.make_id(arg))
                    })
                ));
                Ok(ConstructorPattern(self.make_id(name), args))
            }
            TVariable(name) => {
                Ok(IdentifierPattern(self.make_id(name)))
            }
            x => {
                self.lexer.backtrack();
                static EXPECTED: &'static [&'static str] = &["TVariable", "TConstructor"];
                Err(self.unexpected_token(EXPECTED, x))
            }
        }
    }

    fn typ(&mut self) -> ParseResult<Type<PString::Untyped>> {
        self.typ_(false)
    }
    fn typ_(&mut self, is_argument: bool) -> ParseResult<Type<PString::Untyped>> {
        let x = match *self.lexer.next() {
            TConstructor(x) => {
                let ctor = match str_to_primitive_type(&x) {
                    Some(t) => TypeConstructor::Builtin(t),
                    None => TypeConstructor::Data(self.make_untyped_id(x))
                };
                let args = if is_argument {
                    Vec::new()
                }
                else {
                    try!(self.type_arguments())
                };
                match ctor {
                    TypeConstructor::Builtin(b) if args.len() == 0 => {
                        Type::Builtin(b)
                    }
                    _ => Type::Data(ctor, args)
                }
            }
            TVariable(name) => Type::Generic(name),
            TOpenBracket => {
                let t = try!(self.typ());
                expect!(self, TCloseBracket);
                Type::Array(box t)
            }
            TOpenParen => {
                if is_argument == false {
                    let args = try!(self.sep_by(|t| *t == TComma, |this| this.typ()));
                    expect!(self, TCloseParen);
                    if args.len() != 0 || *self.lexer.peek() == TRArrow {
                        expect!(self, TRArrow);
                        let return_type = try!(self.typ());
                        Type::Function(args, box return_type)
                    }
                    else {
                        Type::Builtin(UnitType)
                    }
                }
                else {
                    expect!(self, TCloseParen);
                    Type::Builtin(UnitType)
                }
            }
            x => {
                self.lexer.backtrack();
                static EXPECTED: &'static [&'static str] = &["identifier", "[", "("];
                return Err(self.unexpected_token(EXPECTED, x))
            }
        };
        Ok(x)
    }

    fn type_arguments(&mut self) -> ParseResult<Vec<Type<PString::Untyped>>> {
        let mut result = Vec::new();
        loop {
            let v = match self.typ_(true) {
                Ok(v) => v,
                Err(_) => return Ok(result)
            };
            result.push(v);
        }
    }
    
    fn field(&mut self) -> ParseResult<Field<PString::Untyped>> {
        debug!("Field");
        let name = expect1!(self, TVariable(x));
        expect!(self, TColon);
        let typ = try!(self.typ());
        Ok(Field { name: self.make_id(name).to_id(), typ: typ })
    }

    pub fn data(&mut self) -> ParseResult<Data<PString>> {
        expect!(self, TData);
        let name = expect1!(self, TConstructor(x));
        let constraints = try!(self.many(|t| *t == TAssign, |this| Ok(expect1!(this, TVariable(x)))));
        expect!(self, TAssign);
        let pipe = TOperator(self.lexer.intern("|"));
        let constructors = try!(self.sep_by(
            |t| *t == pipe, |this| this.constructor())
        );
        Ok(Data { name: self.make_untyped_id(name), constraints: constraints, constructors: constructors })
    }
    pub fn constructor(&mut self) -> ParseResult<Constructor<PString>> {
        let name = expect1!(self, TConstructor(x));
        let constructor_type = match *self.lexer.peek() {
            TOpenParen => {
                let arguments = try!(self.parens(
                    |this| this.sep_by(
                        |t| *t == TComma, |this| this.typ()
                    )
                ));
                ConstructorType::Tuple(arguments)
            }
            TOpenBrace => {
                let fields = try!(self.braces(
                    |this| this.sep_by(
                        |t| *t == TComma, |this| this.field()
                    )
                ));
                ConstructorType::Record(fields)
            }
            x => unexpected!(self, x, TOpenParen, TOpenBrace)
        };
        Ok(Constructor { name: self.make_id(name), arguments: constructor_type })
    }

    pub fn trait_(&mut self) -> ParseResult<Trait<PString>> {
        expect!(self, TTrait);
        let name = expect1!(self, TConstructor(x));
        let self_variable = expect1!(self, TVariable(x));
        let declarations = try!(self.braces(|this| this.many(|t| *t == TCloseBrace, |this| {
            let decl = try!(this.global_declaration());
            expect!(this, TSemicolon);
            Ok(decl)
        })));
        Ok(Trait { name: self.make_untyped_id(name), self_variable: self_variable, declarations: declarations })
    }
    pub fn impl_(&mut self) -> ParseResult<Impl<PString>> {
        expect!(self, TImpl);
        let constraints = try!(self.constraints());
        let trait_name = expect1!(self, TConstructor(x));
        expect!(self, TFor);
        let typ = try!(self.typ());
        let globals = try!(self.braces(|this| this.many(|t| *t == TCloseBrace, |this| this.global() )));
        Ok(Impl {
            trait_name: self.make_untyped_id(trait_name),
            constraints: constraints,
            typ: typ,
            globals: globals
        })
    }
    
    fn angle_brackets<F, T>(&mut self, f: F) -> ParseResult<T>
        where F: FnOnce(&mut Parser<PString>) -> ParseResult<T> {
        static EXPECTED_LT: &'static [&'static str] = &["<"];
        static EXPECTED_GT: &'static [&'static str] = &[">"];
        match *self.lexer.peek() {
            TOperator(s) if s == "<" => {
                self.lexer.next();
                let result = try!(f(self));
                match *self.lexer.next() {
                    TOperator(x) if x == ">" => (),
                    x => return Err(self.unexpected_token(EXPECTED_GT, x))
                }
                Ok(result)
            }
            x => Err(self.unexpected_token(EXPECTED_LT, x))
        }
    }

    fn constraints(&mut self) -> ParseResult<Vec<Constraint>> {
        match *self.lexer.peek() {
            TOperator(s) if s == "<" => {
                let vars = try!(self.angle_brackets(|this| this.sep_by(|t| *t == TComma, |this| {
                    let name = expect1!(this, TConstructor(x));
                    let type_variable = expect1!(this, TVariable(x));
                    Ok(Constraint { type_variable: type_variable, name: name })
                })));
                Ok(vars)
            }
            _ => Ok(Vec::new())
        }
    }

    pub fn global(&mut self) -> ParseResult<Global<PString>> {
        let declaration = try!(self.global_declaration());
        expect!(self, TSemicolon);
        let name = expect1!(self, TVariable(x));
        expect!(self, TAssign);
        let expr = try!(match_token!(self {
            TOpenBrace => { self.block() }
            TLambda => {
                self.located(move |this| {
                    let mut lambda = try!(this.lambda());
                    //Give top level lambdas a name, needed to work the the vm's current name lookup
                    if let Expr::Lambda(ref mut l) = lambda {
                        l.id = this.make_id(name);
                    }
                    Ok(lambda)
                })
            }
        }));

        Ok(Global {
            declaration: declaration,
            expression: expr
        })
    }
    pub fn global_declaration(&mut self) -> ParseResult<GlobalDeclaration<PString>> {
        let name = expect1!(self, TVariable(x));
        expect!(self, TColon);
        let constraints = try!(self.constraints());
        let typ = try!(self.typ());
        Ok(GlobalDeclaration {
            name: self.make_untyped_id(name),
            typ: Constrained {
                constraints: constraints,
                value: typ
            }
        })
    }

    fn braces<F, T>(&mut self, f: F) -> ParseResult<T>
        where F: FnOnce(&mut Parser<PString>) -> ParseResult<T> {
        expect!(self, TOpenBrace);
        let x = try!(f(self));
        expect!(self, TCloseBrace);
        Ok(x)
    }

    fn parens<F, T>(&mut self, f: F) -> ParseResult<T>
        where F: FnOnce(&mut Parser<PString>) -> ParseResult<T> {
        expect!(self, TOpenParen);
        let x = try!(f(self));
        expect!(self, TCloseParen);
        Ok(x)
    }

    fn many<P, F, T>(&mut self, mut terminator: P, mut f: F) -> ParseResult<Vec<T>>
        where P: FnMut(&Token) -> bool,
              F: FnMut(&mut Parser<PString>) -> ParseResult<T> {
        let mut result = Vec::new();
        while !terminator(self.lexer.peek()) {
            result.push(try!(f(self)));
        }
        Ok(result)
    }
    fn sep_by<F, S, T>(&mut self, mut sep: S, mut f: F) -> ParseResult<Vec<T>>
        where S: FnMut(&Token) -> bool,
              F: FnMut(&mut Parser<PString>) -> ParseResult<T> {
        let mut result = Vec::new();
        match f(self) {
            Ok(x) => result.push(x),
            Err(_) => return Ok(result)
        }
        while sep(self.lexer.peek()) {
            self.lexer.next();
            let x = try!(f(self));
            result.push(x);
        }
        Ok(result)
    }
    fn located<F, T>(&mut self, f: F) -> ParseResult<Located<T>>
        where F: FnOnce(&mut Parser<PString>) -> ParseResult<T> {
        let location = self.lexer.location();
        let value = try!(f(self));
        Ok(Located { location: location, value: value })
    }

    fn unexpected_token(&self, expected: &'static [&'static str], actual: Token) -> Located<ParseError> {
        Located { location: self.lexer.location(), value: UnexpectedToken(expected, actual) }
    }
}

#[cfg(test)]
pub mod tests {
    use super::{Parser, ParseResult};
    use base::ast::*;
    use std::io::BufReader;
    use base::interner::*;
    use super::super::tests::*;
    
    type PExpr = LExpr<InternedStr>;
    
    fn binop(l: PExpr, s: &str, r: PExpr) -> PExpr {
        no_loc(Expr::BinOp(box l, intern(s), box r))
    }
    fn int(i: i64) -> PExpr {
        no_loc(Expr::Literal(Integer(i)))
    }
    fn let_(s: &str, e: PExpr) -> PExpr {
        no_loc(Expr::Let(intern(s), box e, None))
    }
    fn id(s: &str) -> PExpr {
        no_loc(Expr::Identifier(intern(s)))
    }
    fn field(s: &str, typ: VMType) -> Field<InternedStr> {
        Field { name: intern(s), typ: typ }
    }
    fn typ(s: &str) -> VMType {
        match str_to_primitive_type(s) {
            Some(b) => Type::Builtin(b),
            None => Type::Data(TypeConstructor::Data(intern(s)), Vec::new())
        }
    }
    fn generic(s: &str) -> VMType {
        Type::Generic(intern(s))
    }
    fn call(e: PExpr, args: Vec<PExpr>) -> PExpr {
        no_loc(Expr::Call(box e, args))
    }
    fn if_else(p: PExpr, if_true: PExpr, if_false: PExpr) -> PExpr {
        no_loc(Expr::IfElse(box p, box if_true, Some(box if_false)))
    }

    fn while_(p: PExpr, expr: PExpr) -> PExpr {
        no_loc(Expr::While(box p, box expr))
    }
    fn assign(p: PExpr, rhs: PExpr) -> PExpr {
        no_loc(Expr::Assign(box p, box rhs))
    }
    fn block(xs: Vec<PExpr>) -> PExpr {
        no_loc(Expr::Block(xs))
    }
    fn lambda(name: &str, args: Vec<InternedStr>, body: PExpr) -> PExpr {
        no_loc(Expr::Lambda(LambdaStruct {
            id: intern(name),
            free_vars: Vec::new(),
            arguments: args,
            body: box body 
        }))
    }
    fn type_decl(name: &str, typ: Type<InternedStr>, body: PExpr) -> PExpr {
        no_loc(Expr::Type(intern(name), typ, box body))
    }

    fn bool(b: bool) -> PExpr {
        no_loc(Expr::Literal(Bool(b)))
    }

    pub fn parse_new(s: &str) -> LExpr<InternedStr> {
        let interner = get_local_interner();
        let mut interner = interner.borrow_mut();
        let &mut(ref mut interner, ref mut gc) = &mut *interner;
        let x = ::parser_new::parse_str(gc, interner, s)
            .unwrap_or_else(|err| panic!("{:?}", err));
        x
    }

    #[test]
    fn expression() {
        ::env_logger::init().unwrap();
        let e = parse_new("2 * 3 + 4");
        assert_eq!(e, binop(binop(int(2), "*", int(3)), "+", int(4)));
        let e = parse_new(r#"\x y -> x + y"#);
        assert_eq!(e, lambda("", vec![intern("x"), intern("y")], binop(id("x"), "+", id("y"))));
        let e = parse_new(r#"type Test = Int in 0"#);
        assert_eq!(e, type_decl("Test", typ("Int"), int(0)));
    }

    pub fn parse<F, T>(s: &str, f: F) -> T
        where F: FnOnce(&mut Parser<InternedStr>) -> ParseResult<T> {
        let mut buffer = BufReader::new(s.as_bytes());
        let interner = get_local_interner();
        let mut interner = interner.borrow_mut();
        let &mut(ref mut interner, ref mut gc) = &mut *interner;
        let mut parser = Parser::new(interner, gc, &mut buffer);
        let x = f(&mut parser)
            .unwrap_or_else(|err| panic!("{:?}", err));
        x
    }

    #[test]
    fn operators() {
        let expr = parse("1 / 4 + (2 - 3) * 2", |p| p.expression());
        assert_eq!(expr, binop(binop(int(1), "/", int(4)), "+", binop(binop(int(2), "-", int(3)), "*", int(2))));
    }
    #[test]
    fn block_test() {
        let expr = parse("1 / { let x = 2; x }", |p| p.expression());
        assert_eq!(expr, binop(int(1), "/", block(vec!(let_("x", int(2)), id("x")))));
    }
    #[test]
    fn function() {
        let s =
r"
main : (Int,Float) -> ();
main = \x y -> { }";
        let func = parse(s, |p| p.global());
        let expected = Global {
            declaration: GlobalDeclaration {
                name: intern("main"),
                typ: Constrained {
                    constraints: Vec::new(),
                    value: Type::Function(vec!(INT_TYPE.clone(), FLOAT_TYPE.clone()), box UNIT_TYPE.clone())
                }
            },
            expression: lambda("main", vec![intern("x"), intern("y")], block(vec!()))
        };
        assert_eq!(func, expected);
    }
    #[test]
    fn generic_function() {
        let func = parse(
r"
id : (a) -> a;
id = \x -> { x }
", |p| p.global());
        let a = Type::Generic(intern("a"));
        let expected = Global {
            declaration: GlobalDeclaration {
                name: intern("id"),
                typ: Constrained {
                    constraints: Vec::new(),
                    value: Type::Function(vec![a.clone()], box a.clone())
                }
            },
            expression: lambda("id", vec![intern("x")], block(vec![id("x")]))
        };
        assert_eq!(func, expected);
    }
    #[test]
    fn call_function() {
        let expr = parse("test(1, x)", |p| p.expression());
        assert_eq!(expr, call(id("test"), vec![int(1), id("x")]));
    }
    #[test]
    fn test_if_else() {
        let expr = parse("if 1 < x { 1 } else { 0 }", |p| p.expression());
        assert_eq!(expr, if_else(binop(int(1), "<", id("x")), block(vec![int(1)]), block(vec![int(0)])));
    }
    #[test]
    fn test_while() {
        let expr = parse("while true { }", |p| p.expression());
        assert_eq!(expr, while_(bool(true), block(vec![])));
    }
    #[test]
    fn test_assign() {
        let expr = parse("{ y = 2; 2 }", |p| p.expression());
        assert_eq!(expr, block(vec![assign(id("y"), int(2)), int(2)]));
    }
    #[test]
    fn data() {
        let module = parse("data Test = Test { y: Int, f: Float }", |p| p.data());
        let expected = Data {
            name: intern("Test"),
            constraints: Vec::new(),
            constructors: vec![Constructor {
                name: intern("Test"),
                arguments: ConstructorType::Record(vec![field("y", INT_TYPE.clone()), field("f", FLOAT_TYPE.clone())])
            }]
        };
        assert_eq!(module, expected);
    }
    #[test]
    fn trait_() {
        let module = parse(
r"
trait Test a {
    test : (a) -> Int;
    test2 : (Int, a) -> ();
}", |p| p.trait_());
        let expected = Trait {
            name: intern("Test"),
            self_variable: intern("a"),
            declarations: vec![
                GlobalDeclaration {
                    name: intern("test"),
                    typ: Constrained {
                        constraints: Vec::new(),
                        value: Type::Function(vec![generic("a")], box INT_TYPE.clone())
                    }
                },
                GlobalDeclaration {
                    name: intern("test2"),
                    typ: Constrained {
                        constraints: Vec::new(),
                        value: Type::Function(vec![INT_TYPE.clone(), generic("a")], box UNIT_TYPE.clone())
                    }
                },
            ]
        };
        assert_eq!(module, expected);
    }
    #[test]
    fn impl_() {
        parse(
r"
impl Test for Int {
    test : (Int) -> Int;
    test = \x -> { x }

    test2 : (Int, Int) -> ();
    test2 = \x y -> { x + y; }
}
", |p| p.impl_());
    }

    #[test]
    fn function_type() {
        let typ = parse("() -> (Int) -> Float", |p| p.typ());
        assert_eq!(typ, Type::Function(Vec::new(), box Type::Function(vec![INT_TYPE.clone()], box FLOAT_TYPE.clone())));
    }

    #[test]
    fn create_lambda() {
        parse(
r"
main : () -> (int) -> float;
main = \ -> {
    \x -> 1.0
}", |p| p.global());
    }
    #[test]
    fn parameterized_types() {
        parse(
r"data Option a = Some(a) |None()

data Named a = Named {
    name: String,
    value: a
}

trait Test a { }

test : <Test a> (a) -> Option a;
test = \x -> {
    Some(x)
}

", |p| p.module());
    }
    #[test]
    fn global_variable() {
        let text = 
r#"

global : Int;
global = { 123 }

"#;
        let module = parse(text, |p| p.module());
        assert_eq!(module.globals[0], Global {
            declaration: GlobalDeclaration {
                name: intern("global"),
                typ: Constrained { constraints: Vec::new(), value: INT_TYPE.clone() }
            },
            expression: block(vec![int(123)])
        });
    }
}
