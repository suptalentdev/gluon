extern crate anymap;

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::ops::Deref;
use std::rc::Rc;
use std::sync::Arc;

use serde::de::{Deserialize, DeserializeSeed, DeserializeState, Deserializer, Error};
use serde::ser::{SerializeState, Serializer};

use kind::ArcKind;
use symbol::Symbol;
use types::{AliasData, AppVec, ArcType, Type};

pub struct SeSeed {
    node_to_id: NodeToId,
}

impl SeSeed {
    pub fn new(node_to_id: NodeToId) -> Self {
        SeSeed {
            node_to_id: node_to_id,
        }
    }
}

impl AsRef<NodeToId> for SeSeed {
    fn as_ref(&self) -> &NodeToId {
        &self.node_to_id
    }
}

pub struct Seed<Id, T> {
    nodes: ::serialization::NodeMap,
    _marker: PhantomData<(Id, T)>,
}

impl<Id, T> AsMut<Seed<Id, T>> for Seed<Id, T> {
    fn as_mut(&mut self) -> &mut Self {
        self
    }
}

impl<Id, T> AsMut<::serialization::NodeMap> for Seed<Id, T> {
    fn as_mut(&mut self) -> &mut ::serialization::NodeMap {
        &mut self.nodes
    }
}

impl<Id, T> Seed<Id, T> {
    pub fn new(nodes: ::serialization::NodeMap) -> Self {
        Seed {
            nodes: nodes,
            _marker: PhantomData,
        }
    }
}

impl<Id, T> Clone for Seed<Id, T> {
    fn clone(&self) -> Self {
        Seed {
            nodes: self.nodes.clone(),
            _marker: PhantomData,
        }
    }
}


pub fn deserialize_type_vec<'de, Id, T, D>(
    seed: &mut Seed<Id, T>,
    deserializer: D,
) -> Result<AppVec<T>, D::Error>
where
    D: ::serde::Deserializer<'de>,
    T: Clone + From<Type<Id, T>> + ::std::any::Any + DeserializeState<'de, Seed<Id, T>>,
    Id: DeserializeState<'de, Seed<Id, T>>
        + Clone
        + ::std::any::Any
        + DeserializeState<'de, Seed<Id, T>>,
{
    DeserializeSeed::deserialize(
        ::serde::de::SeqSeedEx::new(seed, |_| AppVec::default()),
        deserializer,
    )
}
pub fn deserialize_group<'de, Id, T, D>(
    seed: &mut Seed<Id, T>,
    deserializer: D,
) -> Result<Arc<Vec<AliasData<Id, T>>>, D::Error>
where
    D: ::serde::Deserializer<'de>,
    T: Clone + From<Type<Id, T>> + ::std::any::Any + DeserializeState<'de, Seed<Id, T>>,
    Id: DeserializeState<'de, Seed<Id, T>>
        + Clone
        + ::std::any::Any
        + DeserializeState<'de, Seed<Id, T>>,
{
    use serialization::SharedSeed;
    let seed = SharedSeed::new(seed);
    DeserializeSeed::deserialize(seed, deserializer)
}

impl<'a, T> Shared for &'a T {
    fn unique(&self) -> bool {
        false
    }

    fn as_ptr(&self) -> *const () {
        &**self as *const _ as *const ()
    }
}

impl Shared for ::kind::ArcKind {
    fn unique(&self) -> bool {
        ::kind::ArcKind::strong_count(self) == 1
    }

    fn as_ptr(&self) -> *const () {
        &**self as *const _ as *const ()
    }
}

impl<T> Shared for Arc<T> {
    fn unique(&self) -> bool {
        Arc::strong_count(self) == 1
    }

    fn as_ptr(&self) -> *const () {
        &**self as *const T as *const ()
    }
}

impl<T> Shared for ArcType<T> {
    fn unique(&self) -> bool {
        ArcType::strong_count(self) == 1
    }

    fn as_ptr(&self) -> *const () {
        &**self as *const Type<_, _> as *const ()
    }
}

impl Shared for Symbol {
    fn unique(&self) -> bool {
        Symbol::strong_count(self) == 1
    }

    fn as_ptr(&self) -> *const () {
        &**self as *const _ as *const ()
    }
}


#[derive(Clone)]
pub struct IdSeed<Id>(NodeMap, PhantomData<Id>);

impl<Id> IdSeed<Id> {
    pub fn new(map: NodeMap) -> Self {
        IdSeed(map, PhantomData)
    }
}

impl<'de, Id> DeserializeSeed<'de> for IdSeed<Id>
where
    Id: Deserialize<'de>,
{
    type Value = Id;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        Id::deserialize(deserializer)
    }
}

impl<Id> AsMut<NodeMap> for IdSeed<Id> {
    fn as_mut(&mut self) -> &mut NodeMap {
        &mut self.0
    }
}

#[derive(Clone)]
pub struct MapSeed<S, F> {
    seed: S,
    map: F,
}

impl<S, F> MapSeed<S, F> {
    pub fn new(seed: S, map: F) -> MapSeed<S, F> {
        MapSeed {
            seed: seed,
            map: map,
        }
    }
}

impl<S, T, F> AsMut<T> for MapSeed<S, F>
where
    S: AsMut<T>,
{
    fn as_mut(&mut self) -> &mut T {
        self.seed.as_mut()
    }
}


impl<'de, S, F, R> DeserializeSeed<'de> for MapSeed<S, F>
where
    S: DeserializeSeed<'de>,
    F: FnOnce(S::Value) -> R,
{
    type Value = R;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        self.seed.deserialize(deserializer).map(self.map)
    }
}

