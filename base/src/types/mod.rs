use std::{
    borrow::{Borrow, Cow},
    cell::RefCell,
    cmp::Ordering,
    fmt,
    hash::{Hash, Hasher},
    iter::{self, FusedIterator},
    marker::PhantomData,
    mem,
    ops::{Deref, DerefMut},
    rc::Rc,
    sync::Arc,
};

use pretty::{Arena, Doc, DocAllocator, DocBuilder};

use smallvec::SmallVec;

use itertools::Itertools;

use crate::{
    ast::{Commented, EmptyEnv, IdentEnv},
    fnv::FnvMap,
    kind::{ArcKind, Kind, KindCache, KindEnv},
    merge::merge,
    metadata::Comment,
    pos::{BytePos, HasSpan, Span},
    source::Source,
    symbol::{Name, Symbol, SymbolRef},
};

#[cfg(feature = "serde")]
use crate::{
    serde::{de::DeserializeState, ser::SerializeState},
    serialization::{SeSeed, Seed},
};

use self::pretty_print::Printer;
pub use self::{
    flags::Flags,
    pretty_print::{Filter, TypeFormatter},
};

mod flags;
pub mod pretty_print;

macro_rules! forward_eq_hash {
    (<$($param: ident),*> for $typ: ty { $field: ident }) => {
        impl<$($param),*> Eq for $typ where $($param : Eq),* {}

        impl<$($param),*> PartialEq for $typ
            where $($param : PartialEq),*
        {
            fn eq(&self, other: &Self) -> bool {
                self.$field == other.$field
            }
        }

        impl<$($param),*> Hash for $typ
        where $($param : Hash),*
        {
            fn hash<H>(&self, state: &mut H)
            where
                H: std::hash::Hasher,
            {
                self.$field.hash(state)
            }
        }
    }
}

/// Trait for values which contains typed values which can be refered by name
pub trait TypeEnv: KindEnv {
    type Type;
    /// Returns the type of the value bound at `id`
    fn find_type(&self, id: &SymbolRef) -> Option<Self::Type>;

    /// Returns information about the type `id`
    fn find_type_info(&self, id: &SymbolRef) -> Option<Alias<Symbol, Self::Type>>;
}

impl<'a, T: ?Sized + TypeEnv> TypeEnv for &'a T {
    type Type = T::Type;

    fn find_type(&self, id: &SymbolRef) -> Option<Self::Type> {
        (**self).find_type(id)
    }

    fn find_type_info(&self, id: &SymbolRef) -> Option<Alias<Symbol, Self::Type>> {
        (**self).find_type_info(id)
    }
}

impl TypeEnv for EmptyEnv<Symbol> {
    type Type = ArcType;

    fn find_type(&self, _id: &SymbolRef) -> Option<ArcType> {
        None
    }

    fn find_type_info(&self, _id: &SymbolRef) -> Option<Alias<Symbol, ArcType>> {
        None
    }
}

/// Trait which is a `TypeEnv` which also provides access to the type representation of some
/// primitive types
pub trait PrimitiveEnv: TypeEnv {
    fn get_bool(&self) -> ArcType;
}

impl<'a, T: ?Sized + PrimitiveEnv> PrimitiveEnv for &'a T {
    fn get_bool(&self) -> ArcType {
        (**self).get_bool()
    }
}

type_cache! {
    TypeCache(Id, T)
    (kind_cache: crate::kind::KindCache)
    { T, Type }
    hole opaque error int byte float string char
    function_builtin array_builtin unit empty_row
}

impl<Id, T> TypeCache<Id, T>
where
    T: From<Type<Id, T>> + Clone,
{
    pub fn function<I>(&self, args: I, ret: T) -> T
    where
        I: IntoIterator<Item = T>,
        I::IntoIter: DoubleEndedIterator<Item = T>,
    {
        args.into_iter().rev().fold(ret, |body, arg| {
            T::from(Type::Function(ArgType::Explicit, arg, body))
        })
    }

    pub fn function_implicit<I>(&self, args: I, ret: T) -> T
    where
        I: IntoIterator<Item = T>,
        I::IntoIter: DoubleEndedIterator<Item = T>,
    {
        args.into_iter().rev().fold(ret, |body, arg| {
            T::from(Type::Function(ArgType::Implicit, arg, body))
        })
    }

    pub fn tuple<S, I>(&self, symbols: &mut S, elems: I) -> T
    where
        S: ?Sized + IdentEnv<Ident = Id>,
        I: IntoIterator<Item = T>,
    {
        let fields: Vec<_> = elems
            .into_iter()
            .enumerate()
            .map(|(i, typ)| Field {
                name: symbols.from_str(&format!("_{}", i)),
                typ,
            })
            .collect();
        if fields.is_empty() {
            self.unit()
        } else {
            self.record(vec![], fields)
        }
    }

    pub fn variant(&self, fields: Vec<Field<Id, T>>) -> T {
        self.poly_variant(fields, self.empty_row())
    }

    pub fn poly_variant(&self, fields: Vec<Field<Id, T>>, rest: T) -> T {
        Type::poly_variant(fields, rest)
    }

    pub fn record(&self, types: Vec<Field<Id, Alias<Id, T>>>, fields: Vec<Field<Id, T>>) -> T {
        Type::poly_record(types, fields, self.empty_row())
    }

    pub fn effect(&self, fields: Vec<Field<Id, T>>) -> T {
        self.poly_effect(fields, self.empty_row())
    }

    pub fn poly_effect(&self, fields: Vec<Field<Id, T>>, rest: T) -> T {
        Type::poly_effect(fields, rest)
    }

    pub fn array(&self, typ: T) -> T {
        Type::app(self.array_builtin(), collect![typ])
    }
}

impl<Id, T> TypeCache<Id, T>
where
    T: Clone,
{
    pub fn builtin_type(&self, typ: BuiltinType) -> T {
        match typ {
            BuiltinType::String => self.string(),
            BuiltinType::Byte => self.byte(),
            BuiltinType::Char => self.char(),
            BuiltinType::Int => self.int(),
            BuiltinType::Float => self.float(),
            BuiltinType::Array => self.array_builtin(),
            BuiltinType::Function => self.function_builtin(),
        }
    }
}

/// All the builtin types of gluon
#[derive(Copy, Clone, Eq, PartialEq, Debug, Hash)]
#[cfg_attr(feature = "serde_derive", derive(Deserialize, Serialize))]
pub enum BuiltinType {
    /// Unicode string
    String,
    /// Unsigned byte
    Byte,
    /// Character
    Char,
    /// Integer number
    Int,
    /// Floating point number
    Float,
    /// Type constructor for arrays, `Array a : Type -> Type`
    Array,
    /// Type constructor for functions, `(->) a b : Type -> Type -> Type`
    Function,
}

impl BuiltinType {
    pub fn symbol(self) -> &'static SymbolRef {
        SymbolRef::new(self.to_str())
    }
}

impl ::std::str::FromStr for BuiltinType {
    type Err = ();
    fn from_str(x: &str) -> Result<BuiltinType, ()> {
        let t = match x {
            "Int" => BuiltinType::Int,
            "Byte" => BuiltinType::Byte,
            "Float" => BuiltinType::Float,
            "String" => BuiltinType::String,
            "Char" => BuiltinType::Char,
            "Array" => BuiltinType::Array,
            "->" => BuiltinType::Function,
            _ => return Err(()),
        };
        Ok(t)
    }
}

impl BuiltinType {
    pub fn to_str(self) -> &'static str {
        match self {
            BuiltinType::String => "String",
            BuiltinType::Byte => "Byte",
            BuiltinType::Char => "Char",
            BuiltinType::Int => "Int",
            BuiltinType::Float => "Float",
            BuiltinType::Array => "Array",
            BuiltinType::Function => "->",
        }
    }
}

#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde_derive", derive(DeserializeState, SerializeState))]
#[cfg_attr(feature = "serde_derive", serde(serialize_state = "SeSeed"))]
#[cfg_attr(feature = "serde_derive", serde(deserialize_state = "Seed<Id, T>"))]
#[cfg_attr(feature = "serde_derive", serde(de_parameters = "Id, T"))]
pub struct TypeVariable {
    #[cfg_attr(feature = "serde_derive", serde(state))]
    pub kind: ArcKind,
    pub id: u32,
}

forward_eq_hash! { <> for TypeVariable { id } }

#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde_derive", derive(DeserializeState, SerializeState))]
#[cfg_attr(feature = "serde_derive", serde(deserialize_state = "Seed<Id, T>"))]
#[cfg_attr(
    feature = "serde_derive",
    serde(bound(deserialize = "
           Id: DeserializeState<'de, Seed<Id, T>> + Clone + ::std::any::Any"))
)]
#[cfg_attr(feature = "serde_derive", serde(de_parameters = "T"))]
#[cfg_attr(feature = "serde_derive", serde(serialize_state = "SeSeed"))]
#[cfg_attr(
    feature = "serde_derive",
    serde(bound(serialize = "Id: SerializeState<SeSeed>"))
)]
pub struct Skolem<Id> {
    #[cfg_attr(feature = "serde_derive", serde(state))]
    pub name: Id,
    pub id: u32,
    #[cfg_attr(feature = "serde_derive", serde(state))]
    pub kind: ArcKind,
}

forward_eq_hash! { <Id> for Skolem<Id> { id } }

/// FIXME Distinguish generic id's so we only need to compare them by `id` (currently they will get
/// the wrong kind assigned to them otherwise)
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
#[cfg_attr(feature = "serde_derive", derive(DeserializeState, SerializeState))]
#[cfg_attr(feature = "serde_derive", serde(deserialize_state = "Seed<Id, T>"))]
#[cfg_attr(
    feature = "serde_derive",
    serde(bound(deserialize = "
           Id: DeserializeState<'de, Seed<Id, T>> + Clone + ::std::any::Any"))
)]
#[cfg_attr(feature = "serde_derive", serde(de_parameters = "T"))]
#[cfg_attr(feature = "serde_derive", serde(serialize_state = "SeSeed"))]
#[cfg_attr(
    feature = "serde_derive",
    serde(bound(serialize = "Id: SerializeState<SeSeed>"))
)]
pub struct Generic<Id> {
    #[cfg_attr(feature = "serde_derive", serde(state))]
    pub id: Id,
    #[cfg_attr(feature = "serde_derive", serde(state))]
    pub kind: ArcKind,
}

impl<Id> Generic<Id> {
    pub fn new(id: Id, kind: ArcKind) -> Generic<Id> {
        Generic { id, kind }
    }
}

/// An alias is wrapper around `Type::Alias`, allowing it to be cheaply converted to a type and
/// dereferenced to `AliasRef`
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
#[cfg_attr(feature = "serde_derive", derive(DeserializeState, SerializeState))]
#[cfg_attr(feature = "serde_derive", serde(deserialize_state = "Seed<Id, T>"))]
#[cfg_attr(
    feature = "serde_derive",
    serde(bound(deserialize = "
           T: DeserializeState<'de, Seed<Id, T>> + Clone + From<Type<Id, T>> + ::std::any::Any,
           Id: DeserializeState<'de, Seed<Id, T>> + Clone + ::std::any::Any"))
)]
#[cfg_attr(feature = "serde_derive", serde(serialize_state = "SeSeed"))]
#[cfg_attr(
    feature = "serde_derive",
    serde(bound(serialize = "T: SerializeState<SeSeed>"))
)]
pub struct Alias<Id, T> {
    #[cfg_attr(feature = "serde_derive", serde(state))]
    _typ: T,
    #[cfg_attr(feature = "serde_derive", serde(skip))]
    _marker: PhantomData<Id>,
}

impl<Id, T> Deref for Alias<Id, T>
where
    T: Deref<Target = Type<Id, T>>,
{
    type Target = AliasRef<Id, T>;

    fn deref(&self) -> &Self::Target {
        match *self._typ {
            Type::Alias(ref alias) => alias,
            _ => unreachable!(),
        }
    }
}

impl<Id, T> DerefMut for Alias<Id, T>
where
    T: DerefMut<Target = Type<Id, T>>,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        match *self._typ {
            Type::Alias(ref mut alias) => alias,
            _ => unreachable!(),
        }
    }
}

impl<Id, T> From<AliasData<Id, T>> for Alias<Id, T>
where
    T: From<Type<Id, T>>,
{
    fn from(data: AliasData<Id, T>) -> Alias<Id, T> {
        Alias {
            _typ: Type::alias(data.name, data.args, data.typ),
            _marker: PhantomData,
        }
    }
}

impl<Id, T> From<AliasRef<Id, T>> for Alias<Id, T>
where
    T: From<Type<Id, T>>,
{
    fn from(data: AliasRef<Id, T>) -> Alias<Id, T> {
        Alias {
            _typ: Type::Alias(data).into(),
            _marker: PhantomData,
        }
    }
}

impl<Id, T> AsRef<T> for Alias<Id, T> {
    fn as_ref(&self) -> &T {
        &self._typ
    }
}

impl<Id, T> Alias<Id, T>
where
    T: From<Type<Id, T>>,
{
    pub fn new(name: Id, args: Vec<Generic<Id>>, typ: T) -> Alias<Id, T> {
        Alias {
            _typ: Type::alias(name, args, typ),
            _marker: PhantomData,
        }
    }

    pub fn group(group: Vec<AliasData<Id, T>>) -> Vec<Alias<Id, T>> {
        let group = Arc::<[_]>::from(group);
        (0..group.len())
            .map(|index| Alias {
                _typ: T::from(Type::Alias(AliasRef {
                    index,
                    group: group.clone(),
                })),
                _marker: PhantomData,
            })
            .collect()
    }
}

impl<Id, T> Alias<Id, T> {
    pub fn as_type(&self) -> &T {
        &self._typ
    }

    pub fn into_type(self) -> T {
        self._typ
    }
}

impl<Id, T> Alias<Id, T>
where
    T: TypeExt<Id = Id> + Clone,
    Id: Clone + PartialEq,
{
    /// Returns the actual type of the alias
    pub fn typ(&self, interner: &mut impl TypeContext<Id, T>) -> Cow<T> {
        match *self._typ {
            Type::Alias(ref alias) => alias.typ(interner),
            _ => unreachable!(),
        }
    }

    pub fn unpack_canonical_alias<'b>(
        &'b self,
        canonical_alias_type: &'b T,
        mut un_alias: impl FnMut(&'b AliasRef<Id, T>) -> Cow<'b, T>,
    ) -> (Cow<'b, [Generic<Id>]>, &'b AliasRef<Id, T>, Cow<'b, T>) {
        match **canonical_alias_type {
            Type::App(ref func, _) => match **func {
                Type::Alias(ref alias) => (Cow::Borrowed(&[]), alias, un_alias(alias)),
                _ => (Cow::Borrowed(&[]), &**self, Cow::Borrowed(func)),
            },
            Type::Alias(ref alias) => (Cow::Borrowed(&[]), alias, un_alias(alias)),
            Type::Forall(ref params, ref typ) => {
                let mut params = Cow::Borrowed(&params[..]);
                let (more_params, canonical_alias, inner_type) =
                    self.unpack_canonical_alias(typ, un_alias);
                if !more_params.is_empty() {
                    params.to_mut().extend(more_params.iter().cloned());
                }
                (params, canonical_alias, inner_type)
            }
            _ => (
                Cow::Borrowed(&[]),
                &**self,
                Cow::Borrowed(canonical_alias_type),
            ),
        }
    }
}

