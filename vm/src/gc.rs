use std::fmt;
use std::mem;
use std::ptr;
use std::collections::hash_map::Entry;
use std::collections::VecDeque;
use std::cmp::Ordering;
use std::hash::{Hash, Hasher};
use std::ops::{Deref, DerefMut};
use std::cell::Cell;
use std::any::{Any, TypeId};
use std::marker::PhantomData;
use std::sync::Arc;

use base::fnv::FnvMap;
use interner::InternedStr;
use types::VmIndex;
use {Error, Result};

#[inline]
unsafe fn allocate(size: usize) -> *mut u8 {
    // Allocate an extra element if it does not fit exactly
    let cap = size / mem::size_of::<f64>() + (if size % mem::size_of::<f64>() != 0 {
        1
    } else {
        0
    });
    ptr_from_vec(Vec::<f64>::with_capacity(cap))
}

#[inline]
fn ptr_from_vec(mut buf: Vec<f64>) -> *mut u8 {
    let ptr = buf.as_mut_ptr();
    mem::forget(buf);

    ptr as *mut u8
}

#[inline]
unsafe fn deallocate(ptr: *mut u8, old_size: usize) {
    let cap = old_size / mem::size_of::<f64>() + (if old_size % mem::size_of::<f64>() != 0 {
        1
    } else {
        0
    });
    Vec::<f64>::from_raw_parts(ptr as *mut f64, 0, cap);
}

/// Pointer type which can only be written to.
pub struct WriteOnly<'s, T: ?Sized + 's>(*mut T, PhantomData<&'s mut T>);

impl<'s, T: ?Sized> WriteOnly<'s, T> {
    /// Unsafe as the lifetime must not be longer than the liftime of `t`
    unsafe fn new(t: *mut T) -> WriteOnly<'s, T> {
        WriteOnly(t, PhantomData)
    }

    /// Retrieves the pointer allowing it to be manipulated freely.
    /// As it points to uninitialized data care must be taken so to not read it before it has been
    /// initialized
    pub fn as_mut_ptr(&mut self) -> *mut T {
        self.0
    }
}

impl<'s, T> WriteOnly<'s, T> {
    /// Writes a value to the pointer and returns a pointer to the now initialized value.
    pub fn write(self, t: T) -> &'s mut T {
        unsafe {
            ptr::write(self.0, t);
            &mut *self.0
        }
    }
}

impl<'s, T: Copy> WriteOnly<'s, [T]> {
    pub fn write_slice(self, s: &[T]) -> &'s mut [T] {
        let self_ = unsafe { &mut *self.0 };
        assert!(s.len() == self_.len());
        for (to, from) in self_.iter_mut().zip(s) {
            *to = *from;
        }
        self_
    }
}

impl<'s> WriteOnly<'s, str> {
    pub fn write_str(self, s: &str) -> &'s mut str {
        unsafe {
            let ptr: &mut [u8] = mem::transmute::<*mut str, &mut [u8]>(self.0);
            assert!(s.len() == ptr.len());
            for (to, from) in ptr.iter_mut().zip(s.as_bytes()) {
                *to = *from;
            }
            &mut *self.0
        }
    }
}

#[derive(Clone, Copy, Default, Debug)]
#[cfg_attr(feature = "serde_derive", derive(Deserialize, Serialize))]
pub struct Generation(i32);

impl Generation {
    pub fn is_root(self) -> bool {
        self.0 == 0
    }

    /// Returns a generation which compared to any normal generation is always younger.
    pub fn disjoint() -> Generation {
        Generation(-1)
    }

    /// Returns wheter `self` is a parent of the other generation.
    pub fn is_parent_of(self, other: Generation) -> bool {
        self.0 < other.0
    }

    /// Returns true if `self` can contain a value from generation `other`
    pub fn can_contain_values_from(self, other: Generation) -> bool {
        other.0 <= self.0
    }

    pub fn next(self) -> Generation {
        assert!(
            self.0 < i32::max_value(),
            "To many generations has been created"
        );
        Generation(self.0 + 1)
    }
}

