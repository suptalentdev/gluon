use std::borrow::Borrow;
use std::cmp::Ordering;
use std::fmt;
use std::marker::PhantomData;
use std::ops::Deref;

use base::types::ArcType;

use api::{ArrayRef, Getable, Pushable, ValueRef, VmType};
use thread::{ActiveThread, RootedValue, Thread, ThreadInternal, VmRoot};
use types::{VmIndex, VmInt};
use value::{ArrayRepr, Value, ValueArray};
use vm;
use {Result, Variants};

#[cfg(feature = "serde")]
use std::result::Result as StdResult;
#[cfg(feature = "serde")]
use thread::RootedThread;

#[cfg(feature = "serde")]
use serde::de::{Deserialize, Deserializer};
#[cfg(feature = "serde")]
use serde::ser::{Serialize, SerializeState, Serializer};

mod private {
    use super::*;

    pub trait Sealed {}

    impl<'value> Sealed for Variants<'value> {}
    impl<'s, 'value> Sealed for &'s Variants<'value> {}
    impl<T> Sealed for RootedValue<T> where T: Deref<Target = Thread> {}
}

pub trait AsValueRef: private::Sealed {
    fn as_value_ref(&self) -> ValueRef;
}

impl<'value> AsValueRef for Variants<'value> {
    fn as_value_ref(&self) -> ValueRef {
        self.as_ref()
    }
}
impl<T> AsValueRef for RootedValue<T>
where
    T: Deref<Target = Thread>,
{
    fn as_value_ref(&self) -> ValueRef {
        self.get_variant().as_ref()
    }
}

/// Abstraction over `Variants` which allows functions to be polymorphic over
/// `fn foo(&'s self) -> Value<'value>` where `'value` can either be the same as `'s` (when the
/// root does not have a lifetime and needs to *produce* a `Variants` value bound to `&self` or
/// `'value` can be disjoint from `'s` as is the case if a `Variants` struct is stored directly in
/// self (and as such as its own lifetime already)
pub trait AsVariant<'s, 'value>: private::Sealed {
    fn get_variant(&'s self) -> Variants<'value>;
    fn get_value(self) -> Value;
}

impl<'v, 'value> AsVariant<'v, 'value> for Variants<'value> {
    fn get_variant(&'v self) -> Self {
        *self
    }
    fn get_value(self) -> Value {
        Value::from(self.0)
    }
}

impl<'v, 'value> AsVariant<'v, 'value> for &'v Variants<'value> {
    fn get_variant(&'v self) -> Variants<'value> {
        **self
    }
    fn get_value(self) -> Value {
        Value::from(self.0)
    }
}
impl<'value, T> AsVariant<'value, 'value> for RootedValue<T>
where
    T: Deref<Target = Thread>,
{
    fn get_variant(&'value self) -> Variants<'value> {
        self.get_variant()
    }
    fn get_value(self) -> Value {
        RootedValue::get_value(&self)
    }
}

/// Type implementing both `Pushable` and `Getable` of values of `V` regardless of wheter `V`
/// implements the traits.
/// The actual value, `V` is only accessible directly either by `Deref` if it is `Userdata` or a
/// string or by the `to_value` method if it implements `Getable`.
///
/// When the value is not accessible the value can only be transferred back into gluon again
/// without inspecting the value itself two different threads.
pub struct Opaque<T, V>(T, PhantomData<V>)
where
    V: ?Sized;

pub type OpaqueRef<'a, V> = Opaque<Variants<'a>, V>;

pub type OpaqueValue<T, V> = Opaque<RootedValue<T>, V>;

impl<T, V> PartialEq for Opaque<T, V>
where
    T: AsValueRef,
    Self: Borrow<V>,
    V: ?Sized + PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        self.borrow() == other.borrow()
    }
}
impl<T, V> Eq for Opaque<T, V>
where
    T: AsValueRef,
    Self: Borrow<V>,
    V: ?Sized + Eq,
{
}

