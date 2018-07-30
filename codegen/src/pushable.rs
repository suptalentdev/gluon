use std::borrow::Cow;
use std::iter;

use proc_macro2::{Span, TokenStream};

use shared::{map_type_params, split_for_impl};

use syn::{
    self, Data, DataEnum, DataStruct, DeriveInput, Fields, FieldsNamed, FieldsUnnamed, Generics,
    Ident, Type,
};

use attr::{Container, CrateName};

pub fn derive(input: TokenStream) -> TokenStream {
    let derive_input = syn::parse2(input).expect("Input is checked by rustc");

    let container = Container::from_ast(&derive_input);

    let DeriveInput {
        ident,
        data,
        generics,
        ..
    } = derive_input;

    let tokens = match data {
        Data::Struct(ast) => derive_struct(&container, ast, ident, generics),
        Data::Enum(ast) => derive_enum(&container, ast, ident, generics),
        Data::Union(_) => panic!("Unions are not supported"),
    };

    tokens.into()
}

fn derive_struct(
    container: &Container,
    ast: DataStruct,
    ident: Ident,
    generics: Generics,
) -> TokenStream {
    let (field_idents, field_types) = get_info_from_fields(&ast.fields);
    let field_idents2 = &field_idents;

    // destructure the struct so the the fields can be accessed by the push implementation
    let destructured = match &ast.fields {
        Fields::Named(_) => quote! { let #ident { #(#field_idents2),* } = self; },
        Fields::Unnamed(_) => quote! { let #ident ( #(#field_idents2),* ) = self; },
        Fields::Unit => quote!{},
    };

    let push_impl = gen_push_impl(None, &field_idents, &field_types);

    gen_impl(
        &container,
        &ident,
        generics,
        quote! { #destructured #push_impl },
    )
}

fn derive_enum(
    container: &Container,
    ast: DataEnum,
    ident: Ident,
    generics: Generics,
) -> TokenStream {
    // generate a correct implementation for each variant, destructuring the enum
    // to get access to the values
    let match_arms = ast.variants.iter().enumerate().map(|(tag, variant)| {
        let (field_idents, field_types) = get_info_from_fields(&variant.fields);
        let field_idents2 = &field_idents;
        let variant_ident = &variant.ident;

        let pattern = match &variant.fields {
            Fields::Named(_) => quote! { #ident::#variant_ident{ #(#field_idents2),* } },
            Fields::Unnamed(_) => quote! { #ident::#variant_ident( #(#field_idents2),* ) },
            Fields::Unit => quote! { #ident::#variant_ident },
        };

        let push_impl = gen_push_impl(Some(tag), &field_idents, &field_types);

        quote! {
            #pattern => {
                #push_impl
            }
        }
    });

    // build the final match from the individual arms
    let push_impl = quote! {
        match self {
            #(#match_arms),*
        };
    };

    gen_impl(container, &ident, generics, push_impl)
}

fn gen_impl(
    container: &Container,
    ident: &Ident,
    generics: Generics,
    push_impl: TokenStream,
) -> TokenStream {
    let pushable_bounds = create_pushable_bounds(&generics);
    let (impl_generics, ty_generics, where_clause) = split_for_impl(&generics, &["'__vm"]);

    let dummy_const = Ident::new(&format!("_IMPL_PUSHABLE_FOR_{}", ident), Span::call_site());

    let gluon = match container.crate_name {
        CrateName::Some(ref ident) => quote!{
            use #ident::api as _gluon_api;
            use #ident::thread as _gluon_thread;
            use #ident::types as _gluon_types;
            use #ident::Result as _GluonResult;
        },
        CrateName::GluonVm => quote!{
            use api as _gluon_api;
            use thread as _gluon_thread;
            use types as _gluon_types;
            use Result as _GluonResult;
        },
        CrateName::None => quote!{
            use gluon::vm::api as _gluon_api;
            use gluon::vm::thread as _gluon_thread;
            use gluon::vm::types as _gluon_types;
            use gluon::vm::Result as _GluonResult;
        },
    };
    quote! {
        #[allow(non_upper_case_globals)]
        const #dummy_const: () = {
            #gluon

            #[automatically_derived]
            #[allow(unused_attributes, unused_variables)]
            impl #impl_generics _gluon_api::Pushable<'__vm> for #ident #ty_generics
            #where_clause #(#pushable_bounds),*
            {
                fn push(self, ctx: &mut _gluon_thread::ActiveThread<'__vm>) -> _GluonResult<()> {
                    #push_impl
                    Ok(())
                }
            }
        };
    }
}

fn gen_push_impl(
    tag: Option<usize>,
    field_idents: &[Cow<Ident>],
    field_types: &[&Type],
) -> TokenStream {
    debug_assert!(field_idents.len() == field_types.len());

    // push each field onto the stack
    // this has to be done in reverse order so the fields come out in the correct
    // order when popping the stack
    let stack_pushes = field_idents
        .iter()
        .zip(field_types)
        .map(|(ident, ty)| {
            quote! {
                <#ty as _gluon_api::Pushable<'__vm>>::push(#ident, ctx)?;
            }
        })
        .rev();

    // since the number of fields is statically known, we can allocate an array
    // by popping the stack for each field
    let array_init = iter::repeat(quote! { ctx.stack().pop() }).take(field_idents.len());

    let new_data = match tag {
        Some(tag) => quote!{
            ctx.context().new_data(vm, #tag as _gluon_types::VmTag, &fields)?
        },
        None => {
            let field_ident_strs = field_idents.iter().map(|i| i.to_string());
            quote! { {
                let field_names = [#(vm.global_env().intern(#field_ident_strs)?),*];
                ctx.context().new_record(vm, &fields, &field_names)?
            } }
        }
    };

    quote! {
        #(#stack_pushes)*
        let fields = [ #(#array_init),* ];
        let vm = ctx.thread();
        let val = #new_data;
        ctx.stack().push(val);
    }
}

fn create_pushable_bounds(generics: &Generics) -> Vec<TokenStream> {
    map_type_params(generics, |ty| {
        quote! {
            #ty: _gluon_api::Pushable<'__vm>
        }
    })
}

fn get_info_from_fields(fields: &Fields) -> (Vec<Cow<Ident>>, Vec<&Type>) {
    // get all the fields if there are any
    let fields = match fields {
        Fields::Named(FieldsNamed { named, .. }) => named,
        Fields::Unnamed(FieldsUnnamed { unnamed, .. }) => unnamed,
        Fields::Unit => return (Vec::new(), Vec::new()),
    };

    fields
        .iter()
        .enumerate()
        .map(|(idx, field)| {
            // if the fields belong to a struct we use the field name,
            // otherwise generate one from the index of the tuple element
            let ident = match &field.ident {
                Some(ident) => Cow::Borrowed(ident),
                None => Cow::Owned(Ident::new(&format!("_{}", idx), Span::call_site())),
            };

            (ident, &field.ty)
        })
        .unzip()
}
