use std::collections::BTreeMap;

use ast::Argument;
use symbol::{Symbol, SymbolRef};

pub trait MetadataEnv {
    fn get_metadata(&self, id: &SymbolRef) -> Option<&Metadata>;
}

impl<'a, T: ?Sized + MetadataEnv> MetadataEnv for &'a T {
    fn get_metadata(&self, id: &SymbolRef) -> Option<&Metadata> {
        (**self).get_metadata(id)
    }
}

impl MetadataEnv for () {
    fn get_metadata(&self, _id: &SymbolRef) -> Option<&Metadata> {
        None
    }
}

#[derive(Clone, Copy, Eq, PartialEq, Debug)]
#[cfg_attr(feature = "serde_derive", derive(Deserialize, Serialize))]
pub enum CommentType {
    Block,
    Line,
}

#[derive(Clone, Eq, PartialEq, Debug)]
#[cfg_attr(feature = "serde_derive", derive(Deserialize, Serialize))]
pub struct Comment {
    pub typ: CommentType,
    pub content: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
#[cfg_attr(feature = "serde_derive", derive(Deserialize, Serialize))]
pub struct Attribute {
    pub name: String,
    pub arguments: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
#[cfg_attr(feature = "serde_derive", derive(Deserialize, Serialize))]
pub struct Metadata {
    pub definition: Option<Symbol>,
    pub comment: Option<Comment>,
    pub attributes: Vec<Attribute>,
    pub args: Vec<Argument<Symbol>>,
    pub module: BTreeMap<String, Metadata>,
}

impl Metadata {
    pub fn has_data(&self) -> bool {
        self.definition.is_some()
            || self.comment.is_some()
            || !self.module.is_empty()
            || !self.attributes.is_empty()
    }

    pub fn merge(mut self, other: Metadata) -> Metadata {
        self.merge_with(other);
        self
    }

    pub fn merge_with(&mut self, other: Metadata) {
        if other.definition.is_some() {
            self.definition = other.definition;
        }
        if self.comment.is_none() {
            self.comment = other.comment;
        }
        for (key, value) in other.module {
            use std::collections::btree_map::Entry;
            match self.module.entry(key) {
                Entry::Vacant(entry) => {
                    entry.insert(value);
                }
                Entry::Occupied(entry) => entry.into_mut().merge_with(value),
            }
        }
        self.attributes.extend(other.attributes);
        if self.args.is_empty() {
            self.args = other.args;
        }
    }

    pub fn get_attribute(&self, name: &str) -> Option<&str> {
        self.attributes()
            .find(|attribute| attribute.name == name)
            .map(|t| t.arguments.as_ref().map_or("", |s| s))
    }

    pub fn attributes(&self) -> impl Iterator<Item = &Attribute> {
        self.attributes.iter()
    }
}