impl<T, V> PartialOrd for Opaque<T, V>
where
    T: AsValueRef,
    Self: Borrow<V>,
    V: ?Sized + PartialOrd,
{
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.borrow().partial_cmp(&other.borrow())
    }
}

impl<T, V> Ord for Opaque<T, V>
where
    T: AsValueRef,
    Self: Borrow<V>,
    V: ?Sized + Ord,
{
    fn cmp(&self, other: &Self) -> Ordering {
        self.borrow().cmp(&other.borrow())
    }
}

impl<T, V> Deref for Opaque<T, V>
where
    T: AsValueRef,
    V: vm::Userdata,
{
    type Target = V;

    fn deref(&self) -> &V {
        match self.0.as_value_ref() {
            ValueRef::Userdata(data) => data.downcast_ref::<V>().unwrap(),
            _ => ice!("ValueRef is not an Userdata"),
        }
    }
}

impl<T, V> Deref for Opaque<T, [V]>
where
    T: AsValueRef,
    V: ArrayRepr + Copy,
{
    type Target = [V];

    fn deref(&self) -> &[V] {
        match self.0.as_value_ref() {
            ValueRef::Array(data) => data.as_slice().expect("array is not of the correct type"),
            _ => ice!("ValueRef is not an array"),
        }
    }
}

impl<T> Deref for Opaque<T, str>
where
    T: AsValueRef,
{
    type Target = str;

    fn deref(&self) -> &str {
        match self.0.as_value_ref() {
            ValueRef::String(data) => data,
            _ => ice!("ValueRef is not an Userdata"),
        }
    }
}

impl<T, V> AsRef<V> for Opaque<T, V>
where
    V: ?Sized,
    Self: Deref<Target = V>,
{
    fn as_ref(&self) -> &V {
        self
    }
}

impl<T, V> Borrow<V> for Opaque<T, V>
where
    V: ?Sized,
    Self: Deref<Target = V>,
{
    fn borrow(&self) -> &V {
        self
    }
}

#[cfg(feature = "serde")]
impl<'de, V> Deserialize<'de> for OpaqueValue<RootedThread, V>
where
    V: ?Sized,
{
    fn deserialize<D>(deserializer: D) -> StdResult<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = ::api::de::deserialize_raw_value(deserializer)?;
        Ok(Self::from_value(value))
    }
}

#[cfg(feature = "serde")]
impl<T> Serialize for Opaque<T, str>
where
    T: AsValueRef,
{
    fn serialize<S>(&self, serializer: S) -> StdResult<S::Ok, S::Error>
    where
        S: Serializer,
    {
        (**self).serialize(serializer)
    }
}

#[cfg(feature = "serde")]
impl<T> SerializeState<Thread> for Opaque<T, str>
where
    T: AsValueRef,
{
    fn serialize_state<S>(&self, serializer: S, _thread: &Thread) -> StdResult<S::Ok, S::Error>
    where
        S: Serializer,
    {
        (**self).serialize(serializer)
    }
}

impl<T, V> fmt::Debug for Opaque<T, V>
where
    T: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

impl<T, V> Clone for Opaque<T, V>
where
    T: Clone,
{
    fn clone(&self) -> Self {
        Opaque(self.0.clone(), self.1.clone())
    }
}

impl<'vm, V> OpaqueValue<&'vm Thread, V>
where
    V: ?Sized,
{
    pub fn vm_(&self) -> &'vm Thread {
        self.0.vm_()
    }
}

impl<T, V> OpaqueValue<T, V>
where
    T: Deref<Target = Thread>,
    V: ?Sized,
{
    pub fn vm(&self) -> &Thread {
        self.0.vm()
    }

    /// Converts the value into its Rust representation
    pub fn to_value<'vm>(&'vm self) -> V
    where
        V: Getable<'vm, 'vm>,
    {
        V::from_value(self.vm(), self.get_variant())
    }
}

