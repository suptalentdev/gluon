//! Rust type to gluon type conversion

use base::types::{ArcType, Field, Type, TypeCache};
use base::symbol::{Symbol, Symbols};

use {Error as VmError, Result};
use api::VmType;
use thread::Thread;

use serde::de::{self, DeserializeOwned, DeserializeSeed, EnumAccess, Error, IntoDeserializer,
                MapAccess, SeqAccess, VariantAccess, Visitor};

/// Deserializes `T` from a gluon value assuming that `value` is of type `typ`.
pub fn from_rust<T>(thread: &Thread) -> Result<ArcType>
where
    T: DeserializeOwned,
{
    let mut symbols = Symbols::new();
    from_rust_::<T>(&mut symbols, thread)
}

fn from_rust_<T>(symbols: &mut Symbols, thread: &Thread) -> Result<ArcType>
where
    T: DeserializeOwned,
{
    let type_cache = TypeCache::new();
    let mut deserializer = Deserializer::from_value(&type_cache, thread, symbols);
    T::deserialize(&mut deserializer)?;
    let mut variants = Vec::new();
    while let Some(variant) = deserializer.variant.take() {
        variants.push(variant);
        deserializer.variant_index += 1;
        match T::deserialize(&mut deserializer) {
            Ok(_) => (),
            Err(VmError::Message(ref msg)) if msg == "" => break,
            Err(err) => return Err(err),
        }
    }
    if variants.is_empty() {
        Ok(deserializer.typ.expect("typ"))
    } else {
        Ok(type_cache.variant(variants))
    }
}

struct State<'de> {
    cache: &'de TypeCache<Symbol, ArcType<Symbol>>,
    thread: &'de Thread,
    symbols: &'de mut Symbols,
}

struct Deserializer<'de> {
    state: State<'de>,
    typ: Option<ArcType>,
    variant: Option<Field<Symbol, ArcType>>,
    variant_index: usize,
}

impl<'de> Deserializer<'de> {
    fn from_value(
        cache: &'de TypeCache<Symbol, ArcType<Symbol>>,
        thread: &'de Thread,
        symbols: &'de mut Symbols,
    ) -> Self {
        Deserializer {
            state: State {
                cache,
                thread,
                symbols,
            },
            typ: None,
            variant: None,
            variant_index: 0,
        }
    }
}

impl<'de, 't, 'a> de::Deserializer<'de> for &'a mut Deserializer<'de> {
    type Error = VmError;

    fn deserialize_any<V>(self, _visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        Err(VmError::Message("Cant deserialize any".to_string()))
    }