/// A mark and sweep garbage collector.
#[derive(Debug)]
#[cfg_attr(feature = "serde_derive", derive(DeserializeState, SerializeState))]
#[cfg_attr(feature = "serde_derive", serde(deserialize_state = "::serialization::DeSeed"))]
#[cfg_attr(feature = "serde_derive", serde(serialize_state = "::serialization::SeSeed"))]
pub struct Gc {
    /// Linked list of all objects allocted by this garbage collector.
    #[cfg_attr(feature = "serde_derive", serde(skip))]
    values: Option<AllocPtr>,
    /// How many bytes which is currently allocated
    allocated_memory: usize,
    /// How many bytes this garbage collector can allocate before a collection is run
    collect_limit: usize,
    /// The maximum number of bytes this garbage collector may contain
    memory_limit: usize,
    #[cfg_attr(feature = "serde_derive", serde(skip))] type_infos: FnvMap<TypeId, Box<TypeInfo>>,
    #[cfg_attr(feature = "serde_derive", serde(skip))]
    record_infos: FnvMap<Vec<InternedStr>, Box<TypeInfo>>,
    /// The generation of a gc determines what values it needs to copy and what values it can
    /// share. A gc can share values generated by itself (the same generation) and those in an
    /// earlier (lower) generation. It is important to note that two garbage collectors can have
    /// the same value as their generation but that does not mean that they can share values. This
    /// is only enforced in that values can only be shared up or down the tree of generations.
    ///
    /// Example:
    ///           0
    ///          / \
    ///         1   1
    ///        /   / \
    ///       2   2   2
    /// Generations 2 can share values with anything above them in the tree so refering to anything
    /// of generation 1 or 0 does not require a copy. Generation 1 can only share values with
    /// generation 0 so if a generation two value is shared up the tree it needs to be cloned.
    ///
    /// Between the generation 2 garbage collectors no values can be directly shared as they could
    /// only refer to each other through some reference or channel allocated in generation 0 (and
    /// if they do interact with eachother this means the values are cloned into generation 0).
    generation: Generation,
}


/// Trait which creates a typed pointer from a *mut () pointer.
/// For `Sized` types this is just a cast but for unsized types some more metadata must be taken
/// from the provided `D` value to make it initialize correctly.
pub unsafe trait FromPtr<D> {
    unsafe fn make_ptr(data: D, ptr: *mut ()) -> *mut Self;
}

unsafe impl<D, T> FromPtr<D> for T {
    unsafe fn make_ptr(_: D, ptr: *mut ()) -> *mut Self {
        ptr as *mut Self
    }
}

unsafe impl<'s, 't, T> FromPtr<&'s &'t [T]> for [T] {
    unsafe fn make_ptr(v: &'s &'t [T], ptr: *mut ()) -> *mut [T] {
        ::std::slice::from_raw_parts_mut(ptr as *mut T, v.len())
    }
}

/// A definition of some data which may be allocated by the garbage collector.
pub unsafe trait DataDef {
    /// The type of the value allocated.
    type Value: ?Sized + for<'a> FromPtr<&'a Self>;
    /// Returns how many bytes need to be allocted for this `DataDef`
    fn size(&self) -> usize;
    /// Consumes `self` to initialize the allocated value.
    /// Returns the initialized pointer.
    fn initialize<'w>(self, ptr: WriteOnly<'w, Self::Value>) -> &'w mut Self::Value;

    fn fields(&self) -> Option<&[InternedStr]> {
        None
    }
}

/// `DataDef` that moves its value directly into the pointer
/// useful for sized types
pub struct Move<T>(pub T);

unsafe impl<T> DataDef for Move<T> {
    type Value = T;
    fn size(&self) -> usize {
        mem::size_of::<T>()
    }
    fn initialize(self, result: WriteOnly<T>) -> &mut T {
        result.write(self.0)
    }
}

#[derive(Debug)]
struct TypeInfo {
    drop: unsafe fn(*mut ()),
    generation: Generation,
    fields: FnvMap<InternedStr, VmIndex>,
    fields_key: Arc<Vec<InternedStr>>,
}