/// Data for a type alias. Probably you want to use `Alias` instead of this directly as Alias allows
/// for cheap conversion back into a type as well.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde_derive", derive(DeserializeState, SerializeState))]
#[cfg_attr(feature = "serde_derive", serde(deserialize_state = "Seed<Id, T>"))]
#[cfg_attr(
    feature = "serde_derive",
    serde(bound(deserialize = "
           T: DeserializeState<'de, Seed<Id, T>> + Clone + From<Type<Id, T>> + ::std::any::Any,
           Id: DeserializeState<'de, Seed<Id, T>> + Clone + ::std::any::Any"))
)]
#[cfg_attr(feature = "serde_derive", serde(serialize_state = "SeSeed"))]
#[cfg_attr(
    feature = "serde_derive",
    serde(bound(serialize = "T: SerializeState<SeSeed>, Id: SerializeState<SeSeed>"))
)]
pub struct AliasRef<Id, T> {
    /// Name of the Alias
    index: usize,
    #[cfg_attr(
        feature = "serde_derive",
        serde(deserialize_state_with = "crate::serialization::deserialize_group")
    )]
    #[cfg_attr(
        feature = "serde_derive",
        serde(serialize_state_with = "crate::serialization::shared::serialize")
    )]
    /// The other aliases defined in this group
    pub group: Arc<[AliasData<Id, T>]>,
}

impl<Id, T> Eq for AliasRef<Id, T> where AliasData<Id, T>: Eq {}
impl<Id, T> PartialEq for AliasRef<Id, T>
where
    AliasData<Id, T>: PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        **self == **other
    }
}
impl<Id, T> Hash for AliasRef<Id, T>
where
    AliasData<Id, T>: Hash,
{
    fn hash<H>(&self, state: &mut H)
    where
        H: std::hash::Hasher,
    {
        (**self).hash(state)
    }
}

impl<Id, T> AliasRef<Id, T> {
    pub fn try_get_alias_mut(&mut self) -> Option<&mut AliasData<Id, T>> {
        let index = self.index;
        Arc::get_mut(&mut self.group).map(|group| &mut group[index])
    }
    pub(crate) fn try_get_alias(&self) -> Option<&AliasData<Id, T>> {
        let index = self.index;
        Some(&self.group[index])
    }

    #[doc(hidden)]
    pub fn new(index: usize, group: Arc<[AliasData<Id, T>]>) -> Self {
        AliasRef { index, group }
    }

    #[doc(hidden)]
    pub fn index(&self) -> usize {
        self.index
    }
}

impl<Id, T> AliasRef<Id, T>
where
    T: TypeExt<Id = Id> + Clone,
    Id: Clone + PartialEq,
{
    pub fn typ(&self, interner: &mut impl TypeContext<Id, T>) -> Cow<T> {
        match self.typ_(interner, &self.typ) {
            Some(typ) => Cow::Owned(typ),
            None => Cow::Borrowed(&self.typ),
        }
    }

    fn typ_(&self, interner: &mut impl TypeContext<Id, T>, typ: &T) -> Option<T> {
        if !typ.flags().intersects(Flags::HAS_IDENTS) {
            return None;
        }
        match **typ {
            Type::Ident(ref id) => {
                // Replace `Ident` with the alias it resolves to so that a `TypeEnv` is not
                // needed to resolve the type later on
                let replacement =
                    self.group
                        .iter()
                        .position(|alias| alias.name == *id)
                        .map(|index| {
                            interner.intern(Type::Alias(AliasRef {
                                index,
                                group: self.group.clone(),
                            }))
                        });
                if replacement.is_none() {
                    info!("Alias group were not able to resolve an identifier");
                }
                replacement
            }
            _ => walk_move_type_opt(
                typ,
                &mut InternerVisitor::control(interner, |interner, typ: &T| {
                    self.typ_(interner, typ)
                }),
            ),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
#[cfg_attr(feature = "serde_derive", derive(DeserializeState, SerializeState))]
#[cfg_attr(feature = "serde_derive", serde(deserialize_state = "Seed<Id, T>"))]
#[cfg_attr(
    feature = "serde_derive",
    serde(bound(deserialize = "
           T: Clone + From<Type<Id, T>> + ::std::any::Any + DeserializeState<'de, Seed<Id, T>>,
           Id: DeserializeState<'de, Seed<Id, T>> + Clone + ::std::any::Any"))
)]
#[cfg_attr(feature = "serde_derive", serde(serialize_state = "SeSeed"))]
#[cfg_attr(
    feature = "serde_derive",
    serde(bound(serialize = "T: SerializeState<SeSeed>, Id: SerializeState<SeSeed>"))
)]
pub struct AliasData<Id, T> {
    #[cfg_attr(feature = "serde_derive", serde(state))]
    pub name: Id,
    #[cfg_attr(feature = "serde_derive", serde(state))]
    args: Vec<Generic<Id>>,
    /// The type that is being aliased
    #[cfg_attr(feature = "serde_derive", serde(state))]
    typ: T,
    pub is_implicit: bool,
}

impl<Id, T> AliasData<Id, T> {
    /// Returns the type aliased by `self` with out `Type::Ident` resolved to their actual
    /// `Type::Alias` representation
    pub fn unresolved_type(&self) -> &T {
        &self.typ
    }

    pub fn unresolved_type_mut(&mut self) -> &mut T {
        &mut self.typ
    }

    pub fn is_implicit(&self) -> bool {
        self.is_implicit
    }
}

impl<Id, T> AliasData<Id, T>
where
    T: From<Type<Id, T>>,
{
    pub fn new(name: Id, args: Vec<Generic<Id>>, typ: T) -> AliasData<Id, T> {
        AliasData {
            name,
            args,
            typ,
            is_implicit: false,
        }
    }
}

impl<Id, T> AliasData<Id, T>
where
    T: Deref<Target = Type<Id, T>>,
{
    pub fn params(&self) -> &[Generic<Id>] {
        &self.args
    }

    pub fn params_mut(&mut self) -> &mut [Generic<Id>] {
        &mut self.args
    }

    pub fn aliased_type(&self) -> &T {
        &self.typ
    }

    pub fn kind<'k>(&'k self, cache: &'k KindCache) -> Cow<'k, ArcKind> {
        let result_type = self.unresolved_type().kind(cache);
        self.params().iter().rev().fold(result_type, |acc, param| {
            Cow::Owned(Kind::function(param.kind.clone(), acc.into_owned()))
        })
    }
}

impl<Id, T> Deref for AliasRef<Id, T> {
    type Target = AliasData<Id, T>;

    fn deref(&self) -> &Self::Target {
        &self.group[self.index]
    }
}

#[derive(Clone, Hash, Eq, PartialEq, Debug)]
#[cfg_attr(feature = "serde_derive", derive(DeserializeState, SerializeState))]
#[cfg_attr(feature = "serde_derive", serde(deserialize_state = "Seed<Id, U>"))]
#[cfg_attr(feature = "serde_derive", serde(de_parameters = "U"))]
#[cfg_attr(
    feature = "serde_derive",
    serde(bound(deserialize = "
           Id: DeserializeState<'de, Seed<Id, U>> + Clone + ::std::any::Any,
           T: DeserializeState<'de, Seed<Id, U>>
                             "))
)]
#[cfg_attr(feature = "serde_derive", serde(serialize_state = "SeSeed"))]
#[cfg_attr(
    feature = "serde_derive",
    serde(bound(serialize = "T: SerializeState<SeSeed>, Id: SerializeState<SeSeed>"))
)]
pub struct Field<Id, T = ArcType<Id>> {
    #[cfg_attr(feature = "serde_derive", serde(state))]
    pub name: Id,
    #[cfg_attr(feature = "serde_derive", serde(state))]
    pub typ: T,
}

/// `SmallVec` used in the `Type::App` constructor to avoid allocating a `Vec` for every applied
/// type. If `Type` is changed in a way that changes its size it is likely a good idea to change
/// the number of elements in the `SmallVec` so that it fills out the entire `Type` enum while not
/// increasing the size of `Type`.
pub type AppVec<T> = SmallVec<[T; 2]>;

impl<Id, T> Field<Id, T> {
    pub fn new(name: Id, typ: T) -> Field<Id, T> {
        Field { name, typ }
    }

    pub fn ctor<J>(ctor_name: Id, elems: J) -> Self
    where
        J: IntoIterator<Item = T>,
        J::IntoIter: DoubleEndedIterator,
        T: From<Type<Id, T>>,
    {
        let typ = Type::function_type(ArgType::Constructor, elems, Type::opaque());
        Field {
            name: ctor_name,
            typ,
        }
    }
}

#[cfg_attr(feature = "serde_derive", derive(Deserialize, Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ArgType {
    Explicit,
    Implicit,
    Constructor,
}

