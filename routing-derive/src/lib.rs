#![feature(slice_patterns)]

#[macro_use]
extern crate error_chain;
extern crate proc_macro;
extern crate regex;
extern crate syn;
#[macro_use]
extern crate quote;

mod errors;

use std::collections::HashMap;

use proc_macro::TokenStream;
use syn::{Lit, MetaItem, NestedMetaItem, PathSegment, StrStyle, Ty, Variant, VariantData};

use errors::*;

#[proc_macro_derive(RoutingTable, attributes(get, post, delete))]
pub fn expand_derive(input: TokenStream) -> TokenStream {
  let s = input.to_string();

  // parse the tokens into an abstract syntax tree
  let ast = syn::parse_derive_input(&s).unwrap();

  // add new tokens to the syntax tree
  let gen = impl_routing_table(&ast).unwrap();

  gen.parse().unwrap()
}

fn impl_routing_table(ast: &syn::DeriveInput) -> Result<quote::Tokens> {
  let name = &ast.ident;
  if let syn::Body::Enum(ref variants) = ast.body {

    let mut variant_tuples = Vec::new();
    for variant in variants {
      let (method, path_regex) = get_method_and_path_from_variant(&variant)?;
      variant_tuples.push((variant, method, path_regex));
    }

    let mut method_map = HashMap::new();
    for &(ref variant, ref method, ref regex_path) in &variant_tuples {
      method_map.entry(method).or_insert_with(Vec::new).push((variant, regex_path));
    }

    let mut routing_table_fields = Vec::new();
    let mut routing_table_field_initializers = Vec::new();
    let mut method_match_cases = Vec::new();
    for (method, regexes_and_variants) in &method_map {

      // fields
      let mut regex_set_name_str = method.to_string().to_lowercase();
      regex_set_name_str.push_str("_regex_set");
      let regex_set_name = quote::Ident::new(regex_set_name_str);

      let mut regex_vec_name_str = method.to_string().to_lowercase();
      regex_vec_name_str.push_str("_regex_vector");
      let regex_vec_name = quote::Ident::new(regex_vec_name_str);

      routing_table_fields.push(quote!{
        #regex_set_name: regex::RegexSet,
        #regex_vec_name: Vec<regex::Regex>,
      });

      let mut regex_strings = Vec::new();
      let mut regex_constructors = Vec::new();
      let mut regex_match_cases = Vec::new();
      for (index, &(variant, regex_path)) in regexes_and_variants.iter().enumerate() {
        regex_strings.push(regex_path);
        regex_constructors.push(quote!{ regex::Regex::new(#regex_path).unwrap() });
        regex_match_cases.push(construct_route(index, variant));
      }

      routing_table_field_initializers.push(quote!{
        #regex_set_name: regex::RegexSet::new(&[
          #(#regex_strings,)*
        ]).unwrap(),
        #regex_vec_name: vec![
          #(#regex_constructors,)*
        ],
      });

      method_match_cases.push(quote! {
        &hyper::#method => {
          let matches = self.#regex_set_name.matches(request.path());
          if let Some(index) = matches.into_iter().next() {
            let _caps = self.#regex_vec_name[index].captures(request.path()).unwrap();
            match index {
              #(#regex_match_cases,)*
              _ => None
            }
          } else {
            None
          }
        }
      });
    }

    let mut routing_table_struct_name = String::new();
    routing_table_struct_name.push_str("RoutingTableFor");
    routing_table_struct_name.push_str(&name.to_string());
    let routing_table = quote::Ident::new(routing_table_struct_name);

    let impl_tokens = quote! {

      pub struct #routing_table {
        #(#routing_table_fields)*
      }

      impl #routing_table {
        fn new() -> #routing_table {
          #routing_table {
            #(#routing_table_field_initializers)*
          }
        }
      }

      impl RoutingTable<#name> for #routing_table {
        fn route(&self, request: &hyper::Request) -> Option<#name> {
          use #name::*;

          match request.method() {
            #(#method_match_cases,)*
            _ => None
          }
        }
      }

      impl NewRoutingTable<#name> for #name {
        type Table = #routing_table;
        fn routing_table() -> Self::Table {
          #routing_table::new()
        }
      }
    };

    Ok(impl_tokens)
  } else {
    panic!("#[derive(RoutingTable)] is only defined for enums, not for structs!");
  }
}

