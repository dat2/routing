#![feature(slice_patterns)]

extern crate proc_macro;
extern crate syn;
#[macro_use]
extern crate quote;

use proc_macro::TokenStream;
use syn::{Lit, MetaItem, NestedMetaItem, StrStyle, Variant};

#[proc_macro_derive(RoutingTable, attributes(get, post, delete))]
pub fn routing_table(input: TokenStream) -> TokenStream {
  let s = input.to_string();

  // parse the tokens into an abstract syntax tree
  let ast = syn::parse_derive_input(&s).unwrap();

  // add new tokens to the syntax tree
  let gen = impl_routing_table(&ast);

  gen.parse().unwrap()
}

fn impl_routing_table(ast: &syn::DeriveInput) -> quote::Tokens {
  let name = &ast.ident;
  if let syn::Body::Enum(ref variants) = ast.body {

    let mut processed_variants = Vec::new();
    for variant in variants {
      if let Some(tuple) = process_variant(&variant) {
        processed_variants.push(tuple);
      }
    }

    let mut match_cases = Vec::new();
    for &(ref variant_name, ref method, ref path) in &processed_variants {
      match_cases.push(quote! { (&hyper::#method, #path) => Some(#name::#variant_name) });
    }

    let impl_tokens = quote! {
      impl RoutingTable<#name> for #name {
        fn route(request: &hyper::Request) -> Option<Self> {
          match (request.method(), request.path()) {
            #(#match_cases,)*
            _ => None
          }
        }
      }
    };

    println!("{:?}", impl_tokens);

    impl_tokens
  } else {
    panic!("#[derive(RoutingTable)] is only defined for enums, not for structs!");
  }
}

// TODO validations
fn process_variant(variant: &Variant) -> Option<(quote::Ident, quote::Ident, String)> {
  for ref attr in &variant.attrs {
    if let MetaItem::List(ref http_ident, ref nested) = attr.value {
      if let &[NestedMetaItem::Literal(Lit::Str(ref http_path, StrStyle::Cooked))] = nested.as_slice() {
        let variant_ident = quote::Ident::new(variant.ident.to_string());
        let routing_http_ident = quote::Ident::new(capitalize_first(&http_ident.to_string()));
        return Some((variant_ident, routing_http_ident, http_path.to_string()))
      }
    }
  }
  None
}

fn capitalize_first(string: &str) -> String {
  let mut characters = string.chars();

  let mut name = String::new();
  name.push_str(&characters.next().unwrap().to_string().to_uppercase());
  name.extend(characters);
  name
}