/// The representation of gluon's types.
///
/// For efficiency this enum is not stored directly but instead a pointer wrapper which derefs to
/// `Type` is used to enable types to be shared. It is recommended to use the static functions on
/// `Type` such as `Type::app` and `Type::record` when constructing types as those will construct
/// the pointer wrapper directly.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
#[cfg_attr(feature = "serde_derive", derive(DeserializeState, SerializeState))]
#[cfg_attr(feature = "serde_derive", serde(deserialize_state = "Seed<Id, T>"))]
#[cfg_attr(
    feature = "serde_derive",
    serde(bound(deserialize = "
           T: Clone
                + From<Type<Id, T>>
                + ::std::any::Any
                + DeserializeState<'de, Seed<Id, T>>,
           Id: DeserializeState<'de, Seed<Id, T>>
                + Clone
                + ::std::any::Any
                + DeserializeState<'de, Seed<Id, T>>"))
)]
#[cfg_attr(feature = "serde_derive", serde(serialize_state = "SeSeed"))]
pub enum Type<Id, T = ArcType<Id>> {
    /// An unbound type `_`, awaiting ascription.
    Hole,
    /// An opaque type
    Opaque,
    /// A type used to mark type errors
    Error,
    /// A builtin type
    Builtin(BuiltinType),
    /// Universally quantified types
    Forall(
        #[cfg_attr(feature = "serde_derive", serde(state))] Vec<Generic<Id>>,
        #[cfg_attr(feature = "serde_derive", serde(state))] T,
    ),
    /// A type application with multiple arguments. For example,
    /// `Map String Int` would be represented as `App(Map, [String, Int])`.
    App(
        #[cfg_attr(feature = "serde_derive", serde(state))] T,
        #[cfg_attr(
            feature = "serde_derive",
            serde(state_with = "crate::serialization::seq")
        )]
        AppVec<T>,
    ),
    /// Function type which can have a explicit or implicit argument
    Function(
        ArgType,
        #[cfg_attr(feature = "serde_derive", serde(state))] T,
        #[cfg_attr(feature = "serde_derive", serde(state))] T,
    ),
    /// Record constructor, of kind `Row -> Type`
    Record(#[cfg_attr(feature = "serde_derive", serde(state))] T),
    /// Variant constructor, of kind `Row -> Type`
    Variant(#[cfg_attr(feature = "serde_derive", serde(state))] T),
    /// Effect constructor, of kind `Row -> Type -> Type`
    Effect(#[cfg_attr(feature = "serde_derive", serde(state))] T),
    /// The empty row, of kind `Row`
    EmptyRow,
    /// Row extension, of kind `... -> Row -> Row`
    ExtendRow {
        /// The fields of this record type
        #[cfg_attr(feature = "serde_derive", serde(state))]
        fields: Vec<Field<Id, T>>,
        /// The rest of the row
        #[cfg_attr(feature = "serde_derive", serde(state))]
        rest: T,
    },
    /// Row extension, of kind `... -> Row -> Row`
    ExtendTypeRow {
        /// The associated types of this record type
        #[cfg_attr(feature = "serde_derive", serde(state))]
        types: Vec<Field<Id, Alias<Id, T>>>,
        /// The rest of the row
        #[cfg_attr(feature = "serde_derive", serde(state))]
        rest: T,
    },
    /// An identifier type. These are created during parsing, but should all be
    /// resolved into `Type::Alias`es during type checking.
    ///
    /// Identifiers are also sometimes used inside aliased types to avoid cycles
    /// in reference counted pointers. This is a bit of a wart at the moment and
    /// _may_ cause spurious unification failures.
    Ident(#[cfg_attr(feature = "serde_derive", serde(state))] Id),
    Projection(
        #[cfg_attr(
            feature = "serde_derive",
            serde(state_with = "crate::serialization::seq")
        )]
        AppVec<Id>,
    ),
    /// An unbound type variable that may be unified with other types. These
    /// will eventually be converted into `Type::Generic`s during generalization.
    Variable(#[cfg_attr(feature = "serde_derive", serde(state))] TypeVariable),
    /// A variable that needs to be instantiated with a fresh type variable
    /// when the binding is referred to.
    Generic(#[cfg_attr(feature = "serde_derive", serde(state))] Generic<Id>),
    Alias(#[cfg_attr(feature = "serde_derive", serde(state))] AliasRef<Id, T>),
    Skolem(#[cfg_attr(feature = "serde_derive", serde(state))] Skolem<Id>),
}

#[cfg(target_pointer_width = "64")]
// Safeguard against accidentally growing Type as it is a core type
const _: [(); 8 * 6] = [(); std::mem::size_of::<Type<Symbol, ArcType>>()];

impl<Id, T> Type<Id, T> {
    pub fn as_variable(&self) -> Option<&TypeVariable> {
        match *self {
            Type::Variable(ref var) => Some(var),
            _ => None,
        }
    }
}

impl<Id, T> Type<Id, T>
where
    T: From<Type<Id, T>>,
{
    pub fn hole() -> T {
        T::from(Type::Hole)
    }

    pub fn opaque() -> T {
        T::from(Type::Opaque)
    }

    pub fn error() -> T {
        T::from(Type::Error)
    }

    pub fn builtin(typ: BuiltinType) -> T {
        T::from(Type::Builtin(typ))
    }

    pub fn forall(params: Vec<Generic<Id>>, typ: T) -> T {
        if params.is_empty() {
            typ
        } else {
            T::from(Type::Forall(params, typ))
        }
    }

    pub fn array(typ: T) -> T {
        Type::app(Type::array_builtin(), collect![typ])
    }

    pub fn array_builtin() -> T {
        Type::builtin(BuiltinType::Array)
    }

    pub fn app(id: T, args: AppVec<T>) -> T {
        if args.is_empty() {
            id
        } else {
            T::from(Type::App(id, args))
        }
    }

    pub fn variant(fields: Vec<Field<Id, T>>) -> T {
        Type::poly_variant(fields, Type::empty_row())
    }

    pub fn poly_variant(fields: Vec<Field<Id, T>>, rest: T) -> T {
        T::from(Type::Variant(Type::extend_row(fields, rest)))
    }

    pub fn effect(fields: Vec<Field<Id, T>>) -> T {
        Type::poly_effect(fields, Type::empty_row())
    }

    pub fn poly_effect(fields: Vec<Field<Id, T>>, rest: T) -> T {
        T::from(Type::Effect(Type::extend_row(fields, rest)))
    }

    pub fn tuple<S, I>(symbols: &mut S, elems: I) -> T
    where
        S: ?Sized + IdentEnv<Ident = Id>,
        I: IntoIterator<Item = T>,
    {
        T::from(Type::tuple_(symbols, elems))
    }

    pub fn tuple_<S, I>(symbols: &mut S, elems: I) -> Type<Id, T>
    where
        S: ?Sized + IdentEnv<Ident = Id>,
        I: IntoIterator<Item = T>,
    {
        Type::Record(Type::extend_row(
            elems
                .into_iter()
                .enumerate()
                .map(|(i, typ)| Field {
                    name: symbols.from_str(&format!("_{}", i)),
                    typ,
                })
                .collect(),
            Type::empty_row(),
        ))
    }

    pub fn record(types: Vec<Field<Id, Alias<Id, T>>>, fields: Vec<Field<Id, T>>) -> T {
        Type::poly_record(types, fields, Type::empty_row())
    }

    pub fn poly_record(
        types: Vec<Field<Id, Alias<Id, T>>>,
        fields: Vec<Field<Id, T>>,
        rest: T,
    ) -> T {
        T::from(Type::Record(Type::extend_full_row(types, fields, rest)))
    }

    pub fn extend_full_row(
        types: Vec<Field<Id, Alias<Id, T>>>,
        fields: Vec<Field<Id, T>>,
        rest: T,
    ) -> T {
        Self::extend_type_row(types, Self::extend_row(fields, rest))
    }

    pub fn extend_row(fields: Vec<Field<Id, T>>, rest: T) -> T {
        if fields.is_empty() {
            rest
        } else {
            T::from(Type::ExtendRow { fields, rest })
        }
    }

    pub fn extend_type_row(types: Vec<Field<Id, Alias<Id, T>>>, rest: T) -> T {
        if types.is_empty() {
            rest
        } else {
            T::from(Type::ExtendTypeRow { types, rest })
        }
    }

    pub fn empty_row() -> T {
        T::from(Type::EmptyRow)
    }

    pub fn function<I>(args: I, ret: T) -> T
    where
        I: IntoIterator<Item = T>,
        I::IntoIter: DoubleEndedIterator<Item = T>,
    {
        Self::function_type(ArgType::Explicit, args, ret)
    }

    pub fn function_implicit<I>(args: I, ret: T) -> T
    where
        I: IntoIterator<Item = T>,
        I::IntoIter: DoubleEndedIterator<Item = T>,
    {
        Self::function_type(ArgType::Implicit, args, ret)
    }

    pub fn function_type<I>(arg_type: ArgType, args: I, ret: T) -> T
    where
        I: IntoIterator<Item = T>,
        I::IntoIter: DoubleEndedIterator<Item = T>,
    {
        args.into_iter().rev().fold(ret, |body, arg| {
            T::from(Type::Function(arg_type, arg, body))
        })
    }

    pub fn generic(typ: Generic<Id>) -> T {
        T::from(Type::Generic(typ))
    }

    pub fn skolem(typ: Skolem<Id>) -> T {
        T::from(Type::Skolem(typ))
    }

    pub fn variable(typ: TypeVariable) -> T {
        T::from(Type::Variable(typ))
    }

    pub fn alias(name: Id, args: Vec<Generic<Id>>, typ: T) -> T {
        Self::alias_implicit(name, args, typ, false)
    }

    pub fn alias_implicit(name: Id, args: Vec<Generic<Id>>, typ: T, is_implicit: bool) -> T {
        T::from(Type::Alias(AliasRef {
            index: 0,
            group: Arc::from(vec![AliasData {
                name,
                args,
                typ,
                is_implicit,
            }]),
        }))
    }

    pub fn ident(id: Id) -> T {
        T::from(Type::Ident(id))
    }

    pub fn projection(id: AppVec<Id>) -> T {
        T::from(Type::Projection(id))
    }

    pub fn function_builtin() -> T {
        Type::builtin(BuiltinType::Function)
    }

    pub fn string() -> T {
        Type::builtin(BuiltinType::String)
    }

    pub fn char() -> T {
        Type::builtin(BuiltinType::Char)
    }

    pub fn byte() -> T {
        Type::builtin(BuiltinType::Byte)
    }

    pub fn int() -> T {
        Type::builtin(BuiltinType::Int)
    }

    pub fn float() -> T {
        Type::builtin(BuiltinType::Float)
    }

    pub fn unit() -> T {
        Type::record(vec![], vec![])
    }
}

impl<Id, T> Type<Id, T>
where
    T: Deref<Target = Type<Id, T>>,
{
    pub fn as_function(&self) -> Option<(&T, &T)> {
        self.as_function_with_type().map(|t| (t.1, t.2))
    }

    pub fn as_explicit_function(&self) -> Option<(&T, &T)> {
        self.as_function_with_type().and_then(|t| {
            if t.0 == ArgType::Explicit {
                Some((t.1, t.2))
            } else {
                None
            }
        })
    }

    pub fn as_function_with_type(&self) -> Option<(ArgType, &T, &T)> {
        match *self {
            Type::Function(arg_type, ref arg, ref ret) => return Some((arg_type, arg, ret)),
            Type::App(ref app, ref args) => {
                if args.len() == 2 {
                    if let Type::Builtin(BuiltinType::Function) = **app {
                        return Some((ArgType::Explicit, &args[0], &args[1]));
                    }
                } else if args.len() == 1 {
                    if let Type::App(ref app, ref args2) = **app {
                        if let Type::Builtin(BuiltinType::Function) = **app {
                            return Some((ArgType::Explicit, &args2[0], &args[0]));
                        }
                    }
                }
            }
            _ => (),
        }
        None
    }

    pub fn unapplied_args(&self) -> Cow<[T]>
    where
        T: Clone,
    {
        match *self {
            Type::App(ref f, ref args) => {
                let mut f = f;
                let mut extra_args = Vec::new();
                while let Type::App(ref f2, ref args2) = **f {
                    f = f2;
                    extra_args.extend(args2.iter().rev().cloned());
                }
                if extra_args.is_empty() {
                    Cow::Borrowed(args)
                } else {
                    extra_args.reverse();
                    extra_args.extend(args.iter().cloned());
                    Cow::Owned(extra_args)
                }
            }
            _ => Cow::Borrowed(&[]),
        }
    }

    pub fn alias_ident(&self) -> Option<&Id> {
        match self {
            Type::App(id, _) => id.alias_ident(),
            Type::Ident(id) => Some(id),
            Type::Alias(alias) => Some(&alias.name),
            _ => None,
        }
    }

    pub fn applied_alias(&self) -> Option<&AliasRef<Id, T>> {
        match *self {
            Type::Alias(ref alias) => Some(alias),
            Type::App(ref alias, _) => alias.applied_alias(),
            _ => None,
        }
    }

    pub fn is_non_polymorphic_record(&self) -> bool {
        match *self {
            Type::Record(ref row)
            | Type::ExtendRow { rest: ref row, .. }
            | Type::ExtendTypeRow { rest: ref row, .. } => row.is_non_polymorphic_record(),
            Type::EmptyRow => true,
            _ => false,
        }
    }

    pub fn params(&self) -> &[Generic<Id>] {
        match *self {
            Type::Alias(ref alias) => alias.typ.params(),
            _ => &[],
        }
    }

    pub fn kind<'k>(&'k self, cache: &'k KindCache) -> Cow<'k, ArcKind> {
        self.kind_(cache, 0)
    }

    fn kind_<'k>(&'k self, cache: &'k KindCache, applied_args: usize) -> Cow<'k, ArcKind> {
        let mut immediate_kind = match *self {
            Type::Function(_, _, _) => Cow::Borrowed(&cache.typ),
            Type::App(ref t, ref args) => t.kind_(cache, args.len()),
            Type::Error => Cow::Borrowed(&cache.error),
            Type::Hole => Cow::Borrowed(&cache.hole),
            Type::Opaque | Type::Builtin(_) | Type::Record(_) | Type::Variant(_) => {
                Cow::Owned(Kind::typ())
            }
            Type::EmptyRow | Type::ExtendRow { .. } | Type::ExtendTypeRow { .. } => {
                Cow::Owned(Kind::row())
            }
            Type::Effect(_) => {
                let t = Kind::typ();
                Cow::Owned(Kind::function(t.clone(), t))
            }
            Type::Forall(_, ref typ) => typ.kind_(cache, applied_args),
            Type::Variable(ref var) => Cow::Borrowed(&var.kind),
            Type::Skolem(ref skolem) => Cow::Borrowed(&skolem.kind),
            Type::Generic(ref gen) => Cow::Borrowed(&gen.kind),
            // FIXME can be another kind
            Type::Ident(_) | Type::Projection(_) => Cow::Owned(cache.typ()),
            Type::Alias(ref alias) => {
                return if alias.params().len() < applied_args {
                    alias.typ.kind_(cache, applied_args - alias.params().len())
                } else {
                    let mut kind = alias.typ.kind_(cache, 0).into_owned();
                    for arg in &alias.params()[applied_args..] {
                        kind = Kind::function(arg.kind.clone(), kind)
                    }
                    Cow::Owned(kind)
                };
            }
        };
        for _ in 0..applied_args {
            immediate_kind = match immediate_kind {
                Cow::Borrowed(k) => match **k {
                    Kind::Function(_, ref ret) => Cow::Borrowed(ret),
                    _ => return Cow::Borrowed(k),
                },
                Cow::Owned(k) => match *k {
                    Kind::Function(_, ref ret) => Cow::Owned(ret.clone()),
                    _ => return Cow::Owned(k.clone()),
                },
            };
        }
        immediate_kind
    }
}

impl<T> Type<Symbol, T>
where
    T: Deref<Target = Type<Symbol, T>>,
{
    /// Returns the name of `self`
    /// Example:
    /// Option a => Option
    /// Int => Int
    pub fn name(&self) -> Option<&SymbolRef> {
        if let Some(id) = self.alias_ident() {
            return Some(&**id);
        }

        match *self {
            Type::Function(..) => Some(BuiltinType::Function.symbol()),
            Type::App(ref id, _) => match **id {
                Type::Builtin(b) => Some(b.symbol()),
                _ => None,
            },
            Type::Builtin(b) => Some(b.symbol()),
            Type::Effect(_) => Some(SymbolRef::new("Effect")),
            _ => None,
        }
    }

    pub fn owned_name(&self) -> Option<SymbolKey> {
        if let Some(id) = self.alias_ident() {
            return Some(SymbolKey::Owned(id.clone()));
        }

        match *self {
            Type::Function(..) => Some(SymbolKey::Ref(BuiltinType::Function.symbol())),
            Type::App(ref id, _) => match **id {
                Type::Builtin(b) => Some(SymbolKey::Ref(b.symbol())),
                _ => None,
            },
            Type::Builtin(b) => Some(SymbolKey::Ref(b.symbol())),
            Type::Effect(_) => Some(SymbolKey::Ref(SymbolRef::new("Effect"))),
            _ => None,
        }
    }
}

#[derive(Eq, Clone)]
pub enum SymbolKey {
    Owned(Symbol),
    Ref(&'static SymbolRef),
}

impl fmt::Debug for SymbolKey {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", Borrow::<SymbolRef>::borrow(self))
    }
}

impl fmt::Display for SymbolKey {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", Borrow::<SymbolRef>::borrow(self))
    }
}

impl Hash for SymbolKey {
    fn hash<H>(&self, state: &mut H)
    where
        H: Hasher,
    {
        Borrow::<SymbolRef>::borrow(self).hash(state)
    }
}

impl PartialEq for SymbolKey {
    fn eq(&self, other: &Self) -> bool {
        Borrow::<SymbolRef>::borrow(self) == Borrow::<SymbolRef>::borrow(other)
    }
}

impl PartialOrd for SymbolKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Borrow::<SymbolRef>::borrow(self).partial_cmp(Borrow::<SymbolRef>::borrow(other))
    }
}

impl Ord for SymbolKey {
    fn cmp(&self, other: &Self) -> Ordering {
        Borrow::<SymbolRef>::borrow(self).cmp(Borrow::<SymbolRef>::borrow(other))
    }
}

impl Deref for SymbolKey {
    type Target = SymbolRef;

    fn deref(&self) -> &Self::Target {
        match *self {
            SymbolKey::Owned(ref s) => s,
            SymbolKey::Ref(s) => s,
        }
    }
}

impl Borrow<SymbolRef> for SymbolKey {
    fn borrow(&self) -> &SymbolRef {
        self
    }
}

#[derive(Clone)]
struct ArcTypeInner<Id = Symbol> {
    typ: Type<Id, ArcType<Id>>,
    flags: Flags,
}

forward_eq_hash! { <Id> for ArcTypeInner<Id> { typ } }

/// A shared type which is atomically reference counted
#[derive(Eq, PartialEq, Hash)]
pub struct ArcType<Id = Symbol> {
    typ: Arc<ArcTypeInner<Id>>,
}

impl<Id> Default for ArcType<Id>
where
    Id: PartialEq,
{
    fn default() -> Self {
        Type::hole()
    }
}

#[cfg(feature = "serde")]
impl<'de, Id> DeserializeState<'de, Seed<Id, ArcType<Id>>> for ArcType<Id>
where
    Id: DeserializeState<'de, Seed<Id, ArcType<Id>>> + Clone + ::std::any::Any + PartialEq,
{
    fn deserialize_state<D>(
        seed: &mut Seed<Id, ArcType<Id>>,
        deserializer: D,
    ) -> Result<Self, D::Error>
    where
        D: crate::serde::de::Deserializer<'de>,
    {
        use crate::serialization::SharedSeed;
        let seed = SharedSeed::new(seed);
        crate::serde::de::DeserializeSeed::deserialize(seed, deserializer).map(ArcType::new)
    }
}

impl<Id> Clone for ArcType<Id> {
    fn clone(&self) -> ArcType<Id> {
        ArcType {
            typ: self.typ.clone(),
        }
    }
}

impl<Id: fmt::Debug> fmt::Debug for ArcType<Id> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        (**self).fmt(f)
    }
}

impl<Id: AsRef<str>> fmt::Display for ArcType<Id> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", TypeFormatter::new(self))
    }
}

impl<Id> fmt::Pointer for ArcType<Id> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:p}", &**self)
    }
}

impl<Id> Borrow<Type<Id, ArcType<Id>>> for ArcType<Id> {
    fn borrow(&self) -> &Type<Id, ArcType<Id>> {
        self
    }
}

impl<Id> Deref for ArcType<Id> {
    type Target = Type<Id, ArcType<Id>>;

    fn deref(&self) -> &Type<Id, ArcType<Id>> {
        &self.typ.typ
    }
}

impl<Id> HasSpan for ArcType<Id> {
    fn span(&self) -> Span<BytePos> {
        Span::new(0.into(), 0.into())
    }
}

impl<Id> Commented for ArcType<Id> {
    fn comment(&self) -> Option<&Comment> {
        None
    }
}

pub fn row_iter<T>(typ: &T) -> RowIterator<T> {
    RowIterator { typ, current: 0 }
}

pub fn row_iter_mut<Id, T>(typ: &mut T) -> RowIteratorMut<Id, T> {
    RowIteratorMut {
        fields: [].iter_mut(),
        rest: Some(typ),
    }
}

