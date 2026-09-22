//! Proc macros for the Elyra desktop framework: `#[command]` and `#[derive(Model)]`.

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{
    parse_macro_input, Data, DeriveInput, Fields, FnArg, GenericArgument, Ident, ItemFn, LitStr,
    Pat, PathArguments, ReturnType, Type,
};

/// If the return type is `Result<T, _>`, return `T` (used for codegen + error mapping).
fn result_ok_type(output: &ReturnType) -> Option<Type> {
    let ReturnType::Type(_, ty) = output else {
        return None;
    };
    let Type::Path(type_path) = &**ty else {
        return None;
    };
    let segment = type_path.path.segments.last()?;
    if segment.ident != "Result" {
        return None;
    }
    let PathArguments::AngleBracketed(args) = &segment.arguments else {
        return None;
    };
    args.args.iter().find_map(|arg| match arg {
        GenericArgument::Type(t) => Some(t.clone()),
        _ => None,
    })
}

// ---------------------------------------------------------------------------
// #[derive(Model)]
// ---------------------------------------------------------------------------

fn is_bool(ty: &Type) -> bool {
    matches!(ty, Type::Path(p) if p.path.segments.last().is_some_and(|s| s.ident == "bool"))
}

fn is_i64(ty: &Type) -> bool {
    matches!(ty, Type::Path(p) if p.path.segments.last().is_some_and(|s| s.ident == "i64"))
}