// given a variant of an enum, prepare a tuple of (enum_variant_identifier, hyper_method, path)
fn get_method_and_path_from_variant(variant: &Variant) -> Result<(quote::Ident, String)> {
  for ref attr in &variant.attrs {
    if let MetaItem::List(ref method, ref nested) = attr.value {
      if let &[NestedMetaItem::Literal(Lit::Str(ref http_path, StrStyle::Cooked))] = nested.as_slice() {
        let method_ident = quote::Ident::new(capitalize_first(&method.to_string()));
        let path_regex = construct_path_regex_string(variant, http_path);
        return Ok((method_ident, path_regex))
      }
    }
  }
  Err(ErrorKind::MissingAttribute(variant.ident.to_string()).into())
}

fn capitalize_first(string: &str) -> String {
  if string.len() == 0 {
    String::new()
  } else if string.len() == 1 {
    string.to_string().to_uppercase()
  } else {
    let mut characters = string.chars();

    let mut name = String::new();
    name.push_str(&characters.next().unwrap().to_string().to_uppercase());
    name.extend(characters);
    name
  }
}

fn construct_path_regex_string(variant: &Variant, path: &str) -> String {

  let mut regex = HashMap::new();
  if let VariantData::Struct(ref fields) = variant.data {
    for field in fields {
      let field_name = field.ident.clone().unwrap().to_string();
      let field_regex = construct_regex_for_field(&field_name, &field.ty);
      regex.insert(field_name, field_regex);
    }
  }

  let parsed = parse_path(path);
  interpolate(parsed, regex)
}

fn construct_regex_for_field(name: &str, syn_type: &syn::Ty) -> String {
  if let &Ty::Path(None, ref path) = syn_type {
    let inner_regex = match path.segments.as_slice() {
      &[PathSegment{ ref ident, .. }] => match ident.as_ref() {
        "usize" | "u8" | "u16" | "u32" | "u64" => r"\d+",
        _ => ""
      },
      _ => ""
    };

    let mut named_capture_group = String::new();
    named_capture_group.push_str("(?P<");
    named_capture_group.push_str(name);
    named_capture_group.push_str(">");
    named_capture_group.push_str(inner_regex);
    named_capture_group.push_str(")");
    named_capture_group
  } else {
    String::new()
  }
}

#[derive(Debug)]
enum HttpPathSegment {
  Var(String),
  Lit(String)
}

// this will return a list of all things between "/", all empty ones will get removed
fn parse_path(path: &str) -> Vec<HttpPathSegment> {
  path.split("/")
    .filter(|s| s.len() > 0)
    .map(|element| {
      if element.chars().take(1).any(|c| c == ':') {
        HttpPathSegment::Var(element.chars().skip(1).collect::<String>())
      } else {
        HttpPathSegment::Lit(element.to_owned())
      }
    })
    .collect::<Vec<_>>()
}

// this takes a list of path segments, and a context
// the context will be a map of { ident => regex_string }
fn interpolate(segments: Vec<HttpPathSegment>, context: HashMap<String, String>) -> String {
  let mut regex_string = String::new();
  regex_string.push_str(r"^");
  for segment in &segments {
    regex_string.push('/');
    match segment {
      &HttpPathSegment::Var(ref id) => {
        let regex = context.get(id).cloned().unwrap_or_else(String::new);
        regex_string.push_str(&regex)
      },
      &HttpPathSegment::Lit(ref el) => regex_string.push_str(&el)
    };
  }
  regex_string.push_str(r"/?$");
  regex_string
}

fn construct_route(index: usize, variant: &Variant) -> quote::Tokens {
  let ident = &variant.ident;
  let constructor = match variant.data {
    VariantData::Struct(ref fields) => {

      let mut field_initializers = Vec::new();
      for field in fields {
        let ident = &field.ident.clone().unwrap();
        let ident_str = ident.to_string();
        let ty = &field.ty;
        field_initializers.push(quote! {
          #ident: _caps[#ident_str].parse::<#ty>().unwrap()
        });
      }

      quote!{
        Some(#ident {
          #(#field_initializers,)*
        })
      }
    },
    VariantData::Tuple(_) => quote!{ None },
    VariantData::Unit => quote!{ Some(#ident) }
  };
  quote!{ #index => #constructor }
}