pub fn type_field_iter<T>(typ: &T) -> TypeFieldIterator<T> {
    TypeFieldIterator { typ, current: 0 }
}

pub fn remove_forall<'a, Id, T>(typ: &'a T) -> &'a T
where
    T: Deref<Target = Type<Id, T>>,
    Id: 'a,
{
    match **typ {
        Type::Forall(_, ref typ) => remove_forall(typ),
        _ => typ,
    }
}

pub fn remove_forall_mut<'a, Id, T>(typ: &'a mut T) -> &'a mut T
where
    T: DerefMut<Target = Type<Id, T>>,
    Id: 'a,
{
    if let Type::Forall(_, _) = **typ {
        match **typ {
            Type::Forall(_, ref mut typ) => remove_forall_mut(typ),
            _ => unreachable!(),
        }
    } else {
        typ
    }
}

pub trait TypeExt: Deref<Target = Type<<Self as TypeExt>::Id, Self>> + Clone + Sized {
    type Id;

    fn new(typ: Type<Self::Id, Self>) -> Self;

    fn strong_count(typ: &Self) -> usize;

    fn spine(&self) -> &Self {
        match &**self {
            Type::App(ref id, _) => id.spine(),
            _ => self,
        }
    }

    /// Returns an iterator over all type fields in a record.
    /// `{ Test, Test2, x, y } => [Test, Test2]`
    fn type_field_iter(&self) -> TypeFieldIterator<Self> {
        type_field_iter(self)
    }

    /// Returns an iterator over all fields in a record.
    /// `{ Test, Test2, x, y } => [x, y]`
    fn row_iter(&self) -> RowIterator<Self> {
        row_iter(self)
    }

    fn remove_implicit_args<'a>(&'a self) -> &'a Self {
        match **self {
            Type::Function(ArgType::Implicit, _, ref typ) => typ.remove_implicit_args(),
            _ => self,
        }
    }

    fn remove_forall<'a>(&'a self) -> &'a Self {
        remove_forall(self)
    }

    fn remove_forall_and_implicit_args<'a>(&'a self) -> &'a Self {
        match **self {
            Type::Function(ArgType::Implicit, _, ref typ) => typ.remove_forall_and_implicit_args(),
            Type::Forall(_, ref typ) => typ.remove_forall_and_implicit_args(),
            _ => self,
        }
    }

    fn replace_generics(
        &self,
        interner: &mut impl TypeContext<Self::Id, Self>,
        named_variables: &mut FnvMap<Self::Id, Self>,
    ) -> Option<Self>
    where
        Self::Id: Clone + Eq + Hash,
        Self: Clone,
    {
        if named_variables.is_empty() {
            None
        } else {
            self.replace_generics_(interner, named_variables)
        }
    }

    fn replace_generics_(
        &self,
        interner: &mut impl TypeContext<Self::Id, Self>,
        named_variables: &mut FnvMap<Self::Id, Self>,
    ) -> Option<Self>
    where
        Self::Id: Clone + Eq + Hash,
        Self: Clone,
    {
        if !self.flags().intersects(Flags::HAS_GENERICS) {
            return None;
        }

        match &**self {
            Type::Generic(generic) => named_variables.get(&generic.id).cloned(),
            Type::Forall(params, typ) => {
                let removed: AppVec<_> = params
                    .iter()
                    .flat_map(|param| named_variables.remove_entry(&param.id))
                    .collect();

                let new_typ = typ.replace_generics(interner, named_variables);
                let new_typ = new_typ.map(|typ| interner.intern(Type::Forall(params.clone(), typ)));

                named_variables.extend(removed);

                new_typ
            }
            _ => walk_move_type_opt(
                self,
                &mut InternerVisitor::control(interner, |interner, typ: &Self| {
                    typ.replace_generics_(interner, named_variables)
                }),
            ),
        }
    }

    fn forall_scope_iter(&self) -> ForallScopeIter<Self> {
        ForallScopeIter {
            typ: self,
            offset: 0,
        }
    }

    fn pretty<'a, A>(&'a self, arena: &'a Arena<'a, A>) -> DocBuilder<'a, Arena<'a, A>, A>
    where
        Self::Id: AsRef<str> + 'a,
        A: Clone,
        Self: Commented + HasSpan,
    {
        top(self).pretty(&Printer::new(arena, &()))
    }

    fn display<A>(&self, width: usize) -> TypeFormatter<Self::Id, Self, A>
    where
        Self::Id: AsRef<str>,
    {
        TypeFormatter::new(self).width(width)
    }

    /// Applies a list of arguments to a parameterised type, returning `Some`
    /// if the substitution was successful.
    ///
    /// Example:
    ///
    /// ```text
    /// self = forall e t . | Err e | Ok t
    /// args = [Error, Option a]
    /// result = | Err Error | Ok (Option a)
    /// ```
    fn apply_args<'a>(
        &self,
        params: &[Generic<Self::Id>],
        args: &'a [Self],
        interner: &mut impl TypeContext<Self::Id, Self>,
        named_variables: &mut FnvMap<Self::Id, Self>,
    ) -> Option<Self>
    where
        Self::Id: Clone + Eq + Hash,
        Self: fmt::Display,
        Self::Id: fmt::Display,
    {
        let (non_replaced_type, unapplied_args) =
            self.arg_application(params, args, interner, named_variables)?;

        let non_replaced_type = non_replaced_type
            .replace_generics(interner, named_variables)
            .unwrap_or_else(|| non_replaced_type.clone());
        Some(interner.app(non_replaced_type, unapplied_args.iter().cloned().collect()))
    }

    fn arg_application<'a>(
        &self,
        params: &[Generic<Self::Id>],
        args: &'a [Self],
        interner: &mut impl TypeContext<Self::Id, Self>,
        named_variables: &mut FnvMap<Self::Id, Self>,
    ) -> Option<(Self, &'a [Self])>
    where
        Self::Id: Clone + Eq + Hash,
        Self: fmt::Display,
        Self::Id: fmt::Display,
    {
        let typ = if params.len() == args.len() {
            Cow::Borrowed(self)
        } else {
            // It is ok to take the type only if it is fully applied or if it
            // the missing argument only appears in order at the end, i.e:
            //
            // type Test a b c = Type (a Int) b c
            //
            // Test a b == Type (a Int) b
            // Test a == Type (a Int)
            // Test == ??? (Impossible to do a sane substitution)
            let (d, arg_types) = split_app(self);
            let allowed_missing_args = arg_types
                .iter()
                .rev()
                .zip(params.iter().rev())
                .take_while(|&(l, r)| match **l {
                    Type::Generic(ref g) => g == r,
                    _ => false,
                })
                .count();

            if params.len() > allowed_missing_args + args.len() {
                return None;
            }
            // Remove the args at the end of the aliased type
            let arg_types: AppVec<_> = arg_types
                .iter()
                .take(arg_types.len() + args.len() - params.len())
                .cloned()
                .collect();

            let d = d.cloned().unwrap_or_else(|| interner.function_builtin());
            Cow::Owned(interner.app(d, arg_types))
        };

        named_variables.extend(
            params
                .iter()
                .map(|g| g.id.clone())
                .zip(args.iter().cloned()),
        );

        Some((typ.into_owned(), &args[params.len().min(args.len())..]))
    }

    fn flags(&self) -> Flags {
        Flags::all()
    }

    fn instantiate_generics(
        &self,
        interner: &mut impl Substitution<Self::Id, Self>,
        named_variables: &mut FnvMap<Self::Id, Self>,
    ) -> Self
    where
        Self::Id: Clone + Eq + Hash,
    {
        let mut typ = self;
        while let Type::Forall(params, inner_type) = &**typ {
            named_variables.extend(
                params
                    .iter()
                    .map(|param| (param.id.clone(), interner.new_var())),
            );
            typ = inner_type;
        }
        if named_variables.is_empty() {
            typ.clone()
        } else {
            typ.replace_generics(interner, named_variables)
                .unwrap_or_else(|| typ.clone())
        }
    }

    fn skolemize(
        &self,
        interner: &mut impl Substitution<Self::Id, Self>,
        named_variables: &mut FnvMap<Self::Id, Self>,
    ) -> Self
    where
        Self::Id: Clone + Eq + Hash,
    {
        let mut typ = self;
        while let Type::Forall(ref params, ref inner_type) = **typ {
            let iter = params.iter().map(|param| {
                (
                    param.id.clone(),
                    interner.new_skolem(param.id.clone(), param.kind.clone()),
                )
            });
            named_variables.extend(iter);
            typ = inner_type;
        }
        if named_variables.is_empty() {
            typ.clone()
        } else {
            typ.replace_generics(interner, named_variables)
                .unwrap_or_else(|| typ.clone())
        }
    }

    fn skolemize_in(
        &self,
        interner: &mut impl Substitution<Self::Id, Self>,
        named_variables: &mut FnvMap<Self::Id, Self>,
        f: impl FnOnce(Self) -> Self,
    ) -> Self
    where
        Self::Id: Clone + Eq + Hash,
    {
        let skolemized = self.skolemize(interner, named_variables);
        let new_type = f(skolemized);
        interner.with_forall(new_type, self)
    }
}

pub fn forall_params<'a, T, Id>(mut typ: &'a T) -> impl Iterator<Item = &'a Generic<Id>>
where
    Id: 'a,
    T: Deref<Target = Type<Id, T>>,
{
    let mut i = 0;
    iter::repeat(()).scan((), move |_, _| {
        while let Type::Forall(ref params, ref inner_type) = **typ {
            if i < params.len() {
                i += 1;
                return Some(&params[i - 1]);
            } else {
                i = 0;
                typ = inner_type;
            }
        }
        None
    })
}

impl<Id> TypeExt for ArcType<Id>
where
    Id: PartialEq,
{
    type Id = Id;

    fn new(typ: Type<Id, ArcType<Id>>) -> ArcType<Id> {
        let flags = Flags::from_type(&typ);
        Self::with_flags(typ, flags)
    }

    fn strong_count(typ: &ArcType<Id>) -> usize {
        Arc::strong_count(&typ.typ)
    }

    fn flags(&self) -> Flags {
        self.typ.flags
    }
}

pub struct ForallScopeIter<'a, T: 'a> {
    pub typ: &'a T,
    offset: usize,
}

impl<'a, T, Id: 'a> Iterator for ForallScopeIter<'a, T>
where
    T: Deref<Target = Type<Id, T>>,
{
    type Item = &'a Generic<Id>;

    fn next(&mut self) -> Option<Self::Item> {
        match **self.typ {
            Type::Forall(ref params, ref typ) => {
                let offset = self.offset;
                let item = params.get(offset).map(|param| {
                    self.offset += 1;
                    param
                });
                match item {
                    Some(_) => item,
                    None => {
                        self.typ = typ;
                        self.next()
                    }
                }
            }
            _ => None,
        }
    }
}

impl<Id> From<Type<Id, ArcType<Id>>> for ArcType<Id>
where
    Id: PartialEq,
{
    fn from(typ: Type<Id, ArcType<Id>>) -> ArcType<Id> {
        ArcType::new(typ)
    }
}

impl<Id> From<(Type<Id, ArcType<Id>>, Flags)> for ArcType<Id> {
    fn from((typ, flags): (Type<Id, ArcType<Id>>, Flags)) -> ArcType<Id> {
        ArcType::with_flags(typ, flags)
    }
}

#[derive(Clone)]
pub struct TypeFieldIterator<'a, T: 'a> {
    typ: &'a T,
    current: usize,
}

impl<'a, Id: 'a, T> Iterator for TypeFieldIterator<'a, T>
where
    T: Deref<Target = Type<Id, T>>,
{
    type Item = &'a Field<Id, Alias<Id, T>>;

    fn next(&mut self) -> Option<&'a Field<Id, Alias<Id, T>>> {
        match **self.typ {
            Type::ExtendRow { ref rest, .. } | Type::Record(ref rest) => {
                self.typ = rest;
                self.next()
            }
            Type::ExtendTypeRow {
                ref types,
                ref rest,
            } => {
                let current = self.current;
                self.current += 1;
                types.get(current).or_else(|| {
                    self.current = 0;
                    self.typ = rest;
                    self.next()
                })
            }
            _ => None,
        }
    }
}

#[derive(Clone)]
pub struct RowIterator<'a, T: 'a> {
    typ: &'a T,
    current: usize,
}

impl<'a, T> RowIterator<'a, T> {
    pub fn current_type(&self) -> &'a T {
        self.typ
    }
}

impl<'a, Id: 'a, T> Iterator for RowIterator<'a, T>
where
    T: Deref<Target = Type<Id, T>>,
{
    type Item = &'a Field<Id, T>;

    fn next(&mut self) -> Option<&'a Field<Id, T>> {
        match **self.typ {
            Type::Record(ref row) | Type::Variant(ref row) => {
                self.typ = row;
                self.next()
            }
            Type::ExtendRow {
                ref fields,
                ref rest,
                ..
            } => {
                let current = self.current;
                self.current += 1;
                fields.get(current).or_else(|| {
                    self.current = 0;
                    self.typ = rest;
                    self.next()
                })
            }
            Type::ExtendTypeRow { ref rest, .. } => {
                self.typ = rest;
                self.next()
            }
            _ => None,
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.len();
        (len, Some(len))
    }
}

impl<'a, Id: 'a, T> ExactSizeIterator for RowIterator<'a, T>
where
    T: Deref<Target = Type<Id, T>>,
{
    fn len(&self) -> usize {
        let mut typ = self.typ;
        let mut size = 0;
        loop {
            typ = match **typ {
                Type::Record(ref row) | Type::Variant(ref row) => row,
                Type::ExtendRow {
                    ref fields,
                    ref rest,
                    ..
                } => {
                    size += fields.len();
                    rest
                }
                Type::ExtendTypeRow { ref rest, .. } => rest,
                _ => break,
            };
        }
        size
    }
}

pub struct RowIteratorMut<'a, Id: 'a, T: 'a> {
    fields: ::std::slice::IterMut<'a, Field<Id, T>>,
    rest: Option<&'a mut T>,
}

impl<'a, Id, T> RowIteratorMut<'a, Id, T> {
    pub fn current_type(&mut self) -> &mut T {
        self.rest.as_mut().unwrap()
    }
}

impl<'a, Id: 'a, T: 'a> Iterator for RowIteratorMut<'a, Id, T>
where
    T: DerefMut<Target = Type<Id, T>>,
{
    type Item = &'a mut Field<Id, T>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(x) = self.fields.next() {
                return Some(x);
            }
            match ***self.rest.as_ref()? {
                Type::Record(_)
                | Type::Variant(_)
                | Type::ExtendRow { .. }
                | Type::ExtendTypeRow { .. } => (),
                _ => return None,
            };

            let rest = mem::replace(&mut self.rest, None)?;
            self.rest = match **rest {
                Type::Record(ref mut row) | Type::Variant(ref mut row) => Some(row),
                Type::ExtendRow {
                    ref mut fields,
                    ref mut rest,
                    ..
                } => {
                    self.fields = fields.iter_mut();
                    Some(rest)
                }
                Type::ExtendTypeRow { ref mut rest, .. } => Some(rest),
                _ => unreachable!(),
            };
        }
    }
}