    fn deserialize_bool<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.typ = Some(bool::make_type(self.state.thread));
        visitor.visit_bool(false)
    }

    // The `parse_signed` function is generic over the integer type `T` so here
    // it is invoked with `T=i8`. The next 8 methods are similar.
    fn deserialize_i8<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.typ = Some(self.state.cache.byte());
        visitor.visit_i8(0)
    }

    fn deserialize_i16<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.typ = Some(self.state.cache.int());
        visitor.visit_i16(0)
    }

    fn deserialize_i32<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.typ = Some(self.state.cache.int());
        visitor.visit_i32(0)
    }

    fn deserialize_i64<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.typ = Some(self.state.cache.int());
        visitor.visit_i64(0)
    }

    fn deserialize_u8<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.typ = Some(self.state.cache.byte());
        visitor.visit_i8(0)
    }

    fn deserialize_u16<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.typ = Some(self.state.cache.int());
        visitor.visit_u16(0)
    }

    fn deserialize_u32<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.typ = Some(self.state.cache.int());
        visitor.visit_u32(0)
    }

    fn deserialize_u64<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.typ = Some(self.state.cache.int());
        visitor.visit_u64(0)
    }

    fn deserialize_f32<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.typ = Some(self.state.cache.float());
        visitor.visit_f32(0.)
    }

    fn deserialize_f64<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.typ = Some(self.state.cache.float());
        visitor.visit_f64(0.)
    }

    fn deserialize_char<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.typ = Some(self.state.cache.char());
        visitor.visit_char('\0')
    }

    fn deserialize_str<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.typ = Some(self.state.cache.string());
        visitor.visit_str("")
    }

    fn deserialize_string<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_str(visitor)
    }

    fn deserialize_bytes<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.typ = Some(Type::array(self.state.cache.byte()));
        visitor.visit_bytes(b"")
    }

    fn deserialize_byte_buf<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_bytes(visitor)
    }

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        let value = visitor.visit_some(&mut *self)?;
        let option_alias = self.state
            .thread
            .find_type_info("std.types.Option")
            .unwrap()
            .clone()
            .into_type();
        self.typ = Some(Type::app(
            option_alias,
            collect![self.typ.take().expect("typ")],
        ));
        Ok(value)
    }

    fn deserialize_unit<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.typ = Some(self.state.cache.unit());
        visitor.visit_unit()
    }

    fn deserialize_unit_struct<V>(self, _name: &'static str, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_unit(visitor)
    }

    fn deserialize_newtype_struct<V>(self, _name: &'static str, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_seq<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        let value = {
            let mut seq_deserializer = SeqDeserializer::new(&mut *self, 1);
            visitor.visit_seq(&mut seq_deserializer)?
        };
        self.typ = Some(Type::array(self.state.cache.byte()));
        Ok(value)
    }

    fn deserialize_tuple<V>(self, len: usize, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        let (value, types) = {
            let mut seq_deserializer = SeqDeserializer::new(&mut *self, len);
            (
                visitor.visit_seq(&mut seq_deserializer)?,
                seq_deserializer.types,
            )
        };
        self.typ = Some(self.state.cache.tuple(self.state.symbols, types));
        Ok(value)
    }

    fn deserialize_tuple_struct<V>(
        self,
        _name: &'static str,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_seq(visitor)
    }

    fn deserialize_map<V>(self, _visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        Err(VmError::Message(
            "Maps cannot be mapped to gluon types yet".to_string(),
        ))
    }

    fn deserialize_struct<V>(
        self,
        _name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        let (value, types) = {
            let mut map_deserializer = MapDeserializer::new(&mut *self, fields.iter().cloned());
            (
                visitor.visit_map(&mut map_deserializer)?,
                map_deserializer.types,
            )
        };
        self.typ = Some(self.state.cache.record(vec![], types));
        Ok(value)
    }

    fn deserialize_enum<V>(
        self,
        name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        match variants.get(self.variant_index) {
            Some(variant) => visitor.visit_enum(Enum::new(self, name, variant)),
            None => Err(VmError::Message("".to_string())),
        }
    }

    fn deserialize_identifier<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_str(visitor)
    }

    fn deserialize_ignored_any<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_any(visitor)
    }
}

struct SeqDeserializer<'de: 't, 't> {
    deserializer: &'t mut Deserializer<'de>,
    types: Vec<ArcType>,
    len: usize,
}

impl<'de, 't> SeqDeserializer<'de, 't> {
    fn new(deserializer: &'t mut Deserializer<'de>, len: usize) -> Self {
        SeqDeserializer {
            deserializer,
            len,
            types: Vec::new(),
        }
    }
}

impl<'de, 'a, 't> SeqAccess<'de> for &'a mut SeqDeserializer<'de, 't> {
    type Error = VmError;

    fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>>
    where
        T: DeserializeSeed<'de>,
    {
        if self.len == 0 {
            Ok(None)
        } else {
            self.len -= 1;
            let value = seed.deserialize(&mut *self.deserializer)?;
            self.types.push(self.deserializer.typ.take().expect("typ"));
            Ok(Some(value))
        }
    }
}

struct MapDeserializer<'de: 't, 't, I> {
    deserializer: &'t mut Deserializer<'de>,
    iter: I,
    types: Vec<Field<Symbol, ArcType>>,
}

impl<'de, 't, I> MapDeserializer<'de, 't, I> {
    fn new(deserializer: &'t mut Deserializer<'de>, iter: I) -> Self {
        MapDeserializer {
            deserializer,
            iter,
            types: Vec::new(),
        }
    }
}

