//! Compile-time parameter index for the preset tooling.
//!
//! Two cooperating proc-macros wire this up:
//!
//! 1. `derive(Params)` writes a *per-struct sidecar* at
//!    `<target>/param-index/<crate>/<struct>.params.toml` with this
//!    struct's own params (id, field, name, unit) and `#[nested]` child
//!    type names.
//! 2. `__moose_param_index_root!(<params_type>)` (invoked by
//!    `moose::plugin!`) walks the root struct's sidecar and its
//!    `[[nested]]` references, rebasing ids exactly like the runtime
//!    `offset_ids`, and writes the flattened `param_index.toml` that
//!    `cargo moose` resolves `.preset` field names through.
//!
//! Cross-crate `#[nested]` types are unsupported: the aggregator only
//! looks in the plugin crate's own sidecar directory.

use crate::{ParamField, name_hash_id};
use moose_params::AUTO_PARAM_ID_MASK;
use proc_macro::TokenStream;
use quote::quote;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use syn::Type;

/// Write the per-struct sidecar. Best-effort: a missing sidecar
/// surfaces later when the root aggregates.
pub(crate) fn write_struct_sidecar(
    struct_name: &syn::Ident,
    hash_scheme: bool,
    params: &[ParamField],
    nested: &[(syn::Ident, Type, Option<u32>)],
) {
    let Some(out_dir) = sidecar_dir() else {
        return;
    };
    if std::fs::create_dir_all(&out_dir).is_err() {
        return;
    }
    let mut buf = String::new();
    let _ = writeln!(buf, "struct = \"{struct_name}\"");
    // Drives how the aggregator bases nested groups, as at runtime.
    let _ = writeln!(
        buf,
        "scheme = \"{}\"\n",
        if hash_scheme { "hash" } else { "ordinal" }
    );
    for p in params {
        let name = p.attrs.name.clone().unwrap_or_else(|| p.ident.to_string());
        write_param(
            &mut buf,
            p.id(),
            &p.ident.to_string(),
            &name,
            p.attrs.unit.as_deref().unwrap_or(""),
        );
    }
    for (field, ty, base) in nested {
        let Type::Path(syn::TypePath { path, .. }) = ty else {
            continue;
        };
        let Some(t) = path.segments.last() else {
            continue;
        };
        buf.push_str("[[nested]]\n");
        let _ = writeln!(buf, "type = \"{}\"", t.ident);
        let _ = writeln!(buf, "field = \"{}\"", toml_escape(&field.to_string()));
        if let Some(b) = base {
            let _ = writeln!(buf, "base = {b}");
        }
        buf.push('\n');
    }
    let _ = std::fs::write(out_dir.join(format!("{struct_name}.params.toml")), buf);
}

fn write_param(buf: &mut String, id: u32, field: &str, name: &str, unit: &str) {
    buf.push_str("[[param]]\n");
    let _ = writeln!(buf, "id = {id}");
    let _ = writeln!(buf, "field = \"{}\"", toml_escape(field));
    let _ = writeln!(buf, "name = \"{}\"", toml_escape(name));
    if !unit.is_empty() {
        let _ = writeln!(buf, "unit = \"{}\"", toml_escape(unit));
    }
    buf.push('\n');
}

/// One flattened param: `(global id, field, name, unit)`.
type IndexEntry = (u32, String, String, String);