fn split_top<'a, Id, T>(self_: &'a T) -> Option<(Option<&'a T>, Cow<[T]>)>
where
    T: Deref<Target = Type<Id, T>> + Clone,
    Id: 'a,
{
    Some(match **self_ {
        Type::App(ref f, ref args) => (Some(f), Cow::Borrowed(args)),
        Type::Function(_, ref a, ref r) => (None, Cow::Owned(vec![a.clone(), r.clone()])),
        _ => return None,
    })
}

fn clone_cow<'a, T>(cow: Cow<'a, [T]>) -> impl DoubleEndedIterator<Item = T> + 'a
where
    T: ToOwned + Clone,
{
    use itertools::Either;

    match cow {
        Cow::Borrowed(ts) => Either::Left(ts.iter().cloned()),
        Cow::Owned(ts) => Either::Right(ts.into_iter()),
    }
}

pub fn split_app<'a, Id, T>(self_: &'a T) -> (Option<&'a T>, Cow<[T]>)
where
    T: Deref<Target = Type<Id, T>> + Clone,
    Id: 'a,
{
    match split_top(self_) {
        Some((f, args)) => {
            let mut f = f;
            let mut extra_args = Vec::new();
            while let Some((f2, args2)) = f.and_then(split_top) {
                f = f2;
                extra_args.extend(clone_cow(args2).rev());
            }
            if extra_args.is_empty() {
                (f, args)
            } else {
                extra_args.reverse();
                extra_args.extend(clone_cow(args));
                (f, Cow::Owned(extra_args))
            }
        }
        None => (Some(self_), Cow::Borrowed(&[][..])),
    }
}

pub fn ctor_args<Id, T>(typ: &T) -> ArgIterator<T>
where
    T: Deref<Target = Type<Id, T>>,
{
    ArgIterator { typ }
}

pub struct ArgIterator<'a, T: 'a> {
    /// The current type being iterated over. After `None` has been returned this is the return
    /// type.
    pub typ: &'a T,
}

/// Constructs an iterator over a functions arguments
pub fn arg_iter<Id, T>(typ: &T) -> ArgIterator<T>
where
    T: Deref<Target = Type<Id, T>>,
{
    ArgIterator { typ }
}

impl<'a, Id, T> Iterator for ArgIterator<'a, T>
where
    Id: 'a,
    T: Deref<Target = Type<Id, T>>,
{
    type Item = &'a T;
    fn next(&mut self) -> Option<&'a T> {
        self.typ.as_function().map(|(arg, ret)| {
            self.typ = ret;
            arg
        })
    }
}

pub struct ImplicitArgIterator<'a, T: 'a> {
    /// The current type being iterated over. After `None` has been returned this is the return
    /// type.
    pub typ: &'a T,
}

/// Constructs an iterator over a functions arguments
pub fn implicit_arg_iter<Id, T>(typ: &T) -> ImplicitArgIterator<T>
where
    T: Deref<Target = Type<Id, T>>,
{
    ImplicitArgIterator { typ }
}

impl<'a, Id, T> Iterator for ImplicitArgIterator<'a, T>
where
    Id: 'a,
    T: Deref<Target = Type<Id, T>>,
{
    type Item = &'a T;
    fn next(&mut self) -> Option<&'a T> {
        self.typ
            .as_function_with_type()
            .and_then(|(arg_type, arg, ret)| {
                if arg_type == ArgType::Implicit {
                    self.typ = ret;
                    Some(arg)
                } else {
                    None
                }
            })
    }
}

impl<Id> ArcType<Id> {
    fn with_flags(typ: Type<Id, ArcType<Id>>, flags: Flags) -> ArcType<Id> {
        let typ = Arc::new(ArcTypeInner { typ, flags });
        ArcType { typ }
    }

    pub fn set(into: &mut Self, typ: Type<Id, Self>)
    where
        Id: Clone + PartialEq,
    {
        let into = Arc::make_mut(&mut into.typ);
        into.flags = Flags::from_type(&typ);
        into.typ = typ;
    }

    /// Returns the lowest level which this type contains. The level informs from where type
    /// variables where created.
    pub fn level(&self) -> u32 {
        use std::cmp::min;
        fold_type(
            self,
            |typ, level| match **typ {
                Type::Variable(ref var) => min(var.id, level),
                _ => level,
            },
            u32::max_value(),
        )
    }

    pub fn needs_generalize(&self) -> bool
    where
        Id: PartialEq,
    {
        self.flags().intersects(Flags::NEEDS_GENERALIZE)
    }

    pub fn forall_params(&self) -> impl Iterator<Item = &Generic<Id>> {
        forall_params(self)
    }
}

impl TypeVariable {
    pub fn new(var: u32) -> TypeVariable {
        TypeVariable::with_kind(Kind::Type, var)
    }

    pub fn with_kind(kind: Kind, var: u32) -> TypeVariable {
        TypeVariable {
            kind: ArcKind::new(kind),
            id: var,
        }
    }
}

impl fmt::Display for TypeVariable {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.id.fmt(f)
    }
}

impl<Id: fmt::Display> fmt::Display for Generic<Id> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.id.fmt(f)
    }
}

impl fmt::Display for BuiltinType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.to_str().fmt(f)
    }
}

#[derive(PartialEq, Copy, Clone, PartialOrd)]
pub enum Prec {
    /// The type exists in the top context, no parentheses needed.
    Top,
    /// The type exists in a function argument `Type -> a`, parentheses are
    /// needed if the type is a function `(b -> c) -> a`.
    Function,
    /// The type exists in a constructor argument `Option Type`, parentheses
    /// are needed for functions or other constructors `Option (a -> b)`
    /// `Option (Result a b)`.
    Constructor,
}

impl Prec {
    pub fn enclose<'a, A>(
        &self,
        limit: Prec,
        arena: &'a Arena<'a, A>,
        doc: DocBuilder<'a, Arena<'a, A>, A>,
    ) -> DocBuilder<'a, Arena<'a, A>, A> {
        if *self >= limit {
            chain![arena; "(", doc, ")"]
        } else {
            doc
        }
    }
}

#[doc(hidden)]
pub fn dt<T>(prec: Prec, typ: &T) -> DisplayType<T> {
    DisplayType { prec, typ }
}

fn top<T>(typ: &T) -> DisplayType<T> {
    dt(Prec::Top, typ)
}

pub struct DisplayType<'a, T: 'a> {
    prec: Prec,
    typ: &'a T,
}

pub trait ToDoc<'a, A, B, E> {
    fn to_doc(&'a self, allocator: &'a A, env: E) -> DocBuilder<'a, A, B>
    where
        A: DocAllocator<'a, B>;
}

impl<'a, I, A> ToDoc<'a, Arena<'a, A>, A, ()> for ArcType<I>
where
    I: AsRef<str>,
    A: Clone,
{
    fn to_doc(&'a self, arena: &'a Arena<'a, A>, _: ()) -> DocBuilder<'a, Arena<'a, A>, A> {
        self.to_doc(arena, &() as &dyn Source)
    }
}

impl<'a, I, A> ToDoc<'a, Arena<'a, A>, A, &'a dyn Source> for ArcType<I>
where
    I: AsRef<str>,
    A: Clone,
{
    fn to_doc(
        &'a self,
        arena: &'a Arena<'a, A>,
        source: &'a dyn Source,
    ) -> DocBuilder<'a, Arena<'a, A>, A> {
        let printer = Printer::new(arena, source);
        dt(Prec::Top, self).pretty(&printer)
    }
}

fn is_tuple<I, T>(typ: &T) -> bool
where
    I: AsRef<str>,
    T: Deref<Target = Type<I, T>>,
{
    match **typ {
        Type::Record(_) => {
            type_field_iter(typ).next().is_none()
                && row_iter(typ).enumerate().all(|(i, field)| {
                    let name = field.name.as_ref();
                    name.starts_with('_') && name[1..].parse() == Ok(i)
                })
        }
        _ => false,
    }
}

#[macro_export]
macro_rules! chain {
    ($alloc: expr; $first: expr, $($rest: expr),+) => {{
        let mut doc = ::pretty::DocBuilder($alloc, $first.into());
        $(
            doc = doc.append($rest);
        )*
        doc
    }}
}

const INDENT: usize = 4;

impl<'a, I, T> DisplayType<'a, T>
where
    T: Deref<Target = Type<I, T>> + HasSpan + Commented + 'a,
    I: AsRef<str> + 'a,
{
    pub fn pretty<A>(&self, printer: &Printer<'a, I, A>) -> DocBuilder<'a, Arena<'a, A>, A>
    where
        A: Clone,
    {
        let arena = printer.arena;

        let p = self.prec;
        let typ = self.typ;

        let doc = match **typ {
            Type::Hole => arena.text("_"),
            Type::Error => arena.text("!"),
            Type::Opaque => arena.text("<opaque>"),
            Type::Forall(ref args, ref typ) => {
                let doc = chain![arena;
                    chain![arena;
                        "forall ",
                        arena.concat(args.iter().map(|arg| {
                            arena.text(arg.id.as_ref()).append(arena.space())
                        })),
                        "."
                    ].group(),
                    arena.space(),
                    top(typ).pretty(printer)
                ];
                p.enclose(Prec::Function, arena, doc)
            }
            Type::Variable(ref var) => arena.text(format!("{}", var.id)),
            Type::Skolem(ref skolem) => chain![
                arena;
                skolem.name.as_ref(),
                "@",
                skolem.id.to_string()
            ],
            Type::Generic(ref gen) => arena.text(gen.id.as_ref()),
            Type::Function(..) => self.pretty_function(printer).nest(INDENT),
            Type::App(ref t, ref args) => match self.typ.as_function() {
                Some(_) => self.pretty_function(printer).nest(INDENT),
                None => {
                    let doc = dt(Prec::Top, t).pretty(printer);
                    let arg_doc = arena.concat(args.iter().map(|arg| {
                        arena
                            .space()
                            .append(dt(Prec::Constructor, arg).pretty(printer))
                    }));
                    let doc = doc.append(arg_doc.nest(INDENT));
                    p.enclose(Prec::Constructor, arena, doc).group()
                }
            },
            Type::Variant(ref row) => {
                let mut first = true;

                let mut doc = arena.nil();
                let mut row = row;
                loop {
                    row = match **row {
                        Type::EmptyRow => break,
                        Type::ExtendRow {
                            ref fields,
                            ref rest,
                            ..
                        } => {
                            doc = doc.append(arena.concat(fields.iter().map(|field| {
                                let typ = remove_forall(&field.typ);
                                chain![arena;
                                    if first {
                                        first = false;
                                        arena.nil()
                                    } else {
                                        arena.newline()
                                    },
                                    "| ",
                                    field.name.as_ref(),
                                    if typ.as_function().is_some() {
                                        arena.concat(arg_iter(typ).map(|arg| {
                                            chain![arena;
                                                " ",
                                                dt(Prec::Constructor, arg).pretty(printer)
                                            ]
                                        }))
                                    } else {
                                        arena.concat(row_iter(typ).map(|field| {
                                            chain![arena;
                                                " ",
                                                dt(Prec::Constructor, &field.typ).pretty(printer)
                                            ]
                                        }))
                                    }
                                ]
                                .group()
                            })));
                            rest
                        }
                        _ => {
                            doc = chain![arena;
                                doc,
                                arena.newline(),
                                ".. ",
                                top(row).pretty(printer)
                            ];
                            break;
                        }
                    };
                }

                p.enclose(Prec::Constructor, arena, doc).group()
            }

            Type::Effect(ref row) => Self::pretty_record_like(
                row,
                printer,
                "[|",
                &mut |field: &'a Field<I, T>| {
                    chain![arena;
                        pretty_print::doc_comment(arena, field.typ.comment()),
                        pretty_print::ident(arena, field.name.as_ref()),
                        " : "
                    ]
                },
                "|]",
            ),

            Type::Builtin(ref t) => match *t {
                BuiltinType::Function => chain![arena; "(", t.to_str(), ")"],
                _ => arena.text(t.to_str()),
            },
            Type::Record(ref row) => {
                if is_tuple(typ) {
                    Self::pretty_record_like(row, printer, "(", &mut |_| arena.nil(), ")")
                } else {
                    let mut pretty_record_field = |field: &'a Field<I, T>| {
                        chain![arena;
                            pretty_print::doc_comment(arena, field.typ.comment()),
                            pretty_print::ident(arena, field.name.as_ref()),
                            " : "
                        ]
                    };
                    Self::pretty_record_like(row, printer, "{", &mut pretty_record_field, "}")
                }
            }
            Type::ExtendRow { .. } | Type::ExtendTypeRow { .. } => {
                self.pretty_row("{", printer, &mut |field| {
                    chain![arena;
                        pretty_print::doc_comment(arena, field.typ.comment()),
                        pretty_print::ident(arena, field.name.as_ref()),
                        " : "
                    ]
                })
            }
            // This should not be displayed normally as it should only exist in `ExtendRow`
            // which handles `EmptyRow` explicitly
            Type::EmptyRow => arena.text("EmptyRow"),
            Type::Ident(ref id) => printer.symbol_with(id, Name::new(id.as_ref()).name().as_str()),
            Type::Projection(ref ids) => arena.concat(
                ids.iter()
                    .map(|id| printer.symbol(id))
                    .intersperse(arena.text(".")),
            ),
            Type::Alias(ref alias) => printer.symbol(&alias.name),
        };
        match **typ {
            Type::App(..)
            | Type::ExtendRow { .. }
            | Type::ExtendTypeRow { .. }
            | Type::Variant(..)
            | Type::Function(..) => doc,
            _ => {
                let comment = printer.comments_before(typ.span().start());
                comment.append(doc)
            }
        }
    }

    fn pretty_record_like<A>(
        row: &'a T,
        printer: &Printer<'a, I, A>,
        open: &'static str,
        pretty_field: &mut dyn FnMut(&'a Field<I, T>) -> DocBuilder<'a, Arena<'a, A>, A>,
        close: &'static str,
    ) -> DocBuilder<'a, Arena<'a, A>, A>
    where
        A: Clone,
    {
        let arena = printer.arena;

        let forced_newline = row_iter(row).any(|field| field.typ.comment().is_some());

        let newline = if forced_newline {
            arena.newline()
        } else {
            arena.space()
        };

        let mut doc = arena.text(open);
        let empty_fields = match **row {
            Type::EmptyRow => true,
            _ => false,
        };

        doc = match **row {
            Type::EmptyRow => doc,
            Type::ExtendRow { .. } | Type::ExtendTypeRow { .. } => doc
                .append(top(row).pretty_row(open, printer, pretty_field))
                .nest(INDENT),
            _ => doc
                .append(arena.space())
                .append("| ")
                .append(top(row).pretty(printer))
                .nest(INDENT),
        };
        if !empty_fields && open != "(" {
            doc = doc.append(newline);
        }

        doc.append(close).group()
    }

    fn pretty_row<A>(
        &self,
        open: &str,
        printer: &Printer<'a, I, A>,
        pretty_field: &mut dyn FnMut(&'a Field<I, T>) -> DocBuilder<'a, Arena<'a, A>, A>,
    ) -> DocBuilder<'a, Arena<'a, A>, A>
    where
        A: Clone,
    {
        let arena = printer.arena;

        let mut doc = arena.nil();
        let mut typ = self.typ;

        let fields = {
            let mut typ = typ;
            loop {
                match **typ {
                    Type::ExtendRow { ref fields, .. } => break &fields[..],
                    Type::ExtendTypeRow { ref rest, .. } => typ = rest,
                    _ => break &[][..],
                }
            }
        };
        let forced_newline = fields.iter().any(|field| field.typ.comment().is_some());

        let newline = if forced_newline {
            arena.newline()
        } else {
            arena.space()
        };

        let print_any_field = fields
            .iter()
            .any(|field| printer.filter(&field.name) != Filter::Drop);

        let mut filtered = false;

        let types_len = type_field_iter(typ).count();
        for (i, field) in type_field_iter(typ).enumerate() {
            let filter = printer.filter(&field.name);
            if filter == Filter::Drop {
                filtered = true;
                continue;
            }

            let f = chain![arena;
                field.name.as_ref(),
                arena.space(),
                arena.concat(field.typ.params().iter().map(|param| {
                    arena.text(param.id.as_ref()).append(arena.space())
                })),
                arena.text("= "),
                if filter == Filter::RetainKey {
                    arena.text("...")
                } else {
                     top(&field.typ.typ).pretty(printer)
                },
                if i + 1 != types_len || print_any_field {
                    arena.text(",")
                } else {
                    arena.nil()
                }
            ]
            .group();
            doc = doc.append(newline.clone()).append(f);
        }

        let mut row_iter = row_iter(typ);
        for (i, field) in row_iter.by_ref().enumerate() {
            let filter = printer.filter(&field.name);
            if filter == Filter::Drop {
                filtered = true;
                continue;
            }

            let mut rhs = if filter == Filter::RetainKey {
                arena.text("...")
            } else {
                top(&field.typ).pretty(printer)
            };
            match *field.typ {
                // Records handle nesting on their own
                Type::Record(_) => (),
                _ => rhs = rhs.nest(INDENT),
            }
            let f = chain![arena;
                pretty_field(field),
                rhs.group(),
                if i + 1 != fields.len() {
                    arena.text(",")
                } else {
                    arena.nil()
                }
            ]
            .group();
            let space_before = if i == 0 && open == "(" {
                arena.nil()
            } else {
                newline.clone()
            };
            doc = doc.append(space_before).append(f);
        }
        typ = row_iter.typ;

        let doc = if filtered {
            if let Doc::Nil = doc.1 {
                chain![arena;
                    newline.clone(),
                    "..."
                ]
            } else {
                chain![arena;
                    newline.clone(),
                    "...,",
                    doc,
                    if let Doc::Space = newline.1 {
                        arena.text(",")
                    } else {
                        arena.nil()
                    },
                    newline.clone(),
                    "..."
                ]
            }
        } else {
            doc
        };
        match **typ {
            Type::EmptyRow => doc,
            _ => doc
                .append(arena.space())
                .append("| ")
                .append(top(typ).pretty(printer)),
        }
    }

    fn pretty_function<A>(&self, printer: &Printer<'a, I, A>) -> DocBuilder<'a, Arena<'a, A>, A>
    where
        I: AsRef<str>,
        A: Clone,
    {
        let arena = printer.arena;
        let doc = self.pretty_function_(printer);
        self.prec.enclose(Prec::Function, arena, doc).group()
    }

    fn pretty_function_<A>(&self, printer: &Printer<'a, I, A>) -> DocBuilder<'a, Arena<'a, A>, A>
    where
        I: AsRef<str>,
        A: Clone,
    {
        let arena = printer.arena;
        match self.typ.as_function_with_type() {
            Some((arg_type, arg, ret)) => chain![arena;
                if arg_type == ArgType::Implicit { "[" } else { "" },
                dt(Prec::Function, arg).pretty(printer),
                if arg_type == ArgType::Implicit { "]" } else { "" },
                printer.space_after(arg.span().end()),
                "-> ",
                top(ret).pretty_function_(printer)
            ],
            None => self.pretty(printer),
        }
    }
}

pub fn pretty_print<'a, I, T, A>(
    printer: &Printer<'a, I, A>,
    typ: &'a T,
) -> DocBuilder<'a, Arena<'a, A>, A>
where
    I: AsRef<str> + 'a,
    T: Deref<Target = Type<I, T>> + HasSpan + Commented,
    A: Clone,
{
    dt(Prec::Top, typ).pretty(printer)
}

pub fn walk_type<'a, I, T, F>(typ: &'a T, mut f: F)
where
    F: Walker<'a, T>,
    T: Deref<Target = Type<I, T>> + 'a,
    I: 'a,
{
    f.walk(typ)
}

pub fn walk_type_<'a, I, T, F: ?Sized>(typ: &'a T, f: &mut F)
where
    F: Walker<'a, T>,
    T: Deref<Target = Type<I, T>> + 'a,
    I: 'a,
{
    match **typ {
        Type::Function(_, ref arg, ref ret) => {
            f.walk(arg);
            f.walk(ret);
        }
        Type::App(ref t, ref args) => {
            f.walk(t);
            for a in args {
                f.walk(a);
            }
        }
        Type::Forall(_, ref typ)
        | Type::Record(ref typ)
        | Type::Variant(ref typ)
        | Type::Effect(ref typ) => f.walk(typ),
        Type::ExtendRow {
            ref fields,
            ref rest,
        } => {
            for field in fields {
                f.walk(&field.typ);
            }
            f.walk(rest);
        }
        Type::ExtendTypeRow {
            ref types,
            ref rest,
        } => {
            for field in types {
                f.walk(&field.typ.typ);
            }
            f.walk(rest);
        }
        Type::Hole
        | Type::Opaque
        | Type::Error
        | Type::Builtin(_)
        | Type::Variable(_)
        | Type::Generic(_)
        | Type::Skolem(_)
        | Type::Ident(_)
        | Type::Projection(_)
        | Type::Alias(_)
        | Type::EmptyRow => (),
    }
}