/// Whether `s` is a bare SQL identifier. Table and column names are spliced
/// into SQL rather than bound, so they are checked here, at compile time.
fn is_sql_ident(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// A string literal that must be an SQL identifier.
fn lit_ident(lit: LitStr) -> syn::Result<String> {
    let value = lit.value();
    if is_sql_ident(&value) {
        Ok(value)
    } else {
        Err(syn::Error::new_spanned(
            &lit,
            "must be a bare SQL identifier (letters, digits and `_`)",
        ))
    }
}

/// Field metadata resolved from the struct + `#[model(..)]` attributes.
struct ModelField {
    ident: Ident,
    ty: Type,
    column: String,
    is_pk: bool,
    is_bool: bool,
    /// `#[model(cast = ..)]`: the `Cast` implementor that stores this field.
    cast: Option<syn::Path>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RelKind {
    HasMany,
    HasOne,
    BelongsTo,
    BelongsToMany,
}

impl RelKind {
    fn from_path(path: &syn::Path) -> Option<Self> {
        if path.is_ident("has_many") {
            Some(RelKind::HasMany)
        } else if path.is_ident("has_one") {
            Some(RelKind::HasOne)
        } else if path.is_ident("belongs_to") {
            Some(RelKind::BelongsTo)
        } else if path.is_ident("belongs_to_many") {
            Some(RelKind::BelongsToMany)
        } else {
            None
        }
    }
}

/// A relation, declared on the struct (`#[model(has_many(Post, as = "posts"))]`)
/// or on a field whose rows it hydrates (`#[model(has_many(Book))] books: Vec<Book>`).
struct Relation {
    kind: RelKind,
    ty: Ident,
    fk: Option<String>,
    /// `as = ".."`: the method name. Struct-level relations only — a field
    /// relation is named by its field.
    name: Option<String>,
    /// `belongs_to_many` only: the join table, and its key to the related side.
    pivot: Option<String>,
    related_fk: Option<String>,
}

/// A relation declared on a field, hydrated straight into it.
struct FieldRelation {
    field: Ident,
    rel: Relation,
}

fn parse_relation(
    kind: RelKind,
    meta: &syn::meta::ParseNestedMeta,
    on_field: bool,
) -> syn::Result<Relation> {
    let mut ty: Option<Ident> = None;
    let mut fk = None;
    let mut name = None;
    let mut pivot = None;
    let mut related_fk = None;
    meta.parse_nested_meta(|inner| {
        if inner.path.is_ident("fk") {
            fk = Some(lit_ident(inner.value()?.parse()?)?);
        } else if inner.path.is_ident("as") {
            if on_field {
                return Err(inner.error(
                    "`as` names a struct-level relation's method; a field relation is named by its field",
                ));
            }
            let lit: LitStr = inner.value()?.parse()?;
            if syn::parse_str::<Ident>(&lit.value()).is_err() {
                return Err(syn::Error::new_spanned(&lit, "`as` must be a valid method name"));
            }
            name = Some(lit.value());
        } else if inner.path.is_ident("pivot") || inner.path.is_ident("related_fk") {
            if kind != RelKind::BelongsToMany {
                return Err(inner.error("`pivot` and `related_fk` only apply to `belongs_to_many`"));
            }
            let value = lit_ident(inner.value()?.parse()?)?;
            if inner.path.is_ident("pivot") {
                pivot = Some(value);
            } else {
                related_fk = Some(value);
            }
        } else if let Some(id) = inner.path.get_ident() {
            if ty.is_some() {
                return Err(inner.error("a relation names exactly one related model"));
            }
            ty = Some(id.clone());
        } else {
            return Err(inner.error(
                "expected the related model, `fk`, `as`, `pivot` or `related_fk`",
            ));
        }
        Ok(())
    })?;
    let ty =
        ty.ok_or_else(|| meta.error("a relation needs the related model, e.g. `has_many(Post)`"))?;
    Ok(Relation {
        kind,
        ty,
        fk,
        name,
        pivot,
        related_fk,
    })
}

/// `cast = "json"` / `cast = "text"` / `cast = path::To::Caster`.
fn parse_cast(meta: &syn::meta::ParseNestedMeta) -> syn::Result<syn::Path> {
    let value = meta.value()?;
    if value.peek(LitStr) {
        let lit: LitStr = value.parse()?;
        return match lit.value().as_str() {
            "json" => Ok(syn::parse_quote!(::elyra::db::cast::Json)),
            "text" => Ok(syn::parse_quote!(::elyra::db::cast::Text)),
            other => Err(syn::Error::new_spanned(
                &lit,
                format!(
                    "unknown cast \"{other}\"; expected \"json\", \"text\", or a path to a \
                     type implementing `elyra::db::cast::Cast`"
                ),
            )),
        };
    }
    value.parse::<syn::Path>()
}

/// `#[derive(Model)]` — Active-Record CRUD + query builder over `elyra::db`.
///
/// ```ignore
/// #[derive(Model, Serialize, Deserialize, specta::Type)]
/// #[model(table = "todos", timestamps)]
/// struct Todo {
///     #[model(id)] id: i64,
///     title: String,
///     done: bool,                       // <-> INTEGER 0/1 column
///     #[model(column = "body")] text: String,
///     #[model(cast = "json")] tags: Vec<String>,
///     created_at: i64,
///     updated_at: i64,
/// }
/// ```
///
/// Notes: `bool` fields map to an INTEGER `0/1` column (the `Any` driver can't
/// read SQLite's native `BOOLEAN` type). `soft_deletes` makes queries skip rows
/// whose `deleted_at` is set (see `Query::with_trashed` / `only_trashed`).
/// `timestamps` auto-manages `created_at` / `updated_at` (unix seconds).
/// `global_scope = path` constrains every query (repeatable). Unknown options
/// are compile errors. v1 assumes an `i64` autoincrement primary key.
#[proc_macro_derive(Model, attributes(model))]
pub fn derive_model(item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    let name = input.ident.clone();

    let fields = match &input.data {
        Data::Struct(s) => match &s.fields {
            Fields::Named(named) => named.named.clone(),
            _ => {
                return syn::Error::new_spanned(&input, "Model requires named fields")
                    .to_compile_error()
                    .into()
            }
        },
        _ => {
            return syn::Error::new_spanned(&input, "Model can only derive for structs")
                .to_compile_error()
                .into()
        }
    };

    // Struct-level: table, timestamps, soft_deletes, global_scope, relations.
    // Unknown keys are errors: a misspelt `global_scope` silently dropped would
    // be a query that leaks across tenants.
    let mut table = name.to_string().to_lowercase();
    let mut timestamps = false;
    let mut soft_deletes = false;
    let mut global_scopes: Vec<syn::Path> = Vec::new();
    let mut relations: Vec<Relation> = Vec::new();
    for attr in &input.attrs {
        if !attr.path().is_ident("model") {
            continue;
        }
        let parsed = attr.parse_nested_meta(|meta| {
            if let Some(kind) = RelKind::from_path(&meta.path) {
                relations.push(parse_relation(kind, &meta, false)?);
            } else if meta.path.is_ident("table") {
                let lit: LitStr = meta.value()?.parse()?;
                // One `schema.table` qualifier is allowed.
                if !lit.value().split('.').all(is_sql_ident) || lit.value().matches('.').count() > 1
                {
                    return Err(syn::Error::new_spanned(
                        &lit,
                        "table must be a bare SQL identifier, optionally `schema.table`",
                    ));
                }
                table = lit.value();
            } else if meta.path.is_ident("timestamps") {
                timestamps = true;
            } else if meta.path.is_ident("soft_deletes") {
                soft_deletes = true;
            } else if meta.path.is_ident("global_scope") {
                global_scopes.push(meta.value()?.parse()?);
            } else {
                return Err(meta.error(
                    "unknown #[model] option; expected `table`, `timestamps`, `soft_deletes`, \
                     `global_scope`, or a relation (`has_many`, `has_one`, `belongs_to`, \
                     `belongs_to_many`)",
                ));
            }
            Ok(())
        });
        if let Err(e) = parsed {
            return e.to_compile_error().into();
        }
    }

    // Per-field: id / column / cast / a relation.
    let mut infos: Vec<ModelField> = Vec::new();
    let mut field_relations: Vec<FieldRelation> = Vec::new();
    for field in &fields {
        let ident = field.ident.clone().unwrap();
        let mut column = ident.to_string();
        let mut is_pk = false;
        let mut cast: Option<syn::Path> = None;
        let mut rel: Option<Relation> = None;
        for attr in &field.attrs {
            if !attr.path().is_ident("model") {
                continue;
            }
            let parsed = attr.parse_nested_meta(|meta| {
                if let Some(kind) = RelKind::from_path(&meta.path) {
                    if rel.is_some() {
                        return Err(meta.error("a field holds at most one relation"));
                    }
                    rel = Some(parse_relation(kind, &meta, true)?);
                } else if meta.path.is_ident("id") {
                    is_pk = true;
                } else if meta.path.is_ident("column") {
                    column = lit_ident(meta.value()?.parse()?)?;
                } else if meta.path.is_ident("cast") {
                    cast = Some(parse_cast(&meta)?);
                } else {
                    return Err(meta.error(
                        "unknown #[model] field option; expected `id`, `column`, `cast`, or a \
                         relation (`has_many`, `has_one`, `belongs_to`, `belongs_to_many`)",
                    ));
                }
                Ok(())
            });
            if let Err(e) = parsed {
                return e.to_compile_error().into();
            }
        }
        if let Some(rel) = rel {
            if is_pk || cast.is_some() {
                return syn::Error::new_spanned(
                    field,
                    "a relation field is hydrated, not stored: it can't also be `id` or `cast`",
                )
                .to_compile_error()
                .into();
            }
            // A relation field is hydrated, not stored: skip it as a column.
            field_relations.push(FieldRelation { field: ident, rel });
            continue;
        }
        if is_pk && cast.is_some() {
            return syn::Error::new_spanned(field, "the primary key can't be a cast field")
                .to_compile_error()
                .into();
        }
        infos.push(ModelField {
            is_bool: is_bool(&field.ty),
            ident,
            ty: field.ty.clone(),
            column,
            is_pk,
            cast,
        });
    }

    // Primary key: flagged field, else one whose column is "id".
    if !infos.iter().any(|f| f.is_pk) {
        if let Some(f) = infos
            .iter_mut()
            .find(|f| f.column == "id" && f.cast.is_none())
        {
            f.is_pk = true;
        }
    }
    let (pk_ident, pk_col, pk_is_bool, pk_ty) = match infos.iter().find(|f| f.is_pk) {
        Some(f) => (f.ident.clone(), f.column.clone(), f.is_bool, f.ty.clone()),
        None => {
            return syn::Error::new_spanned(
                &input,
                "Model needs a primary key: a field with column `id` or marked #[model(id)]",
            )
            .to_compile_error()
            .into()
        }
    };

    let all_cols: Vec<String> = infos.iter().map(|f| f.column.clone()).collect();

    // `i64` is the default auto-increment key; any other type is app-supplied.
    let pk_is_i64 = is_i64(&pk_ty);

    let has_many_to_many = relations
        .iter()
        .chain(field_relations.iter().map(|f| &f.rel))
        .any(|r| r.kind == RelKind::BelongsToMany);
    if has_many_to_many && !pk_is_i64 {
        return syn::Error::new_spanned(
            &input,
            "`belongs_to_many` needs an `i64` primary key (pivot keys are integers)",
        )
        .to_compile_error()
        .into();
    }

    // Columns for UPDATE ... SET (everything except the primary key).
    let insert: Vec<&ModelField> = infos.iter().filter(|f| !f.is_pk).collect();
    let insert_cols: Vec<String> = insert.iter().map(|f| f.column.clone()).collect();

    let mut from_row_fields: Vec<_> = infos
        .iter()
        .map(|f| {
            let ident = &f.ident;
            let col = &f.column;
            let ty = &f.ty;
            if let Some(cast) = &f.cast {
                quote! { #ident: <#cast as ::elyra::db::cast::Cast<#ty>>::decode(__row, #col)? }
            } else if f.is_bool {
                quote! { #ident: ::elyra::db::sqlx::Row::try_get::<i64, _>(__row, #col)? != 0 }
            } else {
                quote! { #ident: ::elyra::db::sqlx::Row::try_get::<#ty, _>(__row, #col)? }
            }
        })
        .collect();
    // Relation fields aren't columns; hydrate them to their default (empty).
    for r in &field_relations {
        let f = &r.field;
        from_row_fields.push(quote! { #f: ::std::default::Default::default() });
    }

    // Match arms for get_i64: i64 columns return the value; bool columns 0/1.
    let i64_arms: Vec<_> = infos
        .iter()
        .filter(|f| f.cast.is_none())
        .filter_map(|f| {
            let ident = &f.ident;
            let col = &f.column;
            if is_i64(&f.ty) {
                Some(quote! { #col => ::std::option::Option::Some(self.#ident), })
            } else if f.is_bool {
                Some(quote! { #col => ::std::option::Option::Some(self.#ident as i64), })
            } else {
                None
            }
        })
        .collect();

    // Writes build one argument list, so cast fields (encoded to a `Value`) and
    // plain fields (bound as-is) can sit side by side.
    let arg_of = |f: &ModelField| {
        let ident = &f.ident;
        if let Some(cast) = &f.cast {
            let ty = &f.ty;
            quote! {
                ::elyra::db::model::bind_value(
                    &mut __args,
                    &<#cast as ::elyra::db::cast::Cast<#ty>>::encode(&self.#ident)?,
                )?;
            }
        } else if f.is_bool {
            quote! { ::elyra::db::model::bind_arg(&mut __args, if self.#ident { 1i64 } else { 0i64 })?; }
        } else {
            quote! { ::elyra::db::model::bind_arg(&mut __args, ::std::clone::Clone::clone(&self.#ident))?; }
        }
    };
    let update_args: Vec<_> = insert.iter().map(|f| arg_of(f)).collect();
    let pk_field = infos.iter().find(|f| f.is_pk).expect("pk resolved above");
    let pk_arg = arg_of(pk_field);
    let pk_bind = if pk_is_bool {
        quote! { .bind(if self.#pk_ident { 1i64 } else { 0i64 }) }
    } else {
        quote! { .bind(::std::clone::Clone::clone(&self.#pk_ident)) }
    };

    // INSERT column set: omit the PK only for the default i64 auto-increment key
    // (the DB assigns it); for any other PK type the app supplies the value.
    let create: Vec<&ModelField> = if pk_is_i64 {
        infos.iter().filter(|f| !f.is_pk).collect()
    } else {
        infos.iter().collect()
    };
    let create_cols_str = create
        .iter()
        .map(|f| f.column.clone())
        .collect::<Vec<_>>()
        .join(", ");
    let n_create = create.len();
    let create_args: Vec<_> = create.iter().map(|f| arg_of(f)).collect();

    // The insert body differs by key strategy: an i64 PK is read back from the
    // database (RETURNING / last_insert_id); any other PK is supplied by the app.
    let insert_body = if pk_is_i64 {
        quote! {
            let __phs = ::elyra::db::model::placeholders(__db.driver(), #n_create);
            let mut __args = ::elyra::db::sqlx::any::AnyArguments::default();
            #( #create_args )*
            match __db.driver() {
                ::elyra::db::Driver::MySql => {
                    let __sql = ::std::format!("INSERT INTO {} ({}) VALUES ({})", #table, #create_cols_str, __phs);
                    let __res = ::elyra::db::sqlx::query_with(::elyra::db::sqlx::AssertSqlSafe(__sql), __args)
                        .execute(__db.pool()).await?;
                    if let ::std::option::Option::Some(__id) = __res.last_insert_id() {
                        self.#pk_ident = __id;
                    }
                }
                _ => {
                    let __sql = ::std::format!(
                        "INSERT INTO {} ({}) VALUES ({}) RETURNING {}",
                        #table, #create_cols_str, __phs, #pk_col
                    );
                    let __row = ::elyra::db::sqlx::query_with(::elyra::db::sqlx::AssertSqlSafe(__sql), __args)
                        .fetch_one(__db.pool()).await?;
                    self.#pk_ident = ::elyra::db::sqlx::Row::try_get::<i64, _>(&__row, #pk_col)?;
                }
            }
        }
    } else {
        quote! {
            let __phs = ::elyra::db::model::placeholders(__db.driver(), #n_create);
            let mut __args = ::elyra::db::sqlx::any::AnyArguments::default();
            #( #create_args )*
            let __sql = ::std::format!("INSERT INTO {} ({}) VALUES ({})", #table, #create_cols_str, __phs);
            ::elyra::db::sqlx::query_with(::elyra::db::sqlx::AssertSqlSafe(__sql), __args)
                .execute(__db.pool()).await?;
        }
    };

    // `save()` treats a default-valued PK as "unsaved": 0 for i64, "" for String.
    let save_is_new = if pk_is_i64 {
        quote! { self.#pk_ident == 0 }
    } else {
        quote! { self.#pk_ident == <#pk_ty as ::std::default::Default>::default() }
    };

    // Timestamps (unix seconds), matched by column name.
    let now_expr = quote! {
        ::std::time::SystemTime::now()
            .duration_since(::std::time::UNIX_EPOCH)
            .map(|__d| __d.as_secs() as i64)
            .unwrap_or(0)
    };
    let created = timestamps
        .then(|| {
            infos
                .iter()
                .find(|f| f.column == "created_at")
                .map(|f| f.ident.clone())
        })
        .flatten();
    let updated = timestamps
        .then(|| {
            infos
                .iter()
                .find(|f| f.column == "updated_at")
                .map(|f| f.ident.clone())
        })
        .flatten();
    let insert_ts = {
        let mut t = quote! {};
        if created.is_some() || updated.is_some() {
            t = quote! { let __now = #now_expr; };
        }
        if let Some(i) = &created {
            t = quote! { #t self.#i = __now; };
        }
        if let Some(i) = &updated {
            t = quote! { #t self.#i = __now; };
        }
        t
    };
    let update_ts = match &updated {
        Some(i) => quote! { let __now = #now_expr; self.#i = __now; },
        None => quote! {},
    };

    let self_lower = name.to_string().to_lowercase();

    // belongs_to_many: the pivot (defaults follow Laravel — the two model names
    // in alphabetical order, `<model>_id` keys) and its write methods, shared by
    // struct- and field-level declarations.
    let pivot_of = |rel: &Relation| {
        let ty_lower = rel.ty.to_string().to_lowercase();
        let table = rel.pivot.clone().unwrap_or_else(|| {
            let mut pair = [self_lower.clone(), ty_lower.clone()];
            pair.sort();
            pair.join("_")
        });
        let fk = rel.fk.clone().unwrap_or_else(|| format!("{self_lower}_id"));
        let related_fk = rel
            .related_fk
            .clone()
            .unwrap_or_else(|| format!("{ty_lower}_id"));
        quote! {
            ::elyra::db::model::Pivot { table: #table, parent_fk: #fk, related_fk: #related_fk }
        }
    };
    let pivot_methods = |suffix: &str, ty: &Ident, pivot: &proc_macro2::TokenStream| {
        let query = format_ident!("{}_query", suffix);
        let attach = format_ident!("attach_{}", suffix);
        let detach = format_ident!("detach_{}", suffix);
        let detach_all = format_ident!("detach_all_{}", suffix);
        let sync = format_ident!("sync_{}", suffix);
        let sync_wd = format_ident!("sync_{}_without_detaching", suffix);
        quote! {
            /// The related rows as a query to keep narrowing (belongs_to_many).
            pub fn #query(&self) -> ::elyra::db::model::Query<#ty> {
                #pivot.query::<#ty>(self.#pk_ident)
            }
            /// Attach related ids through the pivot (belongs_to_many).
            pub async fn #attach(&self, __db: &::elyra::db::Database, __ids: impl ::std::iter::IntoIterator<Item = i64>) -> ::elyra::db::Result<u64> {
                #pivot.attach(__db, self.#pk_ident, __ids).await
            }
            /// Detach related ids from the pivot (belongs_to_many).
            pub async fn #detach(&self, __db: &::elyra::db::Database, __ids: impl ::std::iter::IntoIterator<Item = i64>) -> ::elyra::db::Result<u64> {
                #pivot.detach(__db, self.#pk_ident, __ids).await
            }
            /// Detach every related row (belongs_to_many).
            pub async fn #detach_all(&self, __db: &::elyra::db::Database) -> ::elyra::db::Result<u64> {
                #pivot.detach_all(__db, self.#pk_ident).await
            }
            /// Make exactly these ids attached, in one transaction (belongs_to_many).
            pub async fn #sync(&self, __db: &::elyra::db::Database, __ids: impl ::std::iter::IntoIterator<Item = i64>) -> ::elyra::db::Result<::elyra::db::model::SyncChanges> {
                #pivot.sync(__db, self.#pk_ident, __ids).await
            }
            /// Attach whichever of these ids are missing; detach nothing (belongs_to_many).
            pub async fn #sync_wd(&self, __db: &::elyra::db::Database, __ids: impl ::std::iter::IntoIterator<Item = i64>) -> ::elyra::db::Result<::elyra::db::model::SyncChanges> {
                #pivot.sync_without_detaching(__db, self.#pk_ident, __ids).await
            }
        }
    };

    // Relation accessor methods.
    let relation_methods: Vec<_> = relations
        .iter()
        .map(|rel| {
            let ty = &rel.ty;
            let ty_lower = ty.to_string().to_lowercase();
            match rel.kind {
                RelKind::HasMany => {
                    let fk = rel.fk.clone().unwrap_or_else(|| format!("{self_lower}_id"));
                    let mname = format_ident!("{}", rel.name.clone().unwrap_or_else(|| format!("{ty_lower}s")));
                    let load = format_ident!("load_{}", mname);
                    quote! {
                        /// Related rows (has_many).
                        pub async fn #mname(&self, __db: &::elyra::db::Database) -> ::elyra::db::Result<::std::vec::Vec<#ty>> {
                            #ty::query().where_eq(#fk, self.#pk_ident).get(__db).await
                        }
                        /// Eager-load this relation for a batch of parents, keyed by primary key.
                        pub async fn #load(__db: &::elyra::db::Database, __parents: &[Self]) -> ::elyra::db::Result<::std::collections::HashMap<i64, ::std::vec::Vec<#ty>>> {
                            ::elyra::db::model::eager_has_many::<Self, #ty>(__db, __parents, #pk_col, #fk).await
                        }
                    }
                }
                RelKind::HasOne => {
                    let fk = rel.fk.clone().unwrap_or_else(|| format!("{self_lower}_id"));
                    let mname = format_ident!("{}", rel.name.clone().unwrap_or_else(|| ty_lower.clone()));
                    let load = format_ident!("load_{}", mname);
                    quote! {
                        /// Related row (has_one).
                        pub async fn #mname(&self, __db: &::elyra::db::Database) -> ::elyra::db::Result<::std::option::Option<#ty>> {
                            #ty::query().where_eq(#fk, self.#pk_ident).first(__db).await
                        }
                        /// Eager-load this relation for a batch of parents, keyed by primary key.
                        pub async fn #load(__db: &::elyra::db::Database, __parents: &[Self]) -> ::elyra::db::Result<::std::collections::HashMap<i64, #ty>> {
                            ::elyra::db::model::eager_has_one::<Self, #ty>(__db, __parents, #pk_col, #fk).await
                        }
                    }
                }
                RelKind::BelongsTo => {
                    let fk = rel.fk.clone().unwrap_or_else(|| format!("{ty_lower}_id"));
                    let mname = format_ident!("{}", rel.name.clone().unwrap_or_else(|| ty_lower.clone()));
                    let load = format_ident!("load_{}", mname);
                    quote! {
                        /// Owning row (belongs_to). Reads the FK by column name and
                        /// looks it up against the owner's own primary key.
                        pub async fn #mname(&self, __db: &::elyra::db::Database) -> ::elyra::db::Result<::std::option::Option<#ty>> {
                            match <Self as ::elyra::db::model::Model>::get_i64(self, #fk) {
                                ::std::option::Option::Some(__id) => #ty::find(__db, __id).await,
                                ::std::option::Option::None => ::std::result::Result::Ok(::std::option::Option::None),
                            }
                        }
                        /// Eager-load owners for a batch of children, keyed by owner primary key.
                        pub async fn #load(__db: &::elyra::db::Database, __children: &[Self]) -> ::elyra::db::Result<::std::collections::HashMap<i64, #ty>> {
                            ::elyra::db::model::eager_belongs_to::<Self, #ty>(__db, __children, #fk, <#ty as ::elyra::db::model::Model>::PK).await
                        }
                    }
                }
                RelKind::BelongsToMany => {
                    let method = rel.name.clone().unwrap_or_else(|| format!("{ty_lower}s"));
                    let mname = format_ident!("{}", method);
                    let load = format_ident!("load_{}", method);
                    let pivot = pivot_of(rel);
                    let writes = pivot_methods(&method, ty, &pivot);
                    quote! {
                        /// Related rows through the pivot table (belongs_to_many).
                        pub async fn #mname(&self, __db: &::elyra::db::Database) -> ::elyra::db::Result<::std::vec::Vec<#ty>> {
                            #pivot.query::<#ty>(self.#pk_ident).get(__db).await
                        }
                        /// Eager-load this relation for a batch of parents in one query, keyed by primary key.
                        pub async fn #load(__db: &::elyra::db::Database, __parents: &[Self]) -> ::elyra::db::Result<::std::collections::HashMap<i64, ::std::vec::Vec<#ty>>> {
                            #pivot.eager::<Self, #ty>(__db, __parents, #pk_col).await
                        }
                        #writes
                    }
                }
            }
        })
        .collect();

    // Field-relation hydrators: fill `self.<field>` from a batch of parents.
    let field_relation_methods: Vec<_> = field_relations
        .iter()
        .map(|fr| {
            let field = &fr.field;
            let rel = &fr.rel;
            let ty = &rel.ty;
            let with = format_ident!("with_{}", field);
            let ty_lower = ty.to_string().to_lowercase();
            match rel.kind {
                RelKind::HasMany => {
                    let fk = rel.fk.clone().unwrap_or_else(|| format!("{self_lower}_id"));
                    quote! {
                        /// Eager-load this `has_many` relation into `self` for a batch of parents (one query).
                        pub async fn #with(__db: &::elyra::db::Database, __parents: &mut [Self]) -> ::elyra::db::Result<()> {
                            let mut __map = ::elyra::db::model::eager_has_many::<Self, #ty>(__db, __parents, #pk_col, #fk).await?;
                            for __p in __parents.iter_mut() {
                                if let ::std::option::Option::Some(__pk) = <Self as ::elyra::db::model::Model>::get_i64(__p, <Self as ::elyra::db::model::Model>::PK) {
                                    __p.#field = __map.remove(&__pk).unwrap_or_default();
                                }
                            }
                            ::std::result::Result::Ok(())
                        }
                    }
                }
                RelKind::HasOne => {
                    let fk = rel.fk.clone().unwrap_or_else(|| format!("{self_lower}_id"));
                    quote! {
                        /// Eager-load this `has_one` relation into `self` for a batch of parents (one query).
                        pub async fn #with(__db: &::elyra::db::Database, __parents: &mut [Self]) -> ::elyra::db::Result<()> {
                            let mut __map = ::elyra::db::model::eager_has_one::<Self, #ty>(__db, __parents, #pk_col, #fk).await?;
                            for __p in __parents.iter_mut() {
                                if let ::std::option::Option::Some(__pk) = <Self as ::elyra::db::model::Model>::get_i64(__p, <Self as ::elyra::db::model::Model>::PK) {
                                    __p.#field = __map.remove(&__pk);
                                }
                            }
                            ::std::result::Result::Ok(())
                        }
                    }
                }
                RelKind::BelongsTo => {
                    let fk = rel.fk.clone().unwrap_or_else(|| format!("{ty_lower}_id"));
                    quote! {
                        /// Eager-load this `belongs_to` relation into `self` for a batch of children
                        /// (one query; requires the related type to be `Clone`).
                        pub async fn #with(__db: &::elyra::db::Database, __children: &mut [Self]) -> ::elyra::db::Result<()> {
                            let __map = ::elyra::db::model::eager_belongs_to::<Self, #ty>(__db, __children, #fk, <#ty as ::elyra::db::model::Model>::PK).await?;
                            for __c in __children.iter_mut() {
                                if let ::std::option::Option::Some(__fk) = <Self as ::elyra::db::model::Model>::get_i64(__c, #fk) {
                                    __c.#field = __map.get(&__fk).cloned();
                                }
                            }
                            ::std::result::Result::Ok(())
                        }
                    }
                }
                RelKind::BelongsToMany => {
                    let pivot = pivot_of(rel);
                    let writes = pivot_methods(&field.to_string(), ty, &pivot);
                    quote! {
                        /// Eager-load this `belongs_to_many` relation into `self` for a batch of
                        /// parents (one query through the pivot).
                        pub async fn #with(__db: &::elyra::db::Database, __parents: &mut [Self]) -> ::elyra::db::Result<()> {
                            let mut __map = #pivot.eager::<Self, #ty>(__db, __parents, #pk_col).await?;
                            for __p in __parents.iter_mut() {
                                if let ::std::option::Option::Some(__pk) = <Self as ::elyra::db::model::Model>::get_i64(__p, <Self as ::elyra::db::model::Model>::PK) {
                                    __p.#field = __map.remove(&__pk).unwrap_or_default();
                                }
                            }
                            ::std::result::Result::Ok(())
                        }
                        #writes
                    }
                }
            }
        })
        .collect();

    let soft_delete_col = if soft_deletes {
        quote!(::std::option::Option::Some("deleted_at"))
    } else {
        quote!(::std::option::Option::None)
    };

    let global_scope_items = if global_scopes.is_empty() {
        quote! {}
    } else {
        quote! {
            const HAS_GLOBAL_SCOPE: bool = true;

            fn global_scope(__q: ::elyra::db::model::Query<Self>) -> ::elyra::db::model::Query<Self> {
                #( let __q = #global_scopes(__q); )*
                __q
            }
        }
    };

    let expanded = quote! {
        impl ::elyra::db::model::Model for #name {
            const TABLE: &'static str = #table;
            const PK: &'static str = #pk_col;
            const COLUMNS: &'static [&'static str] = &[ #(#all_cols),* ];
            const SOFT_DELETE: ::std::option::Option<&'static str> = #soft_delete_col;

            fn from_row(__row: &::elyra::db::sqlx::any::AnyRow) -> ::elyra::db::Result<Self> {
                ::std::result::Result::Ok(Self { #( #from_row_fields ),* })
            }

            fn get_i64(&self, __column: &str) -> ::std::option::Option<i64> {
                match __column {
                    #( #i64_arms )*
                    _ => ::std::option::Option::None,
                }
            }

            #global_scope_items
        }

        impl ::elyra::db::model::Persist for #name {
            fn insert<'__a>(
                &'__a mut self,
                __db: &'__a ::elyra::db::Database,
            ) -> impl ::std::future::Future<Output = ::elyra::db::Result<()>> + ::std::marker::Send + '__a {
                #name::insert(self, __db)
            }
        }

        impl #name {
            /// All rows (respecting soft deletes and global scopes).
            pub async fn all(__db: &::elyra::db::Database) -> ::elyra::db::Result<::std::vec::Vec<Self>> {
                Self::query().get(__db).await
            }

            /// Find one row by primary key (respecting soft deletes and global
            /// scopes — `Self::query().with_trashed().find(..)` reaches a trashed row).
            pub async fn find(__db: &::elyra::db::Database, __id: #pk_ty) -> ::elyra::db::Result<::std::option::Option<Self>> {
                Self::query().find(__db, __id).await
            }

            /// Start a typed query.
            pub fn query() -> ::elyra::db::model::Query<Self> {
                ::elyra::db::model::Query::new()
            }

            /// Insert as a new row. For an `i64` primary key the database assigns
            /// it and it's read back into `self`; other key types are supplied by
            /// the caller before insert.
            pub async fn insert(&mut self, __db: &::elyra::db::Database) -> ::elyra::db::Result<()> {
                #insert_ts
                #insert_body
                ::std::result::Result::Ok(())
            }

            /// Update this row by primary key.
            pub async fn update(&mut self, __db: &::elyra::db::Database) -> ::elyra::db::Result<()> {
                #update_ts
                let __cols: &[&str] = &[ #(#insert_cols),* ];
                let mut __i = 1usize;
                let __sets: ::std::vec::Vec<::std::string::String> = __cols.iter().map(|__c| {
                    let __p = ::elyra::db::model::placeholder(__db.driver(), __i);
                    __i += 1;
                    ::std::format!("{} = {}", __c, __p)
                }).collect();
                let __pkph = ::elyra::db::model::placeholder(__db.driver(), __i);
                let __sql = ::std::format!(
                    "UPDATE {} SET {} WHERE {} = {}",
                    #table, __sets.join(", "), #pk_col, __pkph
                );
                let mut __args = ::elyra::db::sqlx::any::AnyArguments::default();
                #( #update_args )*
                #pk_arg
                ::elyra::db::sqlx::query_with(::elyra::db::sqlx::AssertSqlSafe(__sql), __args)
                    .execute(__db.pool()).await?;
                ::std::result::Result::Ok(())
            }

            /// Delete this row by primary key.
            pub async fn delete(&self, __db: &::elyra::db::Database) -> ::elyra::db::Result<()> {
                let __sql = ::std::format!(
                    "DELETE FROM {} WHERE {} = {}",
                    #table, #pk_col, ::elyra::db::model::placeholder(__db.driver(), 1)
                );
                ::elyra::db::sqlx::query(::elyra::db::sqlx::AssertSqlSafe(__sql))
                    #pk_bind
                    .execute(__db.pool()).await?;
                ::std::result::Result::Ok(())
            }

            /// Insert if unsaved (PK is its type's default), otherwise update.
            pub async fn save(&mut self, __db: &::elyra::db::Database) -> ::elyra::db::Result<()> {
                if #save_is_new {
                    self.insert(__db).await
                } else {
                    self.update(__db).await
                }
            }

            #( #relation_methods )*
            #( #field_relation_methods )*
        }
    };

    expanded.into()
}

// ---------------------------------------------------------------------------
// #[command]
// ---------------------------------------------------------------------------

#[proc_macro_attribute]
pub fn command(attr: TokenStream, item: TokenStream) -> TokenStream {
    // `#[command(can = "posts.delete")]` — the ability the *frontend* must hold
    // to reach this command. Anything else in the attribute is a typo, so it is
    // an error rather than silently ignored (as the whole attribute used to be).
    let mut ability: Option<syn::LitStr> = None;
    let mut middleware: Vec<syn::LitStr> = Vec::new();
    if !attr.is_empty() {
        let parser =
            syn::meta::parser(|meta| {
                if meta.path.is_ident("can") {
                    ability = Some(meta.value()?.parse()?);
                    Ok(())
                } else if meta.path.is_ident("middleware") {
                    // `middleware = "auth"` or `middleware = ["auth", "audit"]`.
                    let value = meta.value()?;
                    if value.peek(syn::token::Bracket) {
                        let list: syn::ExprArray = value.parse()?;
                        for item in list.elems {
                            match item {
                                syn::Expr::Lit(syn::ExprLit {
                                    lit: syn::Lit::Str(lit),
                                    ..
                                }) => middleware.push(lit),
                                other => return Err(syn::Error::new_spanned(
                                    other,
                                    "middleware names are string literals: `[\"auth\", \"audit\"]`",
                                )),
                            }
                        }
                    } else {
                        middleware.push(value.parse()?);
                    }
                    Ok(())
                } else {
                    Err(meta.error(
                        "unknown #[command] option; expected `can = \"ability\"` or \
                     `middleware = [\"name\", ..]`",
                    ))
                }
            });
        if let Err(e) = syn::parse::Parser::parse(parser, attr) {
            return e.to_compile_error().into();
        }
    }
    if let Some(lit) = &ability {
        let value = lit.value();
        if value.trim().is_empty() || value.contains(char::is_whitespace) {
            return syn::Error::new_spanned(
                lit,
                "the `can` ability must be a non-empty string without whitespace, \
                 e.g. `can = \"posts.delete\"`",
            )
            .to_compile_error()
            .into();
        }
    }

    for lit in &middleware {
        let value = lit.value();
        if value.trim().is_empty() || value.contains(char::is_whitespace) {
            return syn::Error::new_spanned(
                lit,
                "a middleware name must be non-empty and without whitespace, e.g. \"auth\"",
            )
            .to_compile_error()
            .into();
        }
    }

    let func = parse_macro_input!(item as ItemFn);

    let vis = &func.vis;
    let sig = &func.sig;
    let fn_ident = &sig.ident;
    let fn_name = fn_ident.to_string();
    let block = &func.block;
    let asyncness = sig.asyncness;
    let output = &sig.output;
    let inputs = &sig.inputs;

    if let Some(FnArg::Receiver(recv)) = inputs.first() {
        return syn::Error::new_spanned(
            recv,
            "elyra #[command] cannot be applied to methods (`self`)",
        )
        .to_compile_error()
        .into();
    }

    // Collect argument idents/types, skipping the first parameter (the Ctx).
    let mut arg_names = Vec::new();
    let mut arg_types = Vec::new();
    for input in inputs.iter().skip(1) {
        match input {
            FnArg::Typed(pt) => match &*pt.pat {
                Pat::Ident(pi) => {
                    arg_names.push(pi.ident.clone());
                    arg_types.push((*pt.ty).clone());
                }
                other => {
                    return syn::Error::new_spanned(
                        other,
                        "elyra #[command] arguments must be simple identifiers",
                    )
                    .to_compile_error()
                    .into();
                }
            },
            FnArg::Receiver(recv) => {
                return syn::Error::new_spanned(recv, "unexpected `self` in command")
                    .to_compile_error()
                    .into();
            }
        }
    }

    let await_tok = asyncness.map(|_| quote!(.await)).unwrap_or_default();
    let inner_call = quote! { __elyra_inner(__ctx, #(#arg_names),*) #await_tok };

    // For codegen: argument name literals and the resolved return type.
    let arg_name_lits: Vec<String> = arg_names.iter().map(|i| i.to_string()).collect();

    // A `-> Result<T, E>` command surfaces `T` to codegen and maps `Err` to an
    // error response; any other type is serialized directly.
    let result_ok = result_ok_type(&sig.output);
    let codegen_ret_ty = match (&result_ok, &sig.output) {
        (Some(ok), _) => quote!(#ok),
        (None, syn::ReturnType::Type(_, ty)) => quote!(#ty),
        (None, syn::ReturnType::Default) => quote!(()),
    };

    // How to turn the handler's output into the response bytes.
    let encode_out = if result_ok.is_some() {
        quote! {
            match #inner_call {
                ::std::result::Result::Ok(__v) => {
                    let __bytes = ::elyra::__private::rmp::to_vec_named(&__v)
                        .map_err(::elyra::__private::Error::encode)?;
                    ::elyra::Result::Ok(__bytes)
                }
                ::std::result::Result::Err(__e) => {
                    ::elyra::Result::Err(::elyra::__private::Error::command(__e))
                }
            }
        }
    } else {
        quote! {
            // Structs serialize as named maps -> plain JS objects that survive
            // field reordering across Rust/TS versions.
            let __out = #inner_call;
            let __bytes = ::elyra::__private::rmp::to_vec_named(&__out)
                .map_err(::elyra::__private::Error::encode)?;
            ::elyra::Result::Ok(__bytes)
        }
    };

    // Decode the argument tuple with compact msgpack (JS side sends `encode([...])`).
    // Zero-arg commands ignore the request body entirely, sidestepping the
    // `()` -> nil vs `[]` -> empty-array msgpack mismatch.
    let decode = if arg_names.is_empty() {
        quote! {}
    } else {
        quote! {
            let ( #(#arg_names,)* ): ( #(#arg_types,)* ) =
                ::elyra::__private::rmp::from_slice(__args)
                    .map_err(::elyra::__private::Error::decode)?;
        }
    };

    // `#[command(can = "…")]` overrides the trait default of "no ability".
    let ability_impl = match &ability {
        Some(lit) => quote! {
            fn ability(&self) -> ::std::option::Option<&'static str> {
                ::std::option::Option::Some(#lit)
            }
        },
        None => quote! {},
    };
    let middleware_impl = if middleware.is_empty() {
        quote! {}
    } else {
        quote! {
            fn middleware(&self) -> &'static [&'static str] {
                &[ #( #middleware ),* ]
            }
        }
    };

    let expanded = quote! {
        #[allow(non_camel_case_types)]
        #[derive(::std::clone::Clone, ::std::marker::Copy)]
        #vis struct #fn_ident;

        impl ::elyra::command::Command for #fn_ident {
            fn name(&self) -> &'static str { #fn_name }

            #ability_impl
            #middleware_impl

            fn signature(
                &self,
                types: &mut ::specta::Types,
            ) -> ::elyra::command::CommandSig {
                ::elyra::command::CommandSig {
                    name: #fn_name,
                    args: ::std::vec![
                        #( (#arg_name_lits, <#arg_types as ::specta::Type>::definition(types)) ),*
                    ],
                    ret: <#codegen_ret_ty as ::specta::Type>::definition(types),
                }
            }

            fn call<'a>(
                &'a self,
                __ctx: ::elyra::Ctx,
                __args: &'a [u8],
            ) -> ::elyra::command::BoxFuture<'a, ::elyra::Result<::std::vec::Vec<u8>>> {
                // Original function body, preserved verbatim.
                #asyncness fn __elyra_inner(#inputs) #output #block

                ::std::boxed::Box::pin(async move {
                    #decode
                    #encode_out
                })
            }
        }
    };

    expanded.into()
}
