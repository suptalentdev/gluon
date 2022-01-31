use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::mem;
use std::ops::{Deref, DerefMut};
use std::slice;

use serde::ser::{SerializeSeed, Serialize, Serializer};

use gc::{Gc, Traverseable};

enum Void {}

/// Abstract array type which have their length store inline with the elements.
/// Fills the same role as Box<[T]> but takes only 8 bytes on the stack instead of 16
pub struct Array<T: Copy> {
    len: usize,
    array_start: [T; 0],
    /// Prevents the array from being instantiated directly
    _void: Void,
}

impl<T> SerializeSeed for Array<T>
    where T: Copy + SerializeSeed
{
    type Seed = T::Seed;

    fn serialize_seed<S>(&self, serializer: S, seed: &Self::Seed) -> Result<S::Ok, S::Error>
        where S: Serializer
    {
        (**self).serialize_seed(serializer, seed)
    }
}

impl<T: Copy> AsRef<[T]> for Array<T> {
    fn as_ref(&self) -> &[T] {
        self
    }
}

impl<T: Copy> AsMut<[T]> for Array<T> {
    fn as_mut(&mut self) -> &mut [T] {
        self
    }
}

impl<T: Copy + PartialEq> PartialEq for Array<T> {
    fn eq(&self, other: &Array<T>) -> bool {
        &**self == other.as_ref()
    }
}

impl<T: Copy + Eq> Eq for Array<T> {}

impl<T: Copy + PartialOrd> PartialOrd for Array<T> {
    fn partial_cmp(&self, other: &Array<T>) -> Option<Ordering> {
        (&**self).partial_cmp(other.as_ref())
    }
}

impl<T: Copy + Ord> Ord for Array<T> {
    fn cmp(&self, other: &Array<T>) -> Ordering {
        (&**self).cmp(other)
    }
}

impl<T: Copy + Hash> Hash for Array<T> {
    fn hash<H>(&self, hasher: &mut H)
    where
        H: Hasher,
    {
        (&**self).hash(hasher)
    }
}

impl<T: Copy + fmt::Debug> fmt::Debug for Array<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", &**self)
    }
}

impl<T: Copy + Traverseable> Traverseable for Array<T> {
    fn traverse(&self, gc: &mut Gc) {
        (**self).traverse(gc)
    }
}

impl<T: Copy> Array<T> {
    pub fn size_of(len: usize) -> usize {
        mem::size_of::<usize>() + mem::size_of::<T>() * len
    }

    pub unsafe fn set_len(&mut self, len: usize) {
        self.len = len;
    }

    /// Initializes an Array.
    /// To be safe it is required that the iterators length is known and exactly the same as the
    /// length of the allocated array.
    pub unsafe fn initialize<I>(&mut self, iterable: I)
    where
        I: IntoIterator<Item = T>,
    {
        let iter = iterable.into_iter();
        self.len = iter.size_hint()
            .1
            .expect("initialize expected a known length");
        for (field, value) in self.iter_mut().zip(iter) {
            *field = value;
        }
    }
}

impl<T: Copy> Deref for Array<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        unsafe { slice::from_raw_parts(self.array_start.as_ptr(), self.len) }
    }
}

impl<T: Copy> DerefMut for Array<T> {
    fn deref_mut(&mut self) -> &mut [T] {
        unsafe { slice::from_raw_parts_mut(self.array_start.as_mut_ptr(), self.len) }
    }
}

impl<'a, T: Copy + 'a> IntoIterator for &'a Array<T> {
    type Item = &'a T;
    type IntoIter = <&'a [T] as IntoIterator>::IntoIter;
    fn into_iter(self) -> Self::IntoIter {
        (&**self).into_iter()
    }
}

impl<'a, T: Copy + 'a> IntoIterator for &'a mut Array<T> {
    type Item = &'a mut T;
    type IntoIter = <&'a mut [T] as IntoIterator>::IntoIter;
    fn into_iter(self) -> Self::IntoIter {
        (&mut **self).into_iter()
    }
}
