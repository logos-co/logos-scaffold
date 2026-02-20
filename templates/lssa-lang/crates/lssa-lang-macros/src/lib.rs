use proc_macro::TokenStream;
use proc_macro2::TokenTree;
use quote::quote;
use syn::{
    Data, DeriveInput, Expr, ExprLit, ExprPath, Fields, Item, ItemFn, ItemMod, Lit, Meta,
    MetaNameValue, Type,
    parse::Parser,
    parse_macro_input,
    punctuated::Punctuated,
};

#[proc_macro_derive(LssaInstruction)]
pub fn derive_lssa_instruction(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let ident = &input.ident;

    let Data::Enum(data_enum) = input.data else {
        return syn::Error::new_spanned(ident, "LssaInstruction can only be derived for enums")
            .to_compile_error()
            .into();
    };

    let mut variants_tokens = Vec::new();
    for variant in data_enum.variants {
        let variant_name = variant.ident.to_string();
        let args_tokens = match variant.fields {
            Fields::Named(fields) => fields
                .named
                .into_iter()
                .map(|field| {
                    let name = field
                        .ident
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "arg".to_string());
                    let ty = field.ty;
                    quote! {
                        lssa_lang::InstructionArgMetadata {
                            name: #name.to_string(),
                            ty: stringify!(#ty).to_string(),
                        }
                    }
                })
                .collect::<Vec<_>>(),
            Fields::Unnamed(fields) => fields
                .unnamed
                .into_iter()
                .enumerate()
                .map(|(index, field)| {
                    let name = format!("arg_{index}");
                    let ty = field.ty;
                    quote! {
                        lssa_lang::InstructionArgMetadata {
                            name: #name.to_string(),
                            ty: stringify!(#ty).to_string(),
                        }
                    }
                })
                .collect::<Vec<_>>(),
            Fields::Unit => Vec::new(),
        };

        variants_tokens.push(quote! {
            lssa_lang::InstructionVariantMetadata {
                name: #variant_name.to_string(),
                args: vec![#(#args_tokens),*],
            }
        });
    }

    quote! {
        impl lssa_lang::LssaInstruction for #ident {
            fn lssa_instruction_variants() -> Vec<lssa_lang::InstructionVariantMetadata> {
                vec![#(#variants_tokens),*]
            }
        }
    }
    .into()
}

#[proc_macro_derive(LssaAccounts, attributes(lssa))]
pub fn derive_lssa_accounts(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let ident = &input.ident;

    let Data::Struct(data_struct) = input.data else {
        return syn::Error::new_spanned(ident, "LssaAccounts can only be derived for structs")
            .to_compile_error()
            .into();
    };

    let mut fields_tokens = Vec::new();
    match data_struct.fields {
        Fields::Named(fields) => {
            for field in fields.named {
                match account_field_token(&field.ident.map(|v| v.to_string()), field.ty, &field.attrs)
                {
                    Ok(token) => fields_tokens.push(token),
                    Err(err) => return err.to_compile_error().into(),
                }
            }
        }
        Fields::Unnamed(fields) => {
            for (index, field) in fields.unnamed.into_iter().enumerate() {
                let fallback = format!("account_{index}");
                match account_field_token(&Some(fallback), field.ty, &field.attrs) {
                    Ok(token) => fields_tokens.push(token),
                    Err(err) => return err.to_compile_error().into(),
                }
            }
        }
        Fields::Unit => {}
    }

    quote! {
        impl lssa_lang::LssaAccounts for #ident {
            fn lssa_account_fields() -> Vec<lssa_lang::AccountFieldMetadata> {
                vec![#(#fields_tokens),*]
            }
        }
    }
    .into()
}

#[proc_macro_attribute]
pub fn lssa_program(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_program_args(parse_macro_input!(attr with Punctuated::<Meta, syn::Token![,]>::parse_terminated));
    let mut module = parse_macro_input!(item as ItemMod);

    let Ok((program_name, program_version)) = args else {
        return args
            .err()
            .expect("error should exist")
            .to_compile_error()
            .into();
    };

    let Some((_, items)) = module.content.as_mut() else {
        return syn::Error::new_spanned(
            module,
            "lssa_program requires an inline module (e.g. `mod name { ... }`)",
        )
        .to_compile_error()
        .into();
    };

    let mut registrations = Vec::new();

    for item in items.iter_mut() {
        let Item::Fn(function) = item else {
            continue;
        };

        match extract_registration(function) {
            Ok(Some(reg)) => registrations.push(reg),
            Ok(None) => {}
            Err(err) => return err.to_compile_error().into(),
        }
    }

    let registration_tokens = registrations
        .iter()
        .map(|registration| {
            let name = &registration.name;
            let instruction_ty = &registration.instruction_ty;
            let accounts_ty = &registration.accounts_ty;
            let execution = registration.execution.iter().map(|value| quote! { #value });

            quote! {
                lssa_lang::build_instruction::<#instruction_ty, #accounts_ty>(
                    lssa_lang::InstructionRegistration {
                        name: #name,
                        instruction_ty: stringify!(#instruction_ty),
                        accounts_ty: stringify!(#accounts_ty),
                        execution: &[#(#execution),*],
                    }
                )
            }
        })
        .collect::<Vec<_>>();

    let idl_fn = quote! {
        pub fn __lssa_idl() -> lssa_lang::idl::ProgramIdl {
            let instructions = vec![#(#registration_tokens),*];
            lssa_lang::build_program_idl("lssa-idl/0.1.0", #program_name, #program_version, instructions)
        }
    };

    let idl_json_fn = quote! {
        #[cfg(test)]
        pub fn __lssa_idl_json() -> String {
            serde_json::to_string_pretty(&__lssa_idl()).expect("IDL serialization should succeed")
        }
    };

    items.push(syn::parse2(idl_fn).expect("generated idl helper should parse"));
    items.push(syn::parse2(idl_json_fn).expect("generated idl json helper should parse"));

    quote! { #module }.into()
}

#[derive(Clone)]
struct InstructionRegistration {
    name: String,
    instruction_ty: Type,
    accounts_ty: Type,
    execution: Vec<String>,
}

fn parse_program_args(args: Punctuated<Meta, syn::Token![,]>) -> syn::Result<(String, String)> {
    let mut name: Option<String> = None;
    let mut version: Option<String> = None;

    for meta in args {
        let Meta::NameValue(MetaNameValue { path, value, .. }) = meta else {
            continue;
        };

        let Some(key) = path.get_ident().map(|id| id.to_string()) else {
            continue;
        };

        let Expr::Lit(ExprLit {
            lit: Lit::Str(value),
            ..
        }) = value
        else {
            return Err(syn::Error::new_spanned(
                value,
                "program attribute values must be string literals",
            ));
        };

        if key == "name" {
            name = Some(value.value());
        } else if key == "version" {
            version = Some(value.value());
        }
    }

    Ok((
        name.unwrap_or_else(|| "program".to_string()),
        version.unwrap_or_else(|| "0.1.0".to_string()),
    ))
}

fn extract_registration(function: &mut ItemFn) -> syn::Result<Option<InstructionRegistration>> {
    let mut registration_attr: Option<syn::Attribute> = None;
    function.attrs.retain(|attr| {
        let is_registration = attr.path().is_ident("lssa_instruction");
        if is_registration {
            registration_attr = Some(attr.clone());
            false
        } else {
            true
        }
    });

    let Some(attr) = registration_attr else {
        return Ok(None);
    };

    let parser = Punctuated::<Meta, syn::Token![,]>::parse_terminated;
    let metas = parser.parse2(attr.meta.require_list()?.tokens.clone())?;

    let mut name: Option<String> = None;
    let mut instruction_ty: Option<Type> = None;
    let mut accounts_ty: Option<Type> = None;
    let mut execution: Vec<String> = vec!["public".to_string()];

    for meta in metas {
        let Meta::NameValue(MetaNameValue { path, value, .. }) = meta else {
            continue;
        };
        let Some(key) = path.get_ident().map(|id| id.to_string()) else {
            continue;
        };

        match key.as_str() {
            "name" => {
                let Expr::Lit(ExprLit {
                    lit: Lit::Str(value),
                    ..
                }) = value
                else {
                    return Err(syn::Error::new_spanned(value, "name must be a string literal"));
                };
                name = Some(value.value());
            }
            "instruction" => {
                let Expr::Path(ExprPath { path, .. }) = value else {
                    return Err(syn::Error::new_spanned(value, "instruction must be a type path"));
                };
                instruction_ty = Some(Type::Path(syn::TypePath { qself: None, path }));
            }
            "accounts" => {
                let Expr::Path(ExprPath { path, .. }) = value else {
                    return Err(syn::Error::new_spanned(value, "accounts must be a type path"));
                };
                accounts_ty = Some(Type::Path(syn::TypePath { qself: None, path }));
            }
            "execution" => {
                let Expr::Lit(ExprLit {
                    lit: Lit::Str(value),
                    ..
                }) = value
                else {
                    return Err(syn::Error::new_spanned(
                        value,
                        "execution must be a string literal",
                    ));
                };
                execution = split_modes(&value.value());
            }
            _ => {}
        }
    }

    let instruction_ty = instruction_ty.ok_or_else(|| {
        syn::Error::new_spanned(
            &function.sig.ident,
            "missing `instruction = Type` in lssa_instruction attribute",
        )
    })?;

    let accounts_ty = accounts_ty.ok_or_else(|| {
        syn::Error::new_spanned(
            &function.sig.ident,
            "missing `accounts = Type` in lssa_instruction attribute",
        )
    })?;

    Ok(Some(InstructionRegistration {
        name: name.unwrap_or_else(|| function.sig.ident.to_string()),
        instruction_ty,
        accounts_ty,
        execution,
    }))
}

fn account_field_token(
    name: &Option<String>,
    ty: Type,
    attrs: &[syn::Attribute],
) -> syn::Result<proc_macro2::TokenStream> {
    let mut mutable = false;
    let mut auth = false;
    let mut claim_if_default = false;
    let mut visibility: Vec<String> = Vec::new();

    for attr in attrs.iter().filter(|attr| attr.path().is_ident("lssa")) {
        for segment in split_attr_segments(attr.meta.require_list()?.tokens.clone()) {
            let raw = segment.to_string().replace(' ', "");
            match raw.as_str() {
                "mut" | "mutable" => {
                    mutable = true;
                    continue;
                }
                "auth" => {
                    auth = true;
                    continue;
                }
                "claim_if_default" => {
                    claim_if_default = true;
                    continue;
                }
                _ => {}
            }

            let parsed: Meta = syn::parse2(segment.clone()).map_err(|_| {
                syn::Error::new_spanned(
                    &segment,
                    "unsupported lssa account annotation; expected mut, auth, claim_if_default, or visibility = \"...\"",
                )
            })?;

            if let Meta::NameValue(MetaNameValue { path, value, .. }) = parsed {
                if !path.is_ident("visibility") {
                    continue;
                }
                let Expr::Lit(ExprLit {
                    lit: Lit::Str(value),
                    ..
                }) = value
                else {
                    return Err(syn::Error::new_spanned(
                        value,
                        "visibility must be a string literal",
                    ));
                };
                visibility = split_modes(&value.value());
            }
        }
    }

    let field_name = name.clone().unwrap_or_else(|| "account".to_string());
    let visibility_tokens = visibility.iter().map(|item| quote! { #item.to_string() });

    Ok(quote! {
        lssa_lang::AccountFieldMetadata {
            name: #field_name.to_string(),
            ty: stringify!(#ty).to_string(),
            mutable: #mutable,
            auth: #auth,
            claim_if_default: #claim_if_default,
            visibility: vec![#(#visibility_tokens),*],
        }
    })
}

fn split_modes(raw: &str) -> Vec<String> {
    raw.split([',', '|'])
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn split_attr_segments(tokens: proc_macro2::TokenStream) -> Vec<proc_macro2::TokenStream> {
    let mut segments = Vec::new();
    let mut current = proc_macro2::TokenStream::new();

    for token in tokens {
        if let TokenTree::Punct(punct) = &token {
            if punct.as_char() == ',' {
                if !current.is_empty() {
                    segments.push(current);
                    current = proc_macro2::TokenStream::new();
                }
                continue;
            }
        }
        current.extend(std::iter::once(token));
    }

    if !current.is_empty() {
        segments.push(current);
    }

    segments
}