pub fn walk_type_mut<I, T, F: ?Sized>(typ: &mut T, f: &mut F)
where
    F: WalkerMut<T>,
    T: DerefMut<Target = Type<I, T>>,
{
    match **typ {
        Type::Forall(_, ref mut typ) => f.walk_mut(typ),
        Type::Function(_, ref mut arg, ref mut ret) => {
            f.walk_mut(arg);
            f.walk_mut(ret);
        }
        Type::App(ref mut t, ref mut args) => {
            f.walk_mut(t);
            for a in args {
                f.walk_mut(a);
            }
        }
        Type::Record(ref mut row) | Type::Variant(ref mut row) | Type::Effect(ref mut row) => {
            f.walk_mut(row)
        }
        Type::ExtendRow {
            ref mut fields,
            ref mut rest,
        } => {
            for field in fields {
                f.walk_mut(&mut field.typ);
            }
            f.walk_mut(rest);
        }
        Type::ExtendTypeRow {
            ref mut types,
            ref mut rest,
        } => {
            for field in types {
                if let Some(alias) = field.typ.try_get_alias_mut() {
                    let field_type = alias.unresolved_type_mut();
                    f.walk_mut(field_type);
                }
            }
            f.walk_mut(rest);
        }
        Type::Hole
        | Type::Opaque
        | Type::Error
        | Type::Builtin(_)
        | Type::Variable(_)
        | Type::Generic(_)
        | Type::Skolem(_)
        | Type::Ident(_)
        | Type::Projection(_)
        | Type::Alias(_)
        | Type::EmptyRow => (),
    }
}

pub fn fold_type<I, T, F, A>(typ: &T, mut f: F, a: A) -> A
where
    F: FnMut(&T, A) -> A,
    T: Deref<Target = Type<I, T>>,
{
    let mut a = Some(a);
    walk_type(typ, |t| {
        a = Some(f(t, a.take().expect("None in fold_type")));
    });
    a.expect("fold_type")
}

pub trait TypeVisitor<Id, T> {
    fn visit(&mut self, typ: &T) -> Option<T>
    where
        T: Deref<Target = Type<Id, T>> + Clone,
        Id: Clone,
    {
        walk_move_type_opt(typ, self)
    }

    fn make(&mut self, typ: Type<Id, T>) -> T;

    fn forall(&mut self, params: Vec<Generic<Id>>, typ: T) -> T {
        if params.is_empty() {
            typ
        } else {
            self.make(Type::Forall(params, typ))
        }
    }

    fn app(&mut self, id: T, args: AppVec<T>) -> T {
        if args.is_empty() {
            id
        } else {
            self.make(Type::App(id, args))
        }
    }
}

pub trait Walker<'a, T> {
    fn walk(&mut self, typ: &'a T);
}

impl<I, T, F: ?Sized> TypeVisitor<I, T> for F
where
    F: FnMut(&T) -> Option<T>,
    T: From<Type<I, T>>,
{
    fn visit(&mut self, typ: &T) -> Option<T>
    where
        T: Deref<Target = Type<I, T>> + From<Type<I, T>> + Clone,
        I: Clone,
    {
        let new_type = walk_move_type_opt(typ, self);
        let new_type2 = self(new_type.as_ref().map_or(typ, |t| t));
        new_type2.or(new_type)
    }

    fn make(&mut self, typ: Type<I, T>) -> T {
        T::from(typ)
    }
}

#[macro_export(local_inner_macros)]
macro_rules! expr {
    ($self: ident, $id: ident, $expr: expr) => {{
        let $id = $self;
        $expr
    }};
}

#[macro_export(local_inner_macros)]
macro_rules! forward_type_interner_methods_no_arg {
    ($typ: ty, [$($tokens: tt)+] $id: ident $($rest : ident)* ) => {
        fn $id(&mut self) -> $typ {
            $crate::expr!(self, $($tokens)+).$id()
        }
        $crate::forward_type_interner_methods_no_arg!($typ, [$($tokens)+] $($rest)*);
    };

    ($typ: ty, [$($tokens: tt)+]) => {
    }
}

#[macro_export]
macro_rules! forward_type_interner_methods {
    ($id: ty, $typ: ty, $($tokens: tt)+ ) => {
        fn intern(&mut self, typ: $crate::types::Type<$id, $typ>) -> $typ {
            $crate::expr!(self, $($tokens)+).intern(typ)
        }

        fn intern_flags(&mut self, typ: $crate::types::Type<$id, $typ>, flags: $crate::types::Flags) -> $typ {
            $crate::expr!(self, $($tokens)+).intern_flags(typ, flags)
        }

        fn builtin(&mut self, typ: $crate::types::BuiltinType) -> $typ {
            $crate::expr!(self, $($tokens)+).builtin(typ)
        }

        fn forall(&mut self, params: Vec<$crate::types::Generic<$id>>, typ: $typ) -> $typ {
            $crate::expr!(self, $($tokens)+).forall(params, typ)
        }

        fn with_forall(&mut self, typ: $typ, from: &$typ) -> $typ
        where
            $id: Clone + Eq + std::hash::Hash,
            $typ: $crate::types::TypeExt<Id = $id> + Clone,
        {
            $crate::expr!(self, $($tokens)+).with_forall(typ, from)
        }

        fn array(&mut self, typ: $typ) -> $typ {
            $crate::expr!(self, $($tokens)+).array(typ)
        }

        fn app(&mut self, id: $typ, args: $crate::types::AppVec<$typ>) -> $typ {
            $crate::expr!(self, $($tokens)+).app(id, args)
        }

        fn variant(&mut self, fields: Vec<$crate::types::Field<$id, $typ>>) -> $typ {
            $crate::expr!(self, $($tokens)+).variant(fields)
        }

        fn poly_variant(&mut self, fields: Vec<$crate::types::Field<$id, $typ>>, rest: $typ) -> $typ {
            $crate::expr!(self, $($tokens)+).poly_variant(fields, rest)
        }

        fn effect(&mut self, fields: Vec<$crate::types::Field<$id, $typ>>) -> $typ {
            $crate::expr!(self, $($tokens)+).effect(fields)
        }

        fn poly_effect(&mut self, fields: Vec<$crate::types::Field<$id, $typ>>, rest: $typ) -> $typ {
            $crate::expr!(self, $($tokens)+).poly_effect(fields, rest)
        }

        fn record(&mut self, types: Vec<$crate::types::Field<$id, $crate::types::Alias<$id, $typ>>>, fields: Vec<$crate::types::Field<$id, $typ>>) -> $typ {
            $crate::expr!(self, $($tokens)+).record(types, fields)
        }

        fn poly_record(
            &mut self,
            types: Vec<$crate::types::Field<$id, $crate::types::Alias<$id, $typ>>>,
            fields: Vec<$crate::types::Field<$id, $typ>>,
            rest: $typ,
        ) -> $typ {
            $crate::expr!(self, $($tokens)+).poly_record(types, fields, rest)
        }

        fn extend_full_row(
            &mut self,
            types: Vec<$crate::types::Field<$id, $crate::types::Alias<$id, $typ>>>,
            fields: Vec<$crate::types::Field<$id, $typ>>,
            rest: $typ,
        ) -> $typ {
            $crate::expr!(self, $($tokens)+).extend_full_row(types, fields, rest)
        }

        fn extend_row(
            &mut self,
            fields: Vec<$crate::types::Field<$id, $typ>>,
            rest: $typ,
        ) -> $typ {
            $crate::expr!(self, $($tokens)+).extend_row(fields, rest)
        }

        fn extend_type_row(
            &mut self,
            types: Vec<$crate::types::Field<$id, $crate::types::Alias<$id, $typ>>>,
            rest: $typ,
        ) -> $typ {
            $crate::expr!(self, $($tokens)+).extend_type_row(types, rest)
        }

        fn generic(&mut self, typ: $crate::types::Generic<$id>) -> $typ {
            $crate::expr!(self, $($tokens)+).generic(typ)
        }

        fn skolem(&mut self, typ: $crate::types::Skolem<$id>) -> $typ {
            $crate::expr!(self, $($tokens)+).skolem(typ)
        }

        fn variable(&mut self, typ: $crate::types::TypeVariable) -> $typ {
            $crate::expr!(self, $($tokens)+).variable(typ)
        }

        fn alias(&mut self, name: $id, args: Vec<$crate::types::Generic<$id>>, typ: $typ) -> $typ {
            $crate::expr!(self, $($tokens)+).alias(name, args, typ)
        }

        fn ident(&mut self, id: $id) -> $typ {
            $crate::expr!(self, $($tokens)+).ident(id)
        }

        fn projection(&mut self, id: $crate::types::AppVec<$id>) -> $typ {
            $crate::expr!(self, $($tokens)+).projection(id)
        }

        fn builtin_type(&mut self, typ: $crate::types::BuiltinType) -> $typ {
            $crate::expr!(self, $($tokens)+).builtin_type(typ)
        }

        fn new_alias(&mut self, name: $id, args: Vec<$crate::types::Generic<$id>>, typ: $typ) -> $crate::types::Alias<$id, $typ> {
            $crate::expr!(self, $($tokens)+).new_alias(name, args, typ)
        }

        fn new_data_alias(&mut self, data: $crate::types::AliasData<$id, $typ>) -> $crate::types::Alias<$id, $typ> {
            $crate::expr!(self, $($tokens)+).new_data_alias(data)
        }

        fn alias_group(
            &mut self,
            group: Vec<$crate::types::AliasData<$id, $typ>>,
            ) -> Vec<$crate::types::Alias<$id, $typ>>
        where
            $typ: $crate::types::TypeExt<Id = $id>,
            $id: PartialEq,
        {
            $crate::expr!(self, $($tokens)+).alias_group(group)
        }

        /*
           Avoid forwarding methods that take an iterator as they can cause panics when using `RefCell`

        fn tuple<S, I>(&mut self, symbols: &mut S, elems: I) -> $typ
        where
            S: ?Sized + $crate::ast::IdentEnv<Ident = $id>,
            I: IntoIterator<Item = $typ>,
        {
            $crate::expr!(self, $($tokens)+).tuple(symbols, elems)
        }

        fn tuple_<S, I>(&mut self, symbols: &mut S, elems: I) -> $crate::types::Type<$id, $typ>
        where
            S: ?Sized + $crate::ast::IdentEnv<Ident = $id>,
            I: IntoIterator<Item = $typ>,
        {
            $crate::expr!(self, $($tokens)+).tuple_(symbols, elems)
        }

        fn function<I>(&mut self, args: I, ret: $typ) -> $typ
        where
            $typ: Clone,
            I: IntoIterator<Item = $typ>,
            I::IntoIter: DoubleEndedIterator<Item = $typ>,
        {
            $crate::expr!(self, $($tokens)+).function( args, ret)
        }

        fn function_implicit<I>(&mut self, args: I, ret: $typ) -> $typ
        where
            I: IntoIterator<Item = $typ>,
            I::IntoIter: DoubleEndedIterator<Item = $typ>,
        {
            $crate::expr!(self, $($tokens)+).function_implicit(args, ret)
        }

        fn function_type<I>(&mut self, arg_type: $crate::types::ArgType, args: I, ret: $typ) -> $typ
        where
            I: IntoIterator<Item = $typ>,
            I::IntoIter: DoubleEndedIterator<Item = $typ>,
        {
            $crate::expr!(self, $($tokens)+).function_type(arg_type, args, ret)
        }
        */

        $crate::forward_type_interner_methods_no_arg!(
            $typ,
            [$($tokens)+]
            hole opaque error array_builtin empty_row function_builtin string char byte int float unit
        );
    }
}

