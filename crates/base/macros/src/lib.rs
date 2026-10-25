//! Proc macros for kontra; the design is in docs/architecture-v3/08-MACROS.md.
//!
//! `vectors!` turns a glob of JSON test vectors into one `#[test]` per file.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::{Expr, Ident, LitStr, Token};

/// Expands `vectors!("dir/*.json", |v| run(v))` into one `#[test]` per matching file.
///
/// The glob is relative to `CARGO_MANIFEST_DIR`. `*` may appear only in the file name.
/// Each test calls the closure with the file's contents as `&'static str`; the closure
/// reads the tolerance from the file and must return `()`.
///
/// Test names are the file stem with non-identifier characters replaced by `_`. A leading
/// digit or a reserved word gets a `vector_` prefix. A glob that matches nothing, and two
/// files whose names collide, are compile errors.
///
/// Editing a vector rebuilds its test. Adding a file does not: touch the file that calls
/// `vectors!` so cargo rebuilds it.
#[proc_macro]
pub fn vectors(input: TokenStream) -> TokenStream {
    expand(input.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

struct Vectors {
    pattern: LitStr,
    run: Expr,
}

impl Parse for Vectors {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let pattern = input.parse()?;
        input.parse::<Token![,]>()?;
        let run = input.parse()?;
        input.parse::<Option<Token![,]>>()?;
        Ok(Self { pattern, run })
    }
}

fn expand(input: TokenStream2) -> syn::Result<TokenStream2> {
    let Vectors { pattern, run } = syn::parse2(input)?;
    let files = matching_files(&pattern)?;
    vector_tests(pattern.span(), &files, &run)
}

/// The per-file tests for `files`; `span` is where a name collision is reported.
fn vector_tests(span: Span, files: &[PathBuf], run: &Expr) -> syn::Result<TokenStream2> {
    let mut seen: HashMap<String, &Path> = HashMap::new();
    let mut tests = Vec::new();
    for file in files {
        let stem = file
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| syn::Error::new(span, format!("non-UTF-8 name: {}", file.display())))?;
        let name = test_name(stem);
        if let Some(first) = seen.insert(name.clone(), file) {
            return Err(syn::Error::new(
                span,
                format!(
                    "{} and {} both become test `{name}`; rename one",
                    first.display(),
                    file.display()
                ),
            ));
        }
        let ident = Ident::new(&name, span);
        let path = file
            .to_str()
            .ok_or_else(|| syn::Error::new(span, format!("non-UTF-8 path: {}", file.display())))?;
        tests.push(quote! {
            #[test]
            fn #ident() {
                (#run)(include_str!(#path));
            }
        });
    }
    Ok(quote! { #(#tests)* })
}

/// The files matching `pattern`, sorted, relative to `CARGO_MANIFEST_DIR`.
fn matching_files(pattern: &LitStr) -> syn::Result<Vec<PathBuf>> {
    let span = pattern.span();
    let glob = pattern.value();
    let manifest = std::env::var_os("CARGO_MANIFEST_DIR")
        .ok_or_else(|| syn::Error::new(span, "CARGO_MANIFEST_DIR is not set"))?;
    let (dir_part, name_part) = glob.rsplit_once('/').unwrap_or(("", &glob));
    if dir_part.contains('*') {
        return Err(syn::Error::new(
            span,
            "`*` is only supported in the file name",
        ));
    }
    let dir = Path::new(&manifest).join(dir_part);
    let entries = std::fs::read_dir(&dir)
        .map_err(|e| syn::Error::new(span, format!("cannot read {}: {e}", dir.display())))?;
    let mut files = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| syn::Error::new(span, e.to_string()))?;
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        let is_file = entry
            .file_type()
            .map_err(|e| syn::Error::new(span, e.to_string()))?
            .is_file();
        if is_file && wildcard_match(name_part, file_name) {
            files.push(entry.path());
        }
    }
    if files.is_empty() {
        return Err(syn::Error::new(span, format!("no file matches `{glob}`")));
    }
    files.sort();
    Ok(files)
}

/// Matches `text` against `pattern`, where each `*` matches any run of characters.
fn wildcard_match(pattern: &str, text: &str) -> bool {
    let mut pieces = pattern.split('*');
    let head = pieces.next().unwrap_or_default();
    let Some(mut rest) = text.strip_prefix(head) else {
        return false;
    };
    let tail: Vec<&str> = pieces.collect();
    let Some((last, middle)) = tail.split_last() else {
        return rest.is_empty();
    };
    for piece in middle {
        let Some((_, after)) = rest.split_once(piece) else {
            return false;
        };
        rest = after;
    }
    rest.ends_with(last)
}

/// A valid Rust identifier for a test named after `stem`.
fn test_name(stem: &str) -> String {
    let name: String = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let needs_prefix =
        name.starts_with(|c: char| c.is_ascii_digit()) || syn::parse_str::<Ident>(&name).is_err();
    if needs_prefix {
        format!("vector_{name}")
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::quote;

    #[test]
    fn wildcard_matches_file_names() {
        assert!(wildcard_match("*.json", "a.json"));
        assert!(wildcard_match("*", "anything"));
        assert!(wildcard_match("a*b*c", "a1b2c"));
        assert!(wildcard_match("exact.json", "exact.json"));
        assert!(!wildcard_match("*.json", "a.json.bak"));
        assert!(!wildcard_match("a*b", "a"));
        assert!(!wildcard_match("exact.json", "other.json"));
    }

    #[test]
    fn test_names_are_identifiers() {
        assert_eq!(test_name("double_exact"), "double_exact");
        assert_eq!(test_name("01-step"), "vector_01_step");
        assert_eq!(test_name("a b.c"), "a_b_c");
        assert_eq!(test_name("fn"), "vector_fn");
        assert_eq!(test_name("self"), "vector_self");
    }

    #[test]
    fn expands_one_test_per_file() {
        let files = [
            PathBuf::from("x/double_exact.json"),
            PathBuf::from("x/a.json"),
        ];
        let run: Expr = syn::parse_quote!(|v| run(v));
        let out = vector_tests(Span::call_site(), &files, &run).expect("expands");
        let file: syn::File = syn::parse2(out).expect("items");
        let names: Vec<String> = file
            .items
            .iter()
            .map(|item| match item {
                syn::Item::Fn(f) => f.sig.ident.to_string(),
                _ => panic!("expected only fn items"),
            })
            .collect();
        assert_eq!(names, ["double_exact", "a"]);
    }

    #[test]
    fn colliding_names_are_an_error() {
        let files = [PathBuf::from("x/a-b.json"), PathBuf::from("x/a_b.json")];
        let run: Expr = syn::parse_quote!(|v| run(v));
        let err = vector_tests(Span::call_site(), &files, &run).expect_err("collides");
        assert!(err.to_string().contains("both become test `a_b`"), "{err}");
    }

    #[test]
    fn glob_matching_nothing_is_an_error() {
        let input = quote! { "vectors/*.missing", |v| run(v) };
        let err = expand(input).expect_err("no match");
        assert!(err.to_string().contains("no file matches"), "{err}");
    }

    #[test]
    fn wildcard_in_directory_is_an_error() {
        let input = quote! { "vec*/*.json", |v| run(v) };
        let err = expand(input).expect_err("directory glob");
        assert!(
            err.to_string().contains("only supported in the file name"),
            "{err}"
        );
    }
}