/// Implementation of `__moose_param_index_root!(<params_type>)`.
/// Errors surface as `compile_error!` so the author sees them at build
/// time.
pub(crate) fn emit_root_impl(input: TokenStream) -> TokenStream {
    let path: syn::Path = match syn::parse(input) {
        Ok(p) => p,
        Err(e) => return e.to_compile_error().into(),
    };
    let Some(seg) = path.segments.last() else {
        return quote! { compile_error!("__moose_param_index_root!: empty params path"); }.into();
    };
    let root_struct = seg.ident.to_string();

    // No plugin in moose.toml = nothing to index (helper crate).
    let Ok((config, pkg_name, moose_toml_path)) = crate::try_resolve_plugin() else {
        return TokenStream::new();
    };
    if !config.plugin.iter().any(|p| p.crate_name == pkg_name) {
        return TokenStream::new();
    }
    let Some(sidecar_dir) = sidecar_dir_for(&pkg_name, &moose_toml_path) else {
        return TokenStream::new();
    };

    let mut entries: Vec<IndexEntry> = Vec::new();
    let mut ancestors = std::collections::HashSet::new();
    if let Err(msg) = aggregate(&sidecar_dir, &root_struct, 0, &mut entries, &mut ancestors) {
        return quote! { compile_error!(#msg); }.into();
    }

    let mut index = String::new();
    for (id, field, name, unit) in &entries {
        write_param(&mut index, *id, field, name, unit);
    }
    let _ = std::fs::write(sidecar_dir.join("param_index.toml"), index);
    TokenStream::new()
}

/// Recursively walk `<root>.params.toml`, then each `[[nested]]`
/// reference, accumulating rebased entries.
fn aggregate(
    sidecar_dir: &Path,
    struct_name: &str,
    id_base: u32,
    entries: &mut Vec<IndexEntry>,
    ancestors: &mut std::collections::HashSet<String>,
) -> Result<(), String> {
    // Track the active path, not every visited struct: one Params type
    // reused in two `#[nested]` slots is walked twice, a cycle is not.
    if !ancestors.insert(struct_name.to_string()) {
        return Err(format!(
            "cyclic #[nested] reference through `{struct_name}`"
        ));
    }
    let path = sidecar_dir.join(format!("{struct_name}.params.toml"));
    let content = std::fs::read_to_string(&path).map_err(|e| {
        format!(
            "no param sidecar at {}: {e}. derive(Params) writes one for each \
             Params struct during compile. Either the type lives in another \
             crate (cross-crate #[nested] is unsupported), or `moose::plugin!` \
             sits lexically above the `{struct_name}` struct - move it below.",
            path.display()
        )
    })?;
    let toml: toml::Table = content
        .parse()
        .map_err(|e| format!("malformed {}: {e}", path.display()))?;
    let hash_scheme = toml.get("scheme").and_then(toml::Value::as_str) != Some("ordinal");
    let str_of = |v: &toml::Value, key: &str| {
        v.get(key)
            .and_then(toml::Value::as_str)
            .unwrap_or_default()
            .to_string()
    };

    let mut own_count = 0u32;
    if let Some(toml::Value::Array(arr)) = toml.get("param") {
        for entry in arr {
            let id = entry
                .get("id")
                .and_then(toml::Value::as_integer)
                .and_then(|i| u32::try_from(i).ok())
                .ok_or_else(|| format!("{}: [[param]].id missing", path.display()))?;
            entries.push((
                moose_params::rebase_nested_param_id(id, id_base),
                str_of(entry, "field"),
                str_of(entry, "name"),
                str_of(entry, "unit"),
            ));
            own_count += 1;
        }
    }
    if let Some(toml::Value::Array(arr)) = toml.get("nested") {
        // Ordinal packs each auto group after the previous ones (by
        // size, ignoring earlier explicit bases, like runtime
        // `offset_ids`); hash bases each group on its slot field name.
        let mut next_base = own_count;
        for entry in arr {
            let Some(t) = entry.get("type").and_then(toml::Value::as_str) else {
                continue;
            };
            let auto_base = if hash_scheme {
                entry
                    .get("field")
                    .and_then(toml::Value::as_str)
                    .map_or(0, name_hash_id)
            } else {
                next_base
            };
            let base = entry
                .get("base")
                .and_then(toml::Value::as_integer)
                .and_then(|b| u32::try_from(b).ok())
                .unwrap_or(auto_base);
            let before = entries.len();
            aggregate(
                sidecar_dir,
                t,
                (id_base + base) & AUTO_PARAM_ID_MASK,
                entries,
                ancestors,
            )?;
            next_base += u32::try_from(entries.len() - before).unwrap_or(0);
        }
    }
    ancestors.remove(struct_name);
    Ok(())
}

fn toml_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Where to write a per-struct sidecar for the *current* compile.
fn sidecar_dir() -> Option<PathBuf> {
    let pkg_name = std::env::var("CARGO_PKG_NAME").ok()?;
    let moose_toml = moose_build::find_moose_toml().ok()?;
    sidecar_dir_for(&pkg_name, &moose_toml)
}

fn sidecar_dir_for(pkg_name: &str, moose_toml: &Path) -> Option<PathBuf> {
    let workspace_root = moose_toml.parent()?;
    Some(moose_build::param_index_dir(
        &moose_build::target_dir(workspace_root),
        pkg_name,
    ))
}