#[derive(Debug)]
struct GcHeader {
    next: Option<AllocPtr>,
    marked: Cell<bool>,
    value_size: usize,
    type_info: *const TypeInfo,
}


struct AllocPtr {
    ptr: *mut GcHeader,
}

unsafe impl Send for AllocPtr {}

impl AllocPtr {
    fn new<T>(type_info: *const TypeInfo, value_size: usize) -> AllocPtr {
        debug_assert!(mem::align_of::<T>() <= mem::align_of::<f64>());
        unsafe {
            let alloc_size = GcHeader::value_offset() + value_size;
            let ptr = allocate(alloc_size) as *mut GcHeader;
            ptr::write(
                ptr,
                GcHeader {
                    next: None,
                    type_info: type_info,
                    value_size: value_size,
                    marked: Cell::new(false),
                },
            );
            AllocPtr { ptr: ptr }
        }
    }

    fn size(&self) -> usize {
        GcHeader::value_offset() + self.value_size
    }
}

impl fmt::Debug for AllocPtr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "AllocPtr {{ ptr: {:?} }}", &**self)
    }
}

impl Drop for AllocPtr {
    fn drop(&mut self) {
        unsafe {
            // Avoid stack overflow by looping through all next pointers instead of doing it
            // recursively
            let mut current = self.next.take();
            while let Some(mut next) = current {
                current = next.next.take();
            }
            let size = self.size();
            ((*self.type_info).drop)(self.value());
            ptr::read(&*self.ptr);
            deallocate(self.ptr as *mut u8, size);
        }
    }
}

impl Deref for AllocPtr {
    type Target = GcHeader;
    fn deref(&self) -> &GcHeader {
        unsafe { &*self.ptr }
    }
}

impl DerefMut for AllocPtr {
    fn deref_mut(&mut self) -> &mut GcHeader {
        unsafe { &mut *self.ptr }
    }
}

impl GcHeader {
    fn value(&mut self) -> *mut () {
        unsafe {
            let ptr: *mut GcHeader = self;
            (ptr as *mut u8).offset(GcHeader::value_offset() as isize) as *mut ()
        }
    }

    fn value_offset() -> usize {
        let hs = mem::size_of::<GcHeader>();
        let max_align = mem::align_of::<f64>();
        hs + ((max_align - (hs % max_align)) % max_align)
    }

    fn generation(&self) -> Generation {
        unsafe { (*self.type_info).generation }
    }
}

/// A pointer to a garbage collected value.
///
/// It is only safe to access data through a `GcPtr` if the value is rooted (stored in a place
/// where the garbage collector will find it during the mark phase).
pub struct GcPtr<T: ?Sized> {
    // TODO Use NonZero to allow for better optimizing
    ptr: *const T,
}

unsafe impl<T: ?Sized + Send + Sync> Send for GcPtr<T> {}
unsafe impl<T: ?Sized + Send + Sync> Sync for GcPtr<T> {}

impl<T: ?Sized> Copy for GcPtr<T> {}

impl<T: ?Sized> Clone for GcPtr<T> {
    fn clone(&self) -> GcPtr<T> {
        GcPtr { ptr: self.ptr }
    }
}

impl<T: ?Sized> Deref for GcPtr<T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.ptr }
    }
}

impl<T: ?Sized> ::std::borrow::Borrow<T> for GcPtr<T> {
    fn borrow(&self) -> &T {
        &**self
    }
}

impl<T: ?Sized + Eq> Eq for GcPtr<T> {}
impl<T: ?Sized + PartialEq> PartialEq for GcPtr<T> {
    fn eq(&self, other: &GcPtr<T>) -> bool {
        **self == **other
    }
}

impl<T: ?Sized + Ord> Ord for GcPtr<T> {
    fn cmp(&self, other: &GcPtr<T>) -> Ordering {
        (**self).cmp(&**other)
    }
}
impl<T: ?Sized + PartialOrd> PartialOrd for GcPtr<T> {
    fn partial_cmp(&self, other: &GcPtr<T>) -> Option<Ordering> {
        (**self).partial_cmp(&**other)
    }
}

