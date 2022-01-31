extern crate serde;

use std::fmt;
use std::ops::{Deref, DerefMut};

use {Error, Result};
use api::Pushable;
use thread::{Context, Thread, ThreadInternal};
use types::{VmTag, VmIndex};
use value::{Def, Value};
use self::serde::ser::{self, Serialize};

pub fn to_value<T>(thread: &Thread, value: &T) -> Result<Value>
    where T: Serialize
{
    let mut context = thread.context();
    let mut serializer = Serializer {
        thread: thread,
        context: &mut context,
    };
    value.serialize(&mut serializer)?;
    Ok(serializer.context.stack.pop())
}

impl ser::Error for Error {
    fn custom<T>(msg: T) -> Self
        where T: fmt::Display
    {
        Error::Message(format!("{}", msg))
    }
}

struct Serializer<'t> {
    thread: &'t Thread,
    context: &'t mut Context,
}

impl<'t> Serializer<'t> {
    fn to_value<T>(&mut self, value: T) -> Result<()>
        where T: Pushable<'t>
    {
        value.push(self.thread, self.context)
    }

    fn alloc(&mut self, tag: VmTag, values: VmIndex) -> Result<()> {
        let value = self.context
            .gc
            .alloc(Def {
                       tag: tag,
                       elems: &self.context.stack[self.context.stack.len() - values..],
                   })?;
        for _ in 0..values {
            self.context.stack.pop();
        }
        self.context.stack.push(Value::Data(value));
        Ok(())
    }
}

struct RecordSerializer<'s, 'vm: 's> {
    serializer: &'s mut Serializer<'vm>,
    variant_index: VmTag,
    values: VmIndex,
}

impl<'s, 'vm> Deref for RecordSerializer<'s, 'vm> {
    type Target = Serializer<'vm>;
    fn deref(&self) -> &Self::Target {
        self.serializer
    }
}

impl<'s, 'vm> DerefMut for RecordSerializer<'s, 'vm> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.serializer
    }
}

impl<'s, 'vm> RecordSerializer<'s, 'vm> {
    fn new(serializer: &'s mut Serializer<'vm>, variant_index: u32) -> Self {
        RecordSerializer {
            serializer: serializer,
            variant_index: variant_index,
            values: 0,
        }
    }
}

impl<'a, 'vm> ser::Serializer for &'a mut Serializer<'vm> {
    type Ok = ();

    // The error type when some error occurs during serialization.
    type Error = Error;

    // Associated types for keeping track of additional state while serializing
    // compound data structures like sequences and maps. In this case no
    // additional state is required beyond what is already stored in the
    // Serializer struct.
    type SerializeSeq = RecordSerializer<'a, 'vm>;
    type SerializeTuple = RecordSerializer<'a, 'vm>;
    type SerializeTupleStruct = RecordSerializer<'a, 'vm>;
    type SerializeTupleVariant = RecordSerializer<'a, 'vm>;
    type SerializeMap = RecordSerializer<'a, 'vm>;
    type SerializeStruct = RecordSerializer<'a, 'vm>;
    type SerializeStructVariant = RecordSerializer<'a, 'vm>;

    // Here we go with the simple methods. The following 12 methods receive one
    // of the primitive types of the data model and map it to JSON by appending
    // into the output string.
    fn serialize_bool(self, v: bool) -> Result<Self::Ok> {
        self.to_value(v)
    }

    // JSON does not distinguish between different sizes of integers, so all
    // signed integers will be serialized the same and all unsigned integers
    // will be serialized the same. Other formats, especially compact binary
    // formats, may need independent logic for the different sizes.
    fn serialize_i8(self, v: i8) -> Result<Self::Ok> {
        self.serialize_i64(v as i64)
    }

    fn serialize_i16(self, v: i16) -> Result<Self::Ok> {
        self.serialize_i64(v as i64)
    }

    fn serialize_i32(self, v: i32) -> Result<Self::Ok> {
        self.serialize_i64(v as i64)
    }