pub struct InternerVisitor<'i, F, T> {
    interner: &'i mut T,
    visitor: F,
}

pub trait TypeContext<Id, T> {
    fn intern_flags(&mut self, typ: Type<Id, T>, flags: Flags) -> T;

    fn intern(&mut self, typ: Type<Id, T>) -> T;

    fn hole(&mut self) -> T {
        self.intern(Type::Hole)
    }

    fn opaque(&mut self) -> T {
        self.intern(Type::Opaque)
    }

    fn error(&mut self) -> T {
        self.intern(Type::Error)
    }

    fn builtin(&mut self, typ: BuiltinType) -> T {
        self.intern(Type::Builtin(typ))
    }

    fn forall(&mut self, params: Vec<Generic<Id>>, typ: T) -> T {
        if params.is_empty() {
            typ
        } else {
            self.intern(Type::Forall(params, typ))
        }
    }

    fn with_forall(&mut self, typ: T, from: &T) -> T
    where
        Id: Clone + Eq + Hash,
        T: TypeExt<Id = Id> + Clone,
    {
        let params = forall_params(from).cloned().collect();
        self.forall(params, typ)
    }

    fn array(&mut self, typ: T) -> T {
        let a = self.array_builtin();
        self.app(a, collect![typ])
    }

    fn array_builtin(&mut self) -> T {
        self.builtin(BuiltinType::Array)
    }

    fn app(&mut self, id: T, args: AppVec<T>) -> T {
        if args.is_empty() {
            id
        } else {
            self.intern(Type::App(id, args))
        }
    }

    fn variant(&mut self, fields: Vec<Field<Id, T>>) -> T {
        let empty_row = self.empty_row();
        self.poly_variant(fields, empty_row)
    }

    fn poly_variant(&mut self, fields: Vec<Field<Id, T>>, rest: T) -> T {
        let row = self.extend_row(fields, rest);
        self.intern(Type::Variant(row))
    }

    fn effect(&mut self, fields: Vec<Field<Id, T>>) -> T {
        let empty_row = self.empty_row();
        self.poly_effect(fields, empty_row)
    }

    fn poly_effect(&mut self, fields: Vec<Field<Id, T>>, rest: T) -> T {
        let extend_row = self.extend_row(fields, rest);
        self.intern(Type::Effect(extend_row))
    }

    fn tuple<S, I>(&mut self, symbols: &mut S, elems: I) -> T
    where
        S: ?Sized + IdentEnv<Ident = Id>,
        I: IntoIterator<Item = T>,
    {
        let t = self.tuple_(symbols, elems);
        self.intern(t)
    }

    fn tuple_<S, I>(&mut self, symbols: &mut S, elems: I) -> Type<Id, T>
    where
        S: ?Sized + IdentEnv<Ident = Id>,
        I: IntoIterator<Item = T>,
    {
        let empty_row = self.empty_row();
        Type::Record(
            self.extend_row(
                elems
                    .into_iter()
                    .enumerate()
                    .map(|(i, typ)| Field {
                        name: symbols.from_str(&format!("_{}", i)),
                        typ,
                    })
                    .collect(),
                empty_row,
            ),
        )
    }

    fn record(&mut self, types: Vec<Field<Id, Alias<Id, T>>>, fields: Vec<Field<Id, T>>) -> T {
        let empty_row = self.empty_row();
        self.poly_record(types, fields, empty_row)
    }

    fn poly_record(
        &mut self,
        types: Vec<Field<Id, Alias<Id, T>>>,
        fields: Vec<Field<Id, T>>,
        rest: T,
    ) -> T {
        let row = self.extend_full_row(types, fields, rest);
        self.intern(Type::Record(row))
    }

    fn extend_full_row(
        &mut self,
        types: Vec<Field<Id, Alias<Id, T>>>,
        fields: Vec<Field<Id, T>>,
        rest: T,
    ) -> T {
        let rest = self.extend_row(fields, rest);
        self.extend_type_row(types, rest)
    }

    fn extend_row(&mut self, fields: Vec<Field<Id, T>>, rest: T) -> T {
        if fields.is_empty() {
            rest
        } else {
            self.intern(Type::ExtendRow { fields, rest })
        }
    }

    fn extend_type_row(&mut self, types: Vec<Field<Id, Alias<Id, T>>>, rest: T) -> T {
        if types.is_empty() {
            rest
        } else {
            self.intern(Type::ExtendTypeRow { types, rest })
        }
    }

    fn empty_row(&mut self) -> T {
        self.intern(Type::EmptyRow)
    }

    fn function<I>(&mut self, args: I, ret: T) -> T
    where
        T: Clone,
        I: IntoIterator<Item = T>,
        I::IntoIter: DoubleEndedIterator<Item = T>,
    {
        self.function_type(ArgType::Explicit, args, ret)
    }

    fn function_implicit<I>(&mut self, args: I, ret: T) -> T
    where
        I: IntoIterator<Item = T>,
        I::IntoIter: DoubleEndedIterator<Item = T>,
    {
        self.function_type(ArgType::Implicit, args, ret)
    }

    fn function_type<I>(&mut self, arg_type: ArgType, args: I, ret: T) -> T
    where
        I: IntoIterator<Item = T>,
        I::IntoIter: DoubleEndedIterator<Item = T>,
    {
        args.into_iter().rev().fold(ret, |body, arg| {
            self.intern(Type::Function(arg_type, arg, body))
        })
    }

    fn generic(&mut self, typ: Generic<Id>) -> T {
        self.intern(Type::Generic(typ))
    }

    fn skolem(&mut self, typ: Skolem<Id>) -> T {
        self.intern(Type::Skolem(typ))
    }

    fn variable(&mut self, typ: TypeVariable) -> T {
        self.intern(Type::Variable(typ))
    }

    fn alias(&mut self, name: Id, args: Vec<Generic<Id>>, typ: T) -> T {
        self.intern(Type::Alias(AliasRef {
            index: 0,
            group: Arc::from(vec![AliasData {
                name,
                args,
                typ,
                is_implicit: false,
            }]),
        }))
    }

    fn ident(&mut self, id: Id) -> T {
        self.intern(Type::Ident(id))
    }

    fn projection(&mut self, id: AppVec<Id>) -> T {
        self.intern(Type::Projection(id))
    }

    fn function_builtin(&mut self) -> T {
        self.builtin(BuiltinType::Function)
    }

    fn string(&mut self) -> T {
        self.builtin(BuiltinType::String)
    }

    fn char(&mut self) -> T {
        self.builtin(BuiltinType::Char)
    }

    fn byte(&mut self) -> T {
        self.builtin(BuiltinType::Byte)
    }

    fn int(&mut self) -> T {
        self.builtin(BuiltinType::Int)
    }

    fn float(&mut self) -> T {
        self.builtin(BuiltinType::Float)
    }

    fn unit(&mut self) -> T {
        self.record(vec![], vec![])
    }

    fn builtin_type(&mut self, typ: BuiltinType) -> T {
        match typ {
            BuiltinType::String => self.string(),
            BuiltinType::Byte => self.byte(),
            BuiltinType::Char => self.char(),
            BuiltinType::Int => self.int(),
            BuiltinType::Float => self.float(),
            BuiltinType::Array => self.array_builtin(),
            BuiltinType::Function => self.function_builtin(),
        }
    }

    fn new_alias(&mut self, name: Id, args: Vec<Generic<Id>>, typ: T) -> Alias<Id, T> {
        Alias {
            _typ: self.alias(name, args, typ),
            _marker: PhantomData,
        }
    }

    fn new_data_alias(&mut self, data: AliasData<Id, T>) -> Alias<Id, T> {
        Alias {
            _typ: self.intern(Type::Alias(AliasRef {
                index: 0,
                group: Arc::from(vec![data]),
            })),
            _marker: PhantomData,
        }
    }

    fn alias_group(&mut self, group: Vec<AliasData<Id, T>>) -> Vec<Alias<Id, T>>
    where
        T: TypeExt<Id = Id>,
        Id: PartialEq,
    {
        let group = Arc::<[_]>::from(group);
        (0..group.len())
            .map(|index| {
                let typ = Type::Alias(AliasRef {
                    index,
                    group: group.clone(),
                });
                let flags = Flags::from_type(&typ)
                    | (if group[index].is_implicit {
                        Flags::HAS_IMPLICIT
                    } else {
                        Flags::empty()
                    });

                Alias {
                    _typ: self.intern_flags(typ, flags),
                    _marker: PhantomData,
                }
            })
            .collect()
    }
}

pub trait Substitution<Id, T>: TypeContext<Id, T> {
    fn new_var(&mut self) -> T;
    fn new_skolem(&mut self, name: Id, kind: ArcKind) -> T;
}

impl<'b, Id, T, V> TypeContext<Id, T> for &'b Rc<V>
where
    for<'a> &'a V: TypeContext<Id, T>,
{
    forward_type_interner_methods!(Id, T, self_, &***self_);
}

impl<Id, T, V> TypeContext<Id, T> for Rc<V>
where
    for<'a> &'a V: TypeContext<Id, T>,
{
    forward_type_interner_methods!(Id, T, self_, &**self_);
}

impl<'a, Id, T, V> TypeContext<Id, T> for &'a RefCell<V>
where
    V: TypeContext<Id, T>,
{
    forward_type_interner_methods!(Id, T, self_, self_.borrow_mut());
}

pub type SharedInterner<Id, T> = TypeCache<Id, T>;

#[derive(Default, Clone)]
pub struct NullInterner;

impl<Id, T> TypeContext<Id, T> for NullInterner
where
    T: From<(Type<Id, T>, Flags)> + From<Type<Id, T>>,
{
    fn intern(&mut self, typ: Type<Id, T>) -> T {
        T::from(typ)
    }

    fn intern_flags(&mut self, typ: Type<Id, T>, flags: Flags) -> T {
        T::from((typ, flags))
    }
}

macro_rules! forward_to_cache {
    ($($id: ident)*) => {
        $(
            fn $id(&mut self) -> T {
                TypeCache::$id(self)
            }
        )*
    }
}

impl<Id, T> TypeContext<Id, T> for TypeCache<Id, T>
where
    T: From<(Type<Id, T>, Flags)> + From<Type<Id, T>> + Clone,
{
    fn intern(&mut self, typ: Type<Id, T>) -> T {
        T::from(typ)
    }

    fn intern_flags(&mut self, typ: Type<Id, T>, flags: Flags) -> T {
        T::from((typ, flags))
    }

    forward_to_cache! {
        hole opaque error int byte float string char
        function_builtin array_builtin unit empty_row
    }
}

impl<'a, Id, T> TypeContext<Id, T> for &'a TypeCache<Id, T>
where
    T: From<(Type<Id, T>, Flags)> + From<Type<Id, T>> + Clone,
{
    fn intern(&mut self, typ: Type<Id, T>) -> T {
        T::from(typ)
    }

    fn intern_flags(&mut self, typ: Type<Id, T>, flags: Flags) -> T {
        T::from((typ, flags))
    }

    forward_to_cache! {
        hole opaque error int byte float string char
        function_builtin array_builtin unit empty_row
    }
}

#[derive(Clone, Default)]
struct Interned<T>(T);

impl<T> Eq for Interned<T>
where
    T: Deref,
    T::Target: Eq,
{
}

impl<T> PartialEq for Interned<T>
where
    T: Deref,
    T::Target: PartialEq,
{
    #[inline(always)]
    fn eq(&self, other: &Self) -> bool {
        *self.0 == *other.0
    }
}

impl<T> std::hash::Hash for Interned<T>
where
    T: Deref,
    T::Target: std::hash::Hash,
{
    #[inline(always)]
    fn hash<H>(&self, state: &mut H)
    where
        H: std::hash::Hasher,
    {
        (*self.0).hash(state)
    }
}

impl<Id, T> Borrow<Type<Id, T>> for Interned<T>
where
    T: Deref<Target = Type<Id, T>>,
{
    fn borrow(&self) -> &Type<Id, T> {
        &self.0
    }
}

pub trait TypeContextAlloc: Sized {
    type Id;
    fn alloc(into: &mut Self, typ: Type<Self::Id, Self>, flags: Flags);
}

impl TypeContextAlloc for ArcType {
    type Id = Symbol;
    fn alloc(into: &mut Self, typ: Type<Symbol, Self>, flags: Flags) {
        match Arc::get_mut(&mut into.typ) {
            Some(into) => {
                into.flags = flags;
                into.typ = typ;
            }
            None => {
                *into = Self::with_flags(typ, flags);
            }
        }
    }
}

pub struct Interner<Id, T> {
    set: FnvMap<Interned<T>, ()>,
    scratch: Interned<T>,
    type_cache: TypeCache<Id, T>,
}

impl<Id, T> Default for Interner<Id, T>
where
    T: Default + Deref + From<Type<Id, T>> + Clone,
    T::Target: Eq + Hash,
{
    fn default() -> Self {
        let mut interner = Interner {
            set: Default::default(),
            scratch: Default::default(),
            type_cache: Default::default(),
        };

        macro_rules! populate_builtins {
            ($($id: ident)*) => {
                $(
                    let x = interner.type_cache.$id();
                    interner.set.insert(Interned(x), ());
                )*
            }
        }

        populate_builtins! {
            hole opaque error int byte float string char
            function_builtin array_builtin unit empty_row
        }

        interner
    }
}