impl<T: ?Sized + Hash> Hash for GcPtr<T> {
    fn hash<H>(&self, state: &mut H)
    where
        H: Hasher,
    {
        (**self).hash(state)
    }
}
impl<T: ?Sized + fmt::Debug> fmt::Debug for GcPtr<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "GcPtr({:?})", &**self)
    }
}
impl<T: ?Sized + fmt::Display> fmt::Display for GcPtr<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        (**self).fmt(f)
    }
}

impl<T: ?Sized> GcPtr<T> {
    /// Unsafe as it is up to the caller to ensure that this pointer is not referenced somewhere
    /// else
    pub unsafe fn as_mut(&mut self) -> &mut T {
        &mut *(self.ptr as *mut T)
    }

    /// Unsafe as `ptr` must have been allocted by this garbage collector
    pub unsafe fn from_raw(ptr: *const T) -> GcPtr<T> {
        GcPtr { ptr: ptr }
    }

    pub fn generation(&self) -> Generation {
        self.header().generation()
    }

    pub fn field_map(&self) -> &FnvMap<InternedStr, VmIndex> {
        unsafe { &(*self.header().type_info).fields }
    }

    pub fn fields(&self) -> &Arc<Vec<InternedStr>> {
        unsafe { &(*self.header().type_info).fields_key }
    }

    fn header(&self) -> &GcHeader {
        // Use of transmute_copy allows us to get the pointer
        // to the data regardless of wether T is unsized or not
        // (DST is structured as (ptr, len))
        // This function should always be safe to call as GcPtr's should always have a header
        // TODO: Better way of doing this?
        unsafe {
            let p: *mut u8 = mem::transmute_copy(&self.ptr);
            let header = p.offset(-(GcHeader::value_offset() as isize));
            &*(header as *const GcHeader)
        }
    }
}

impl<'a, T: Traverseable + Send + Sync + 'a> GcPtr<T> {
    /// Coerces `self` to a `Traverseable` trait object
    pub fn as_traverseable(self) -> GcPtr<Traverseable + Send + Sync + 'a> {
        GcPtr {
            ptr: self.ptr as *const (Traverseable + Send + Sync),
        }
    }
}
impl GcPtr<str> {
    /// Coerces `self` to a `Traverseable` trait object
    pub fn as_traverseable_string(self) -> GcPtr<Traverseable + Send + Sync> {
        // As there is nothing to traverse in a str we can safely cast it to *const u8 and use
        // u8's Traverseable impl
        GcPtr {
            ptr: self.as_ptr() as *const (Traverseable + Send + Sync),
        }
    }
}

/// Trait which must be implemented on all root types which contain `GcPtr`
/// A type implementing Traverseable must call traverse on each of its fields
/// which in turn contains `GcPtr`
pub trait Traverseable {
    fn traverse(&self, gc: &mut Gc) {
        let _ = gc;
    }
}

impl<T> Traverseable for Move<T>
where
    T: Traverseable,
{
    fn traverse(&self, gc: &mut Gc) {
        self.0.traverse(gc)
    }
}

impl<T: ?Sized> Traverseable for Box<T>
where
    T: Traverseable,
{
    fn traverse(&self, gc: &mut Gc) {
        (**self).traverse(gc)
    }
}

impl<'a, T: ?Sized> Traverseable for &'a T
where
    T: Traverseable,
{
    fn traverse(&self, gc: &mut Gc) {
        (**self).traverse(gc);
    }
}

impl<'a, T: ?Sized> Traverseable for &'a mut T
where
    T: Traverseable,
{
    fn traverse(&self, gc: &mut Gc) {
        (**self).traverse(gc);
    }
}

macro_rules! tuple_traverse {
    () => {};
    ($first: ident $($id: ident)*) => {
        tuple_traverse!($($id)*);
        impl <$first $(,$id)*> Traverseable for ($first, $($id,)*)
            where $first: Traverseable
                  $(, $id: Traverseable)* {
            #[allow(non_snake_case)]
            fn traverse(&self, gc: &mut Gc) {
                let (ref $first, $(ref $id,)*) = *self;
                $first.traverse(gc);
                $(
                    $id.traverse(gc);
                )*
            }
        }
    }
}

