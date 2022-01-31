use super::{Field, Type, TypeExt};

use bitflags::bitflags;

bitflags! {
    #[derive(Deserialize, Serialize)]
    pub struct Flags: u8 {
        const HAS_VARIABLES = 1 << 0;
        const HAS_SKOLEMS = 1 << 1;
        const HAS_GENERICS = 1 << 2;
        const HAS_FORALL = 1 << 3;
        const HAS_IDENTS = 1 << 4;


        const NEEDS_GENERALIZE =
            Flags::HAS_VARIABLES.bits | Flags::HAS_SKOLEMS.bits;
    }
}

trait AddFlags {
    fn add_flags(&self, flags: &mut Flags);
}

impl<T> AddFlags for [T]
where
    T: AddFlags,
{
    fn add_flags(&self, flags: &mut Flags) {
        for t in self {
            t.add_flags(flags);
        }
    }
}

impl<Id, T> AddFlags for Field<Id, T>
where
    T: AddFlags,
{
    fn add_flags(&self, flags: &mut Flags) {
        self.typ.add_flags(flags);
    }
}

impl<T> AddFlags for T
where
    T: TypeExt,
{
    fn add_flags(&self, flags: &mut Flags) {
        *flags |= self.flags()
    }
}

impl<Id, T> AddFlags for Type<Id, T>
where
    T: AddFlags,
{
    fn add_flags(&self, flags: &mut Flags) {
        match self {
            Type::Function(_, arg, ret) => {
                arg.add_flags(flags);
                ret.add_flags(flags);
            }
            Type::App(ref f, ref args) => {
                f.add_flags(flags);
                args.add_flags(flags);
            }
            Type::Record(ref typ)
            | Type::Variant(ref typ)
            | Type::Effect(ref typ)
            | Type::Forall(_, ref typ) => {
                *flags |= Flags::HAS_FORALL;
                typ.add_flags(flags);
            }
            Type::Skolem(_) => *flags |= Flags::HAS_SKOLEMS,
            Type::ExtendRow { fields, rest, .. } => {
                fields.add_flags(flags);
                rest.add_flags(flags);
            }
            Type::Variable(_) => *flags |= Flags::HAS_VARIABLES,
            Type::Generic(_) => *flags |= Flags::HAS_GENERICS,
            Type::Ident(_) => *flags |= Flags::HAS_IDENTS,
            Type::Hole
            | Type::Opaque
            | Type::Error
            | Type::Builtin(..)
            | Type::Projection(_)
            | Type::Alias(_)
            | Type::EmptyRow => (),
        }
    }
}

impl Flags {
    pub fn from_type<Id, T>(typ: &Type<Id, T>) -> Self
    where
        T: TypeExt,
    {
        let mut flags = Flags::empty();
        typ.add_flags(&mut flags);
        flags
    }
}