    // Not particularly efficient but this is example code anyway. A more
    // performant approach would be to use the `itoa` crate.
    fn serialize_i64(self, v: i64) -> Result<Self::Ok> {
        self.to_value(v as isize)
    }

    fn serialize_u8(self, v: u8) -> Result<Self::Ok> {
        self.serialize_u64(v as u64)
    }

    fn serialize_u16(self, v: u16) -> Result<Self::Ok> {
        self.serialize_u64(v as u64)
    }

    fn serialize_u32(self, v: u32) -> Result<Self::Ok> {
        self.serialize_u64(v as u64)
    }

    fn serialize_u64(self, v: u64) -> Result<Self::Ok> {
        self.to_value(v as isize)
    }

    fn serialize_f32(self, v: f32) -> Result<Self::Ok> {
        self.serialize_f64(v as f64)
    }

    fn serialize_f64(self, v: f64) -> Result<Self::Ok> {
        self.to_value(v)
    }

    fn serialize_char(self, v: char) -> Result<Self::Ok> {
        self.serialize_str(&v.to_string())
    }

    fn serialize_str(self, v: &str) -> Result<Self::Ok> {
        self.to_value(v)
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<Self::Ok> {
        self.to_value(v)
    }

    // An absent optional is represented as the JSON `null`.
    fn serialize_none(self) -> Result<Self::Ok> {
        self.serialize_unit()
    }

    // A present optional is represented as just the contained value. Note that
    // this is a lossy representation. For example the values `Some(())` and
    // `None` both serialize as just `null`. Unfortunately this is typically
    // what people expect when working with JSON. Other formats are encouraged
    // to behave more intelligently if possible.
    fn serialize_some<T>(self, value: &T) -> Result<Self::Ok>
        where T: ?Sized + Serialize
    {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<Self::Ok> {
        self.context.stack.push(Value::Tag(0));
        Ok(())
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<Self::Ok> {
        self.serialize_unit()
    }

    fn serialize_unit_variant(self,
                              _name: &'static str,
                              variant_index: u32,
                              _variant: &'static str)
                              -> Result<Self::Ok> {
        self.context.stack.push(Value::Tag(variant_index));
        Ok(())
    }

    // As is done here, serializers are encouraged to treat newtype structs as
    // insignificant wrappers around the data they contain.
    fn serialize_newtype_struct<T>(self, _name: &'static str, value: &T) -> Result<Self::Ok>
        where T: ?Sized + Serialize
    {
        value.serialize(self)
    }

    // Note that newtype variant (and all of the other variant serialization
    // methods) refer exclusively to the "externally tagged" enum
    // representation.
    //
    // Serialize this to JSON in externally tagged form as `{ NAME: VALUE }`.
    fn serialize_newtype_variant<T>(self,
                                    _name: &'static str,
                                    variant_index: u32,
                                    _variant: &'static str,
                                    value: &T)
                                    -> Result<Self::Ok>
        where T: ?Sized + Serialize
    {
        value.serialize(&mut *self)?;
        self.alloc(variant_index, 1)
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq> {
        Ok(RecordSerializer::new(self, 0))
    }

    fn serialize_tuple(self, len: usize) -> Result<Self::SerializeTuple> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_struct(self,
                              _name: &'static str,
                              len: usize)
                              -> Result<Self::SerializeTupleStruct> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_variant(self,
                               _name: &'static str,
                               variant_index: u32,
                               _variant: &'static str,
                               _len: usize)
                               -> Result<Self::SerializeTupleVariant> {
        Ok(RecordSerializer::new(self, variant_index))
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap> {
        Ok(RecordSerializer::new(self, 0))
    }

    fn serialize_struct(self, _name: &'static str, len: usize) -> Result<Self::SerializeStruct> {
        self.serialize_map(Some(len))
    }

    fn serialize_struct_variant(self,
                                _name: &'static str,
                                variant_index: u32,
                                _variant: &'static str,
                                _len: usize)
                                -> Result<Self::SerializeStructVariant> {
        Ok(RecordSerializer::new(self, variant_index))
    }
}

impl<'a, 'vm> ser::SerializeSeq for RecordSerializer<'a, 'vm> {
    type Ok = ();
    type Error = Error;

    fn serialize_element<T>(&mut self, value: &T) -> Result<()>
        where T: ?Sized + Serialize
    {
        value.serialize(&mut **self)
    }

    fn end(self) -> Result<Self::Ok> {
        self.serializer.alloc(self.variant_index, self.values)
    }
}

impl<'a, 'vm> ser::SerializeTuple for RecordSerializer<'a, 'vm> {
    type Ok = ();
    type Error = Error;

    fn serialize_element<T>(&mut self, value: &T) -> Result<()>
        where T: ?Sized + Serialize
    {
        value.serialize(&mut **self)
    }

    fn end(self) -> Result<Self::Ok> {
        self.serializer.alloc(self.variant_index, self.values)
    }
}

impl<'a, 'vm> ser::SerializeTupleStruct for RecordSerializer<'a, 'vm> {
    type Ok = ();
    type Error = Error;

    fn serialize_field<T>(&mut self, value: &T) -> Result<()>
        where T: ?Sized + Serialize
    {
        value.serialize(&mut **self)
    }

    fn end(self) -> Result<Self::Ok> {
        self.serializer.alloc(self.variant_index, self.values)
    }
}

impl<'a, 'vm> ser::SerializeTupleVariant for RecordSerializer<'a, 'vm> {
    type Ok = ();
    type Error = Error;

    fn serialize_field<T>(&mut self, value: &T) -> Result<()>
        where T: ?Sized + Serialize
    {
        value.serialize(&mut **self)
    }

    fn end(self) -> Result<Self::Ok> {
        self.serializer.alloc(self.variant_index, self.values)
    }
}

impl<'a, 'vm> ser::SerializeMap for RecordSerializer<'a, 'vm> {
    type Ok = ();
    type Error = Error;

    fn serialize_key<T>(&mut self, _key: &T) -> Result<()>
        where T: ?Sized + Serialize
    {
        Ok(())
    }

    fn serialize_value<T>(&mut self, value: &T) -> Result<()>
        where T: ?Sized + Serialize
    {
        value.serialize(&mut **self)
    }

    fn end(self) -> Result<Self::Ok> {
        self.serializer.alloc(self.variant_index, self.values)
    }
}

impl<'a, 'vm> ser::SerializeStruct for RecordSerializer<'a, 'vm> {
    type Ok = ();
    type Error = Error;

    fn serialize_field<T>(&mut self, _key: &'static str, value: &T) -> Result<()>
        where T: ?Sized + Serialize
    {
        value.serialize(&mut **self)
    }

    fn end(self) -> Result<Self::Ok> {
        self.serializer.alloc(self.variant_index, self.values)
    }
}

// Similar to `SerializeTupleVariant`, here the `end` method is responsible for
// closing both of the curly braces opened by `serialize_struct_variant`.
impl<'a, 'vm> ser::SerializeStructVariant for RecordSerializer<'a, 'vm> {
    type Ok = ();
    type Error = Error;

    fn serialize_field<T>(&mut self, _key: &'static str, value: &T) -> Result<()>
        where T: ?Sized + Serialize
    {
        value.serialize(&mut **self)?;
        self.values += 1;
        Ok(())
    }

    fn end(self) -> Result<Self::Ok> {
        self.serializer.alloc(self.variant_index, self.values)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use thread::{RootedThread, ThreadInternal};

    #[derive(Serialize)]
    struct Test {
        test: i32,
        string: String,
    }

    fn make_value<'vm, T>(thread: &'vm Thread, value: T) -> Value
        where T: Pushable<'vm>
    {
        let mut context = thread.context();
        value.push(thread, &mut context);
        context.stack.pop()
    }

    #[test]
    fn bool() {
        let thread = RootedThread::new();
        assert_eq!(to_value(&thread, &true).unwrap(), Value::Tag(1));
    }
}