tuple_traverse!(A B C D E F G H I J);

macro_rules! empty_traverse {
    ($($id: ty)*) => {
        $(impl Traverseable for $id {
            fn traverse(&self, _: &mut Gc) {}
        })*
    }
}

empty_traverse! { () Any u8 u16 u32 u64 usize i8 i16 i32 i64 isize f32 f64 str }

impl<T: ?Sized> Traverseable for *const T {
    fn traverse(&self, _: &mut Gc) {}
}

impl<T: ?Sized> Traverseable for *mut T {
    fn traverse(&self, _: &mut Gc) {}
}

impl<T> Traverseable for Cell<T>
where
    T: Traverseable + Copy,
{
    fn traverse(&self, f: &mut Gc) {
        self.get().traverse(f);
    }
}

impl<U> Traverseable for [U]
where
    U: Traverseable,
{
    fn traverse(&self, f: &mut Gc) {
        for x in self.iter() {
            x.traverse(f);
        }
    }
}

impl<T> Traverseable for Vec<T>
where
    T: Traverseable,
{
    fn traverse(&self, gc: &mut Gc) {
        (**self).traverse(gc);
    }
}

impl<T> Traverseable for VecDeque<T>
where
    T: Traverseable,
{
    fn traverse(&self, gc: &mut Gc) {
        self.as_slices().traverse(gc);
    }
}

/// When traversing a `GcPtr` we need to mark it
impl<T: ?Sized> Traverseable for GcPtr<T>
where
    T: Traverseable,
{
    fn traverse(&self, gc: &mut Gc) {
        if !gc.mark(*self) {
            // Continue traversing if this ptr was not already marked
            (**self).traverse(gc);
        }
    }
}

impl Gc {
    /// Constructs a new garbage collector
    pub fn new(generation: Generation, memory_limit: usize) -> Gc {
        Gc {
            values: None,
            allocated_memory: 0,
            collect_limit: 100,
            memory_limit: memory_limit,
            type_infos: FnvMap::default(),
            record_infos: FnvMap::default(),
            generation: generation,
        }
    }

    pub fn set_memory_limit(&mut self, memory_limit: usize) {
        self.memory_limit = memory_limit;
    }

    pub fn generation(&self) -> Generation {
        self.generation
    }

    pub fn new_child_gc(&self) -> Gc {
        Gc::new(self.generation.next(), self.memory_limit)
    }

    /// Allocates a new object. If the garbage collector has hit the collection limit a collection
    /// will occur.
    ///
    /// Unsafe since `roots` must be able to traverse all accesible `GcPtr` values.
    pub unsafe fn alloc_and_collect<R, D>(&mut self, roots: R, def: D) -> Result<GcPtr<D::Value>>
    where
        R: Traverseable,
        D: DataDef + Traverseable,
        D::Value: Sized + Any,
    {
        self.check_collect((roots, &def));
        self.alloc(def)
    }

    /// Allocates a new object.
    pub fn alloc<D>(&mut self, def: D) -> Result<GcPtr<D::Value>>
    where
        D: DataDef,
        D::Value: Sized + Any,
    {
        let size = def.size();
        let needed = self.allocated_memory.saturating_add(size);
        if needed >= self.memory_limit {
            return Err(Error::OutOfMemory {
                limit: self.memory_limit,
                needed: needed,
            });
        }
        Ok(self.alloc_ignore_limit_(size, def))
    }

    pub fn alloc_ignore_limit<D>(&mut self, def: D) -> GcPtr<D::Value>
    where
        D: DataDef,
        D::Value: Sized + Any,
    {
        self.alloc_ignore_limit_(def.size(), def)
    }