macro_rules! forward_to_intern_cache {
    ($($id: ident)*) => {
        $(
            fn $id(&mut self) -> T {
                self.type_cache.$id()
            }
        )*
    }
}

impl<Id, T> TypeContext<Id, T> for Interner<Id, T>
where
    T: TypeContextAlloc<Id = Id> + TypeExt<Id = Id> + Eq + Hash + Clone,
    Id: Eq + Hash,
{
    fn intern(&mut self, typ: Type<Id, T>) -> T {
        let flags = Flags::from_type(&typ);
        self.intern_flags(typ, flags)
    }

    fn intern_flags(&mut self, typ: Type<Id, T>, flags: Flags) -> T {
        use std::collections::hash_map::Entry;

        T::alloc(&mut self.scratch.0, typ, flags);
        match self.set.entry(self.scratch.clone()) {
            Entry::Occupied(entry) => return entry.key().0.clone(),
            Entry::Vacant(entry) => {
                entry.insert(());
                self.scratch.0.clone()
            }
        }
    }

    forward_to_intern_cache! {
        hole opaque error int byte float string char
        function_builtin array_builtin unit empty_row
    }
}

impl<'i, F, V> InternerVisitor<'i, F, V> {
    pub fn new<I, T>(interner: &'i mut V, visitor: F) -> Self
    where
        F: FnMut(&mut V, &T) -> Option<T>,
        T: TypeExt<Id = I>,
        V: TypeContext<I, T>,
    {
        InternerVisitor { interner, visitor }
    }

    pub fn control<I, T>(
        interner: &'i mut V,
        visitor: F,
    ) -> InternerVisitor<'i, ControlVisitation<F>, V>
    where
        F: FnMut(&mut V, &T) -> Option<T>,
        T: TypeExt<Id = I>,
        V: TypeContext<I, T>,
    {
        InternerVisitor {
            interner,
            visitor: ControlVisitation(visitor),
        }
    }
}

impl<'i, F, V, I, T> TypeVisitor<I, T> for InternerVisitor<'i, F, V>
where
    F: FnMut(&mut V, &T) -> Option<T>,
    T: TypeExt<Id = I>,
    V: TypeContext<I, T>,
{
    fn visit(&mut self, typ: &T) -> Option<T>
    where
        T: Deref<Target = Type<I, T>> + Clone,
        I: Clone,
    {
        let new_type = walk_move_type_opt(typ, self);
        let new_type2 = (self.visitor)(self.interner, new_type.as_ref().map_or(typ, |t| t));
        new_type2.or(new_type)
    }

    fn make(&mut self, typ: Type<I, T>) -> T {
        self.interner.intern(typ)
    }
}

impl<'i, F, V, I, T> TypeVisitor<I, T> for InternerVisitor<'i, ControlVisitation<F>, V>
where
    F: FnMut(&mut V, &T) -> Option<T>,
    T: TypeExt<Id = I>,
    V: TypeContext<I, T>,
{
    fn visit(&mut self, typ: &T) -> Option<T>
    where
        T: Deref<Target = Type<I, T>> + Clone,
        I: Clone,
    {
        (self.visitor.0)(self.interner, typ)
    }

    fn make(&mut self, typ: Type<I, T>) -> T {
        self.interner.intern(typ)
    }
}

/// Wrapper type which allows functions to control how to traverse the members of the type
pub struct ControlVisitation<F: ?Sized>(pub F);

impl<F, I, T> TypeVisitor<I, T> for ControlVisitation<F>
where
    F: FnMut(&T) -> Option<T>,
    T: From<Type<I, T>>,
{
    fn visit(&mut self, typ: &T) -> Option<T>
    where
        T: Deref<Target = Type<I, T>> + From<Type<I, T>> + Clone,
        I: Clone,
    {
        (self.0)(typ)
    }

    fn make(&mut self, typ: Type<I, T>) -> T {
        T::from(typ)
    }
}

impl<'a, F, T> Walker<'a, T> for ControlVisitation<F>
where
    F: ?Sized + FnMut(&'a T),
    T: 'a,
{
    fn walk(&mut self, typ: &'a T) {
        (self.0)(typ)
    }
}

pub struct FlagsVisitor<F: ?Sized>(pub Flags, pub F);

impl<F, I, T> TypeVisitor<I, T> for FlagsVisitor<F>
where
    F: FnMut(&T) -> Option<T>,
    T: From<Type<I, T>> + TypeExt<Id = I>,
{
    fn visit(&mut self, typ: &T) -> Option<T>
    where
        T: Deref<Target = Type<I, T>> + From<Type<I, T>> + Clone,
        I: Clone,
    {
        if self.0.intersects(typ.flags()) {
            let new_type = walk_move_type_opt(typ, self);
            let new_type2 = (self.1)(new_type.as_ref().map_or(typ, |t| t));
            new_type2.or(new_type)
        } else {
            None
        }
    }

    fn make(&mut self, typ: Type<I, T>) -> T {
        T::from(typ)
    }
}

impl<'a, T, F> Walker<'a, T> for FlagsVisitor<F>
where
    F: ?Sized + FnMut(&'a T),
    T: TypeExt + 'a,
{
    fn walk(&mut self, typ: &'a T) {
        if self.0.intersects(typ.flags()) {
            (self.1)(typ);
            walk_type_(typ, self);
        }
    }
}

impl<'a, I, T, F> Walker<'a, T> for F
where
    F: ?Sized + FnMut(&'a T),
    T: Deref<Target = Type<I, T>> + 'a,
    I: 'a,
{
    fn walk(&mut self, typ: &'a T) {
        self(typ);
        walk_type_(typ, self)
    }
}

pub trait WalkerMut<T> {
    fn walk_mut(&mut self, typ: &mut T);
}

impl<I, T, F: ?Sized> WalkerMut<T> for F
where
    F: FnMut(&mut T),
    T: DerefMut<Target = Type<I, T>>,
{
    fn walk_mut(&mut self, typ: &mut T) {
        self(typ);
        walk_type_mut(typ, self)
    }
}

/// Walks through a type calling `f` on each inner type. If `f` return `Some` the type is replaced.
pub fn walk_move_type<F: ?Sized, I, T>(typ: T, f: &mut F) -> T
where
    F: TypeVisitor<I, T>,
    T: Deref<Target = Type<I, T>> + Clone,
    I: Clone,
{
    f.visit(&typ).unwrap_or(typ)
}

pub fn visit_type_opt<F: ?Sized, I, T>(typ: &T, f: &mut F) -> Option<T>
where
    F: TypeVisitor<I, T>,
    T: Deref<Target = Type<I, T>> + Clone,
    I: Clone,
{
    f.visit(typ)
}

pub fn walk_move_type_opt<F: ?Sized, I, T>(typ: &Type<I, T>, f: &mut F) -> Option<T>
where
    F: TypeVisitor<I, T>,
    T: Deref<Target = Type<I, T>> + Clone,
    I: Clone,
{
    match *typ {
        Type::Forall(ref args, ref typ) => f.visit(typ).map(|typ| f.forall(args.clone(), typ)),

        Type::Function(arg_type, ref arg, ref ret) => {
            let new_arg = f.visit(arg);
            let new_ret = f.visit(ret);
            merge(arg, new_arg, ret, new_ret, |arg, ret| {
                f.make(Type::Function(arg_type, arg, ret))
            })
        }
        Type::App(ref id, ref args) => {
            let new_args = walk_move_types(args, |t| f.visit(t));
            merge(id, f.visit(id), args, new_args, |x, y| f.app(x, y))
        }
        Type::Record(ref row) => f.visit(row).map(|row| f.make(Type::Record(row))),
        Type::Variant(ref row) => f.visit(row).map(|row| f.make(Type::Variant(row))),
        Type::Effect(ref row) => f.visit(row).map(|row| f.make(Type::Effect(row))),
        Type::ExtendRow {
            ref fields,
            ref rest,
        } => {
            let new_fields = walk_move_types(fields, |field| {
                f.visit(&field.typ)
                    .map(|typ| Field::new(field.name.clone(), typ))
            });
            let new_rest = f.visit(rest);
            merge(fields, new_fields, rest, new_rest, |fields, rest| {
                f.make(Type::ExtendRow { fields, rest })
            })
        }
        Type::ExtendTypeRow {
            ref types,
            ref rest,
        } => {
            let new_rest = f.visit(rest);
            new_rest.map(|rest| {
                f.make(Type::ExtendTypeRow {
                    types: types.clone(),
                    rest,
                })
            })
        }
        Type::Hole
        | Type::Opaque
        | Type::Error
        | Type::Builtin(_)
        | Type::Variable(_)
        | Type::Skolem(_)
        | Type::Generic(_)
        | Type::Ident(_)
        | Type::Projection(_)
        | Type::Alias(_)
        | Type::EmptyRow => None,
    }
}

struct WalkMoveTypes<'a, I, F, T> {
    types: I,
    clone_types_iter: I,
    f: F,
    clone_types: usize,
    next: Option<T>,
    _marker: PhantomData<&'a T>,
}

impl<'a, I, F, T> Iterator for WalkMoveTypes<'a, I, F, T>
where
    I: Iterator<Item = &'a T>,
    F: FnMut(&'a T) -> Option<T>,
    T: Clone + 'a,
{
    type Item = T;
    fn next(&mut self) -> Option<Self::Item> {
        if self.clone_types > 0 {
            self.clone_types -= 1;
            self.clone_types_iter.next().cloned()
        } else if let Some(typ) = self.next.take() {
            self.clone_types_iter.next();
            Some(typ)
        } else {
            let f = &mut self.f;
            if let Some((i, typ)) = self
                .types
                .by_ref()
                .enumerate()
                .find_map(|(i, typ)| f(typ).map(|typ| (i, typ)))
            {
                self.clone_types = i;
                self.next = Some(typ);
                self.next()
            } else {
                self.clone_types = usize::max_value();
                self.next()
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.clone_types_iter.size_hint()
    }
}

pub fn walk_move_types<'a, I, F, T, R>(types: I, f: F) -> Option<R>
where
    I: IntoIterator<Item = &'a T>,
    I::IntoIter: FusedIterator + Clone,
    F: FnMut(&'a T) -> Option<T>,
    T: Clone + 'a,
    R: std::iter::FromIterator<T>,
{
    walk_move_types_(types, f).map(|iter| iter.collect())
}

fn walk_move_types_<'a, I, F, T>(types: I, mut f: F) -> Option<WalkMoveTypes<'a, I::IntoIter, F, T>>
where
    I: IntoIterator<Item = &'a T>,
    I::IntoIter: FusedIterator + Clone,
    F: FnMut(&'a T) -> Option<T>,
    T: Clone + 'a,
{
    let mut types = types.into_iter();
    let clone_types_iter = types.clone();
    if let Some((i, typ)) = types
        .by_ref()
        .enumerate()
        .find_map(|(i, typ)| f(typ).map(|typ| (i, typ)))
    {
        Some(WalkMoveTypes {
            clone_types_iter,
            types,
            f,
            clone_types: i,
            next: Some(typ),
            _marker: PhantomData,
        })
    } else {
        None
    }
}

pub fn translate_alias<Id, T, U, F>(alias: &AliasData<Id, T>, mut translate: F) -> AliasData<Id, U>
where
    T: Deref<Target = Type<Id, T>>,
    U: Clone,
    Id: Clone,
    F: FnMut(&T) -> U,
{
    AliasData {
        name: alias.name.clone(),
        args: alias.args.clone(),
        typ: translate(&alias.typ),
        is_implicit: alias.is_implicit,
    }
}

pub fn translate_type<Id, T, U>(interner: &mut impl TypeContext<Id, U>, arc_type: &T) -> U
where
    T: Deref<Target = Type<Id, T>>,
    U: Clone,
    Id: Clone,
{
    translate_type_with(interner, arc_type, |interner, typ| {
        translate_type(interner, typ)
    })
}

pub fn translate_type_with<Id, T, U, I, F>(
    interner: &mut I,
    typ: &Type<Id, T>,
    mut translate: F,
) -> U
where
    T: Deref<Target = Type<Id, T>>,
    U: Clone,
    Id: Clone,
    F: FnMut(&mut I, &T) -> U,
    I: TypeContext<Id, U>,
{
    macro_rules! intern {
        ($e: expr) => {{
            let t = $e;
            interner.intern(t)
        }};
    }
    match *typ {
        Type::Function(arg_type, ref arg, ref ret) => {
            let t = Type::Function(arg_type, translate(interner, arg), translate(interner, ret));
            interner.intern(t)
        }
        Type::App(ref f, ref args) => {
            let t = Type::App(
                translate(interner, f),
                args.iter().map(|typ| translate(interner, typ)).collect(),
            );
            interner.intern(t)
        }
        Type::Record(ref row) => intern!(Type::Record(translate(interner, row))),
        Type::Variant(ref row) => intern!(Type::Variant(translate(interner, row))),
        Type::Effect(ref row) => intern!(Type::Effect(translate(interner, row))),
        Type::Forall(ref params, ref typ) => {
            let t = Type::Forall(params.clone(), translate(interner, typ));
            interner.intern(t)
        }
        Type::Skolem(ref skolem) => interner.intern(Type::Skolem(Skolem {
            name: skolem.name.clone(),
            id: skolem.id.clone(),
            kind: skolem.kind.clone(),
        })),
        Type::ExtendRow {
            ref fields,
            ref rest,
        } => {
            let fields = fields
                .iter()
                .map(|field| Field {
                    name: field.name.clone(),
                    typ: translate(interner, &field.typ),
                })
                .collect();

            let rest = translate(interner, rest);

            interner.extend_row(fields, rest)
        }
        Type::ExtendTypeRow {
            ref types,
            ref rest,
        } => {
            let types = types
                .iter()
                .map(|field| Field {
                    name: field.name.clone(),
                    typ: Alias {
                        _typ: translate(interner, &field.typ.as_type()),
                        _marker: PhantomData,
                    },
                })
                .collect();
            let rest = translate(interner, rest);

            interner.extend_type_row(types, rest)
        }
        Type::Hole => interner.hole(),
        Type::Opaque => interner.opaque(),
        Type::Error => interner.error(),
        Type::Builtin(ref builtin) => interner.builtin_type(builtin.clone()),
        Type::Variable(ref var) => interner.variable(var.clone()),
        Type::Generic(ref gen) => interner.generic(gen.clone()),
        Type::Ident(ref id) => interner.ident(id.clone()),
        Type::Projection(ref ids) => interner.projection(ids.clone()),
        Type::Alias(ref alias) => {
            let group: Vec<_> = alias
                .group
                .iter()
                .map(|alias_data| translate_alias(alias_data, |a| translate(interner, a)))
                .collect();

            interner.intern(Type::Alias(AliasRef {
                index: alias.index,
                group: Arc::from(group),
            }))
        }
        Type::EmptyRow => interner.empty_row(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walk_move_types_test() {
        assert_eq!(
            walk_move_types([1, 2, 3].iter(), |i| if *i == 2 { Some(4) } else { None }),
            Some(vec![1, 4, 3])
        );
    }
}