pub type Id = u32;
type IdToShared<T> = HashMap<Id, T>;

#[derive(Clone)]
pub struct NodeMap(Rc<RefCell<anymap::Map>>);

impl Default for NodeMap {
    fn default() -> Self {
        NodeMap(Rc::new(RefCell::new(anymap::Map::new())))
    }
}

impl AsMut<NodeMap> for NodeMap {
    fn as_mut(&mut self) -> &mut NodeMap {
        self
    }
}

impl NodeMap {
    pub fn insert<T>(&self, id: Id, value: T)
    where
        T: Any,
    {
        self.0
            .borrow_mut()
            .entry::<IdToShared<T>>()
            .or_insert(IdToShared::new())
            .insert(id, value);
    }

    pub fn get<T>(&self, id: &Id) -> Option<T>
    where
        T: Any + Clone,
    {
        self.0
            .borrow()
            .get::<IdToShared<T>>()
            .and_then(|map| map.get(id).cloned())
    }
}

pub struct SharedSeed<'seed, T, S: 'seed>(pub &'seed mut S, PhantomData<T>);

impl<'seed, T, S> SharedSeed<'seed, T, S> {
    pub fn new(s: &'seed mut S) -> Self {
        SharedSeed(s, PhantomData)
    }
}


impl<'seed, T, S> AsMut<S> for SharedSeed<'seed, T, S> {
    fn as_mut(&mut self) -> &mut S {
        self.0
    }
}

#[derive(DeserializeState, SerializeState)]
#[cfg_attr(feature = "serde_derive", serde(deserialize_state = "S"))]
#[cfg_attr(feature = "serde_derive", serde(de_parameters = "S"))]
#[cfg_attr(feature = "serde_derive", serde(bound(deserialize = "T: DeserializeState<'de, S>")))]
#[cfg_attr(feature = "serde_derive", serde(bound(serialize = "T: SerializeState")))]
#[cfg_attr(feature = "serde_derive", serde(serialize_state = "T::Seed"))]
pub enum Variant<T> {
    Marked(
        Id,
        #[cfg_attr(feature = "serde_derive", serde(seed))]
        T,
    ),
    Plain(
        #[cfg_attr(feature = "serde_derive", serde(seed))]
        T,
    ),
    Reference(Id),
}

impl<'de, 'seed, T, S> DeserializeSeed<'de> for SharedSeed<'seed, T, S>
where
    T: DeserializeState<'de, S>,
    S: AsMut<NodeMap>,
    T: Any + Clone,
{
    type Value = T;

    fn deserialize<D>(mut self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        match Variant::<T>::deserialize_state(self.0, deserializer)? {
            Variant::Marked(id, node) => {
                self.0.as_mut().insert(id, node.clone());
                Ok(node)
            }
            Variant::Plain(value) => Ok(value),
            Variant::Reference(id) => {
                match self.0.as_mut().get(&id) {
                    Some(rc) => Ok(rc),
                    None => Err(D::Error::custom(format_args!("missing id {}", id))),
                }
            }
        }
    }
}

pub trait Shared {
    fn unique(&self) -> bool;
    fn as_ptr(&self) -> *const ();
}

pub type NodeToId = Rc<RefCell<HashMap<*const (), Id>>>;

enum Lookup {
    Unique,
    Found(Id),
    Inserted(Id),
}

fn node_to_id<T>(map: &NodeToId, node: &T) -> Lookup
where
    T: Shared,
{
    if Shared::unique(node) {
        return Lookup::Unique;
    }
    let mut map = map.borrow_mut();
    if let Some(id) = map.get(&node.as_ptr()) {
        return Lookup::Found(*id);
    }
    let id = map.len() as Id;
    map.insert(node.as_ptr(), id);
    Lookup::Inserted(id)
}


pub fn serialize_seq<'a, S, T, V>(
    self_: &'a T,
    serializer: S,
    seed: &V::Seed,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
    T: Deref<Target = [V]>,
    V: SerializeState,
{
    (**self_).serialize_state(serializer, seed)
}

impl<Id> SerializeState for ArcType<Id>
where
    Id: SerializeState<Seed = SeSeed>,
{
    type Seed = SeSeed;

    fn serialize_state<S>(&self, serializer: S, seed: &Self::Seed) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        ::serialization::shared::serialize(self, serializer, seed)
    }
}

impl SerializeState for ArcKind {
    type Seed = SeSeed;

    fn serialize_state<S>(&self, serializer: S, seed: &Self::Seed) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        ::serialization::shared::serialize(self, serializer, seed)
    }
}

pub mod shared {
    use super::*;
    use serde::de::DeserializeSeed;

    pub fn serialize<S, T>(
        self_: &T,
        serializer: S,
        seed: &<T::Target as SerializeState>::Seed,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
        T: Shared + Deref,
        T::Target: SerializeState,
        <T::Target as SerializeState>::Seed: AsRef<NodeToId>,
    {
        let node = match node_to_id(seed.as_ref(), self_) {
            Lookup::Unique => Variant::Plain(&**self_),
            Lookup::Found(id) => Variant::Reference(id),
            Lookup::Inserted(id) => Variant::Marked(id, &**self_),
        };
        node.serialize_state(serializer, seed)
    }

    pub fn deserialize<'de, D, S, T>(seed: &mut S, deserializer: D) -> Result<T, D::Error>
    where
        D: Deserializer<'de>,
        T: DeserializeState<'de, S>,
        S: AsMut<NodeMap>,
        T: Any + Clone,
    {
        SharedSeed::new(seed).deserialize(deserializer)
    }
}
