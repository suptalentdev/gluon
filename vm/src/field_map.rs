use std::borrow::Borrow;
use std::sync::Arc;

use base::fnv::FnvMap;

use interner::InternedStr;
use types::{VmIndex, VmTag};

#[derive(Clone, Debug, Hash, Eq, PartialEq)]
struct Fields(Arc<Vec<InternedStr>>);

impl Borrow<[InternedStr]> for Fields {
    fn borrow(&self) -> &[InternedStr] {
        &self.0
    }
}

#[derive(Debug)]
pub struct FieldMap {
    /// Maps fields into a tag
    tags: FnvMap<Fields, VmTag>,
    /// Maps the tag the record has and the field name onto the offset in the data
    fields: FnvMap<(VmTag, InternedStr), VmIndex>,
    field_list: FnvMap<VmTag, Fields>,
}

impl FieldMap {
    pub fn new() -> FieldMap {
        FieldMap {
            tags: FnvMap::default(),
            fields: FnvMap::default(),
            field_list: FnvMap::default(),
        }
    }

    pub fn get_offset(&self, tag: VmTag, name: InternedStr) -> Option<VmIndex> {
        self.fields.get(&(tag, name)).cloned()
    }

    pub fn get_fields(&self, tag: VmTag) -> Option<&Arc<Vec<InternedStr>>> {
        self.field_list.get(&tag).map(|x| &x.0)
    }

    pub fn get_map(&mut self, fields: &[InternedStr]) -> VmTag {
        if let Some(tag) = self.tags.get(fields) {
            return *tag | ::value::DataStruct::record_bit();
        }
        let tag = self.tags.len() as VmTag;
        let fields = Fields(Arc::new(fields.to_owned()));
        self.tags.insert(fields.clone(), tag);
        for (offset, field) in fields.0.iter().enumerate() {
            self.fields.insert((tag, *field), offset as VmIndex);
        }
        self.field_list.insert(tag, fields.clone());
        tag | ::value::DataStruct::record_bit()
    }
}
