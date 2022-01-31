//! Collection types which allows insertion of new values while shared references to its contents
//! are alive. This is done by storing each value in a stable memory location and preventing an
//! earlier inserted value to be overwritten.
use std::cell::{RefCell, Ref};
use std::collections::HashMap;
use std::fmt;
use std::hash::Hash;
use std::iter::{FromIterator, IntoIterator};
use std::ops::{Index, IndexMut};

// NOTE: transmute is used to circumvent the borrow checker in this module
// This is safe since the containers hold boxed values meaning allocating larger
// storage does not invalidate the references that are handed out and because values
// can only be inserted, never modified (this could be done with a Rc pointer as well but
// is not done for efficiency since it is not needed)

unsafe fn forget_lifetime<'a, 'b, T: ?Sized>(x: &'a T) -> &'b T {
    ::std::mem::transmute(x)
}

unsafe fn forget_lifetime_mut<'a, 'b, T: ?Sized>(x: &'a mut T) -> &'b mut T {
    ::std::mem::transmute(x)
}

// A mapping between K and V where once a value has been inserted it cannot be changed
// Through this and the fact the all values are stored as pointers it is possible to safely
// insert new values without invalidating pointers retrieved from it
pub struct FixedMap<K, V> {
    map: RefCell<HashMap<K, Box<V>>>,
}

impl<K: Eq + Hash + fmt::Debug, V: fmt::Debug> fmt::Debug for FixedMap<K, V> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.map.borrow().fmt(f)
    }
}

impl<K: Eq + Hash, V> FixedMap<K, V> {
    pub fn new() -> FixedMap<K, V> {
        FixedMap { map: RefCell::new(HashMap::new()) }
    }

    pub fn clear(&mut self) {
        self.map.borrow_mut().clear();
    }

    pub fn try_insert(&self, key: K, value: V) -> Result<(), (K, V)> {
        if self.get(&key).is_some() {
            Err((key, value))
        } else {
            self.map.borrow_mut().insert(key, Box::new(value));
            Ok(())
        }
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.map.borrow().len()
    }

    pub fn get(&self, k: &K) -> Option<&V> {
        self.map
            .borrow()
            .get(k)
            .map(|x| unsafe { forget_lifetime(&**x) })
    }
}

#[derive(Debug)]
pub struct FixedVec<T> {
    vec: RefCell<Vec<Box<T>>>,
}

impl<T> FixedVec<T> {
    pub fn new() -> FixedVec<T> {
        FixedVec { vec: RefCell::new(Vec::new()) }
    }

    pub fn clear(&mut self) {
        self.vec.borrow_mut().clear();
    }

    pub fn push(&self, value: T) {
        self.vec.borrow_mut().push(Box::new(value))
    }

    #[allow(dead_code)]
    pub fn extend<I: Iterator<Item = T>>(&self, iter: I) {
        self.vec.borrow_mut().extend(iter.map(|v| Box::new(v)))
    }

    pub fn borrow(&self) -> Ref<Vec<Box<T>>> {
        self.vec.borrow()
    }

    pub fn find<F>(&self, mut test: F) -> Option<(usize, &T)>
        where F: FnMut(&T) -> bool
    {
        self.vec
            .borrow()
            .iter()
            .enumerate()
            .find(|&(_, boxed)| test(&**boxed))
            .map(|(i, boxed)| (i, unsafe { forget_lifetime(&**boxed) }))
    }

    pub fn len(&self) -> usize {
        self.vec.borrow().len()
    }
}

impl<T> Index<usize> for FixedVec<T> {
    type Output = T;
    fn index(&self, index: usize) -> &T {
        let vec = self.vec.borrow();
        let result = &*vec[index];
        unsafe { forget_lifetime(result) }
    }
}

impl<T> IndexMut<usize> for FixedVec<T> {
    fn index_mut(&mut self, index: usize) -> &mut T {
        let mut vec = self.vec.borrow_mut();
        let result = &mut *vec[index];
        unsafe { forget_lifetime_mut(result) }
    }
}

impl<A> FromIterator<A> for FixedVec<A> {
    fn from_iter<T: IntoIterator<Item = A>>(iterator: T) -> FixedVec<A> {
        let vec: Vec<_> = iterator.into_iter().map(|x| Box::new(x)).collect();
        FixedVec { vec: RefCell::new(vec) }
    }
}