    fn alloc_ignore_limit_<D>(&mut self, size: usize, def: D) -> GcPtr<D::Value>
    where
        D: DataDef,
        D::Value: Sized + Any,
    {
        unsafe fn drop<T>(t: *mut ()) {
            ptr::drop_in_place(t as *mut T);
        }
        let type_info: *const TypeInfo = match def.fields() {
            Some(fields) => match self.record_infos
                .get(fields)
                .map(|info| &**info as *const _)
            {
                Some(info) => info,
                None => &**self.record_infos.entry(fields.to_owned()).or_insert(
                    Box::new(TypeInfo {
                        drop: drop::<D::Value>,
                        generation: self.generation,
                        fields: fields
                            .iter()
                            .enumerate()
                            .map(|(i, s)| (*s, i as VmIndex))
                            .collect(),
                        fields_key: Arc::new(fields.to_owned()),
                    }),
                ),
            },
            None => match self.type_infos.entry(TypeId::of::<D::Value>()) {
                Entry::Occupied(entry) => &**entry.get(),
                Entry::Vacant(entry) => &**entry.insert(Box::new(TypeInfo {
                    drop: drop::<D::Value>,
                    generation: self.generation,
                    fields: FnvMap::default(),
                    fields_key: Arc::new(Vec::new()),
                })),
            },
        };
        let mut ptr = AllocPtr::new::<D::Value>(type_info, size);
        ptr.next = self.values.take();
        self.allocated_memory += ptr.size();
        unsafe {
            let p: *mut D::Value = D::Value::make_ptr(&def, ptr.value());
            let ret: *const D::Value = &*def.initialize(WriteOnly::new(p));
            // Check that the returned pointer is the same as the one we sent as an extra precaution
            // that the pointer was initialized
            assert!(ret == p);
            self.values = Some(ptr);
            GcPtr { ptr: p }
        }
    }

    pub unsafe fn check_collect<R>(&mut self, roots: R) -> bool
    where
        R: Traverseable,
    {
        if self.allocated_memory >= self.collect_limit {
            self.collect(roots);
            true
        } else {
            false
        }
    }

    /// Does a mark and sweep collection by walking from `roots`. This function is unsafe since
    /// roots need to cover all reachable object.
    pub unsafe fn collect<R>(&mut self, roots: R)
    where
        R: Traverseable,
    {
        info!("Start collect {:?}", self.generation);
        roots.traverse(self);
        self.sweep();
        self.collect_limit = 2 * self.allocated_memory;
    }

    /// Marks the GcPtr
    /// Returns true if the pointer was already marked
    pub fn mark<T: ?Sized>(&mut self, value: GcPtr<T>) -> bool {
        let header = value.header();
        // We only need to mark and traverse values from this garbage collectors generation
        if header.generation().is_parent_of(self.generation()) || header.marked.get() {
            true
        } else {
            header.marked.set(true);
            false
        }
    }

    /// Clears out any unmarked pointers and resets marked pointers.
    ///
    /// Unsafe as it is up to the caller to make sure that all reachable pointers have been marked
    unsafe fn sweep(&mut self) {
        fn moving<T>(t: T) -> T {
            t
        }

        let mut count = 0;
        let mut free_count = 0;

        let mut first = self.values.take();
        {
            // Pointer to the current pointer (if it exists)
            let mut maybe_header = &mut first;
            loop {
                let mut free = false;
                let mut replaced_next = None;
                match *maybe_header {
                    Some(ref mut header) => {
                        // If the current pointer is not marked we take the rest of the list and
                        // move it to `replaced_next`
                        if !header.marked.get() {
                            replaced_next = header.next.take();
                            free = true;
                        } else {
                            header.marked.set(false);
                        }
                    }
                    // Reached the end of the list
                    None => break,
                }
                count += 1;
                if free {
                    free_count += 1;
                    // Free the current pointer
                    self.free(maybe_header.take());
                    *maybe_header = replaced_next;
                } else {
                    // Just move to the next pointer
                    maybe_header = &mut moving(maybe_header).as_mut().unwrap().next;
                }
            }
        }
        info!("GC: Freed {} / Traversed {}", free_count, count);
        self.values = first;
    }

    fn free(&mut self, header: Option<AllocPtr>) {
        if let Some(ref ptr) = header {
            self.allocated_memory -= ptr.size();
        }
        debug!("FREE: {:?}", header);
        drop(header);
    }
}