impl<T, V> Opaque<T, V>
where
    V: ?Sized,
{
    pub fn from_value(value: T) -> Self {
        Opaque(value, PhantomData)
    }

    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<'s, 'value, T, V> Opaque<T, V>
where
    T: AsVariant<'s, 'value>,
    V: ?Sized,
{
    /// Unsafe as `Value` are not rooted
    pub unsafe fn get_value(&'s self) -> Value {
        self.0.get_variant().get_value()
    }

    pub fn get_variant(&'s self) -> Variants<'value> {
        self.0.get_variant()
    }

    pub fn get_ref(&'s self) -> ValueRef<'value> {
        self.0.get_variant().as_ref()
    }
}

impl<T, V> VmType for Opaque<T, V>
where
    V: ?Sized + VmType,
    V::Type: Sized,
{
    type Type = V::Type;
    fn make_type(vm: &Thread) -> ArcType {
        V::make_type(vm)
    }

    fn extra_args() -> VmIndex {
        V::extra_args()
    }
}

impl<'s, 'value, 'vm, T, V> Pushable<'vm> for Opaque<T, V>
where
    T: Pushable<'vm>,
    V: ?Sized + VmType,
    V::Type: Sized,
{
    fn push(self, context: &mut ActiveThread<'vm>) -> Result<()> {
        self.0.push(context)
    }
}

impl<'vm, 'value, V> Getable<'vm, 'value> for Opaque<Variants<'value>, V>
where
    V: ?Sized,
{
    fn from_value(_vm: &'vm Thread, value: Variants<'value>) -> Self {
        Opaque::from_value(value)
    }
}

impl<'vm, 'value, T, V> Getable<'vm, 'value> for OpaqueValue<T, V>
where
    V: ?Sized,
    T: VmRoot<'vm>,
{
    fn from_value(vm: &'vm Thread, value: Variants<'value>) -> Self {
        OpaqueValue::from_value(vm.root_value(value))
    }
}

impl<'s, 'value, T, V> Opaque<T, [V]>
where
    T: AsVariant<'s, 'value>,
{
    pub fn len(&'s self) -> usize {
        self.get_value_array().len()
    }

    fn get_array(&'s self) -> ArrayRef<'value> {
        match self.0.get_variant().as_ref() {
            ValueRef::Array(array) => array,
            _ => ice!("Expected an array"),
        }
    }

    pub(crate) fn get_value_array(&'s self) -> &'value ValueArray {
        self.get_array().0
    }

    pub fn get(&'s self, index: VmInt) -> Option<OpaqueRef<'value, V>> {
        self.get_array().get(index as usize).map(Opaque::from_value)
    }

    pub fn iter(&'s self) -> Iter<'s, 'value, T, V> {
        Iter {
            index: 0,
            array: self,
            _marker: PhantomData,
        }
    }
}

impl<T, V> OpaqueValue<T, [V]>
where
    T: Deref<Target = Thread>,
{
    pub fn get2<'value>(&'value self, index: VmInt) -> Option<V>
    where
        V: for<'vm> Getable<'vm, 'value>,
    {
        self.get_array()
            .get(index as usize)
            .map(|v| V::from_value(self.0.vm(), v))
    }
}

pub struct Iter<'a, 'value, T, V>
where
    T: 'a,
    V: 'a,
{
    index: usize,
    array: &'a Opaque<T, [V]>,
    _marker: PhantomData<&'value ()>,
}

impl<'s, 'value, T, V> Iterator for Iter<'s, 'value, T, V>
where
    T: AsVariant<'s, 'value>,
{
    type Item = OpaqueRef<'value, V>;

    fn next(&mut self) -> Option<Self::Item> {
        let i = self.index;
        if i < self.array.len() {
            self.index += 1;
            self.array.get(i as VmInt)
        } else {
            None
        }
    }
}

impl<'a, T, V> IntoIterator for &'a Opaque<T, [V]>
where
    T: AsVariant<'a, 'a>,
{
    type Item = OpaqueRef<'a, V>;
    type IntoIter = Iter<'a, 'a, T, V>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