impl<'de, 't, I> MapAccess<'de> for MapDeserializer<'de, 't, I>
where
    I: Iterator<Item = &'static str> + Clone,
{
    type Error = VmError;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>>
    where
        K: DeserializeSeed<'de>,
    {
        match self.iter.clone().next() {
            Some(field) => seed.deserialize(field.into_deserializer()).map(Some),
            None => Ok(None),
        }
    }

    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value>
    where
        V: DeserializeSeed<'de>,
    {
        match self.iter.next() {
            Some(field) => {
                let value = seed.deserialize(&mut *self.deserializer)?;
                self.types.push(Field::new(
                    self.deserializer.state.symbols.symbol(field),
                    self.deserializer.typ.take().expect("typ"),
                ));
                Ok(value)
            }
            None => Err(Self::Error::custom("Unable to deserialize value")),
        }
    }
}

struct Enum<'a, 'de: 'a> {
    de: &'a mut Deserializer<'de>,
    enum_name: &'static str,
    variant: &'static str,
}

impl<'a, 'de> Enum<'a, 'de> {
    fn new(de: &'a mut Deserializer<'de>, enum_name: &'static str, variant: &'static str) -> Self {
        Enum {
            de,
            enum_name,
            variant,
        }
    }
}

impl<'a, 'de> EnumAccess<'de> for Enum<'a, 'de> {
    type Error = VmError;
    type Variant = Self;

    fn variant_seed<V>(self, seed: V) -> Result<(V::Value, Self::Variant)>
    where
        V: DeserializeSeed<'de>,
    {
        seed.deserialize(self.variant.into_deserializer())
            .map(|value| (value, self))
    }
}

impl<'de, 'a> VariantAccess<'de> for Enum<'a, 'de> {
    type Error = VmError;

    fn unit_variant(self) -> Result<()> {
        let enum_type = Type::ident(self.de.state.symbols.symbol(self.enum_name));
        self.de.variant = Some(Field::new(
            self.de.state.symbols.symbol(self.variant),
            enum_type,
        ));
        Ok(())
    }

    fn newtype_variant_seed<T>(self, seed: T) -> Result<T::Value>
    where
        T: DeserializeSeed<'de>,
    {
        let value = seed.deserialize(&mut *self.de)?;
        let enum_type = Type::ident(self.de.state.symbols.symbol(self.enum_name));
        self.de.variant = Some(Field::new(
            self.de.state.symbols.symbol(self.variant),
            Type::function(collect![self.de.typ.take().expect("typ")], enum_type),
        ));
        Ok(value)
    }

    fn tuple_variant<V>(self, len: usize, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        let (value, types) = {
            let mut seq_deserializer = SeqDeserializer::new(&mut *self.de, len);
            (
                visitor.visit_seq(&mut seq_deserializer)?,
                seq_deserializer.types,
            )
        };
        let enum_type = Type::ident(self.de.state.symbols.symbol(self.enum_name));
        self.de.variant = Some(Field::new(
            self.de.state.symbols.symbol(self.variant),
            Type::function(types, enum_type),
        ));
        Ok(value)
    }

    fn struct_variant<V>(self, fields: &'static [&'static str], visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.tuple_variant(fields.len(), visitor)
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use super::from_rust_;
    use thread::RootedThread;

    #[allow(dead_code)]
    #[derive(Deserialize)]
    struct Test {
        x: i32,
        name: String,
    }

    #[test]
    fn struct_type() {
        let mut symbols = Symbols::new();
        let typ = from_rust_::<Test>(&mut symbols, &RootedThread::new());
        assert_eq!(
            typ,
            Ok(Type::record(
                vec![],
                vec![
                    Field::new(symbols.symbol("x"), Type::int()),
                    Field::new(symbols.symbol("name"), Type::string()),
                ]
            ))
        );
    }

    #[allow(dead_code)]
    #[derive(Deserialize)]
    enum Enum {
        A,
        B(i32),
        C(String, f64),
    }

    #[test]
    fn enum_type() {
        let mut symbols = Symbols::new();
        let typ = from_rust_::<Enum>(&mut symbols, &RootedThread::new());
        assert_eq!(
            typ,
            Ok(Type::variant(vec![
                Field::new(symbols.symbol("A"), Type::ident(symbols.symbol("Enum"))),
                Field::new(
                    symbols.symbol("B"),
                    Type::function(vec![Type::int()], Type::ident(symbols.symbol("Enum"))),
                ),
                Field::new(
                    symbols.symbol("C"),
                    Type::function(
                        vec![Type::string(), Type::float()],
                        Type::ident(symbols.symbol("Enum")),
                    ),
                ),
            ]))
        );
    }
}