#[cfg(test)]
mod tests {
    use super::{DataDef, Gc, GcHeader, GcPtr, Generation, Move, Traverseable, WriteOnly};
    use std::fmt;
    use std::mem;
    use std::rc::Rc;
    use std::cell::Cell;
    use std::usize;

    use self::Value::*;

    fn object_count(gc: &Gc) -> usize {
        let mut header: &GcHeader = match gc.values {
            Some(ref x) => &**x,
            None => return 0,
        };
        let mut count = 1;
        loop {
            match header.next {
                Some(ref ptr) => {
                    count += 1;
                    header = &**ptr;
                }
                None => break,
            }
        }
        count
    }


    #[derive(Copy, Clone)]
    struct Data_ {
        fields: GcPtr<Vec<Value>>,
    }

    impl PartialEq for Data_ {
        fn eq(&self, other: &Data_) -> bool {
            self.fields.ptr == other.fields.ptr
        }
    }
    impl fmt::Debug for Data_ {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            self.fields.ptr.fmt(f)
        }
    }

    struct Def<'a> {
        elems: &'a [Value],
    }
    unsafe impl<'a> DataDef for Def<'a> {
        type Value = Vec<Value>;
        fn size(&self) -> usize {
            mem::size_of::<Self::Value>()
        }
        fn initialize(self, result: WriteOnly<Vec<Value>>) -> &mut Vec<Value> {
            result.write(self.elems.to_owned())
        }
    }

    #[derive(Copy, Clone, PartialEq, Debug)]
    enum Value {
        Int(i32),
        Data(Data_),
    }

    impl Traverseable for Value {
        fn traverse(&self, gc: &mut Gc) {
            match *self {
                Data(ref data) => data.fields.traverse(gc),
                _ => (),
            }
        }
    }

    fn new_data(p: GcPtr<Vec<Value>>) -> Value {
        Data(Data_ { fields: p })
    }

    #[test]
    fn gc_header() {
        let mut gc: Gc = Gc::new(Generation::default(), usize::MAX);
        let ptr = gc.alloc(Def { elems: &[Int(1)] }).unwrap();
        let header: *const _ = ptr.header();
        let other: &mut GcHeader = gc.values.as_mut().unwrap();
        assert_eq!(&*ptr as *const _ as *const (), other.value());
        assert_eq!(header, other as *const _);
    }

    #[test]
    fn basic() {
        let mut gc: Gc = Gc::new(Generation::default(), usize::MAX);
        let mut stack: Vec<Value> = Vec::new();
        stack.push(new_data(gc.alloc(Def { elems: &[Int(1)] }).unwrap()));
        let d2 = new_data(gc.alloc(Def { elems: &[stack[0]] }).unwrap());
        stack.push(d2);
        assert_eq!(object_count(&gc), 2);
        unsafe {
            gc.collect(&mut *stack);
        }
        assert_eq!(object_count(&gc), 2);
        match stack[0] {
            Data(ref data) => assert_eq!(data.fields[0], Int(1)),
            _ => ice!(),
        }
        match stack[1] {
            Data(ref data) => assert_eq!(data.fields[0], stack[0]),
            _ => ice!(),
        }
        stack.pop();
        stack.pop();
        unsafe {
            gc.collect(&mut *stack);
        }
        assert_eq!(object_count(&gc), 0);
    }

    pub struct Dropable {
        dropped: Rc<Cell<bool>>,
    }

    impl Drop for Dropable {
        fn drop(&mut self) {
            self.dropped.set(true);
        }
    }

    #[test]
    fn drop() {
        let dropped = Rc::new(Cell::new(false));
        let mut gc = Gc::new(Generation::default(), usize::MAX);
        {
            let ptr = gc.alloc(Move(Dropable {
                dropped: dropped.clone(),
            })).unwrap();
            assert_eq!(false, ptr.dropped.get());
        }
        assert_eq!(false, dropped.get());
        unsafe {
            gc.collect(());
        }
        assert_eq!(true, dropped.get());
    }
}
