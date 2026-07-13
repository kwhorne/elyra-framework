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

/// Field metadata resolved from the struct + `#[model(..)]` attributes.
struct ModelField {
    ident: Ident,
    ty: Type,
    column: String,
    is_pk: bool,
    is_bool: bool,
}

enum RelKind {
    HasMany,
    HasOne,
    BelongsTo,
}

/// A relation declared via `#[model(has_many(Post, fk = "user_id", as = "posts"))]`.
struct Relation {
    kind: RelKind,
    ty: Ident,
    fk: Option<String>,
    name: Option<String>,
}

/// A relation declared on a *field* (e.g. `#[model(has_many(Book, fk = "author_id"))]
/// books: Vec<Book>`), whose rows are hydrated straight into that field.
struct FieldRelation {
    field: Ident,
    kind: RelKind,
    ty: Ident,
    fk: Option<String>,
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
///     created_at: i64,
///     updated_at: i64,
/// }
/// ```
///
/// Notes: `bool` fields map to an INTEGER `0/1` column (the `Any` driver can't
/// read SQLite's native `BOOLEAN` type). `timestamps` auto-manages `created_at`
/// / `updated_at` (unix seconds). v1 assumes an `i64` autoincrement primary key.
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

    // Struct-level: #[model(table = "..", timestamps, has_many(..), belongs_to(..), has_one(..))]
    let mut table = name.to_string().to_lowercase();
    let mut timestamps = false;
    let mut relations: Vec<Relation> = Vec::new();
    for attr in &input.attrs {
        if attr.path().is_ident("model") {
            let _ = attr.parse_nested_meta(|meta| {
                let kind = if meta.path.is_ident("has_many") {
                    Some(RelKind::HasMany)
                } else if meta.path.is_ident("has_one") {
                    Some(RelKind::HasOne)
                } else if meta.path.is_ident("belongs_to") {
                    Some(RelKind::BelongsTo)
                } else {
                    None
                };

                if let Some(kind) = kind {
                    let mut ty: Option<Ident> = None;
                    let mut fk = None;
                    let mut rel_name = None;
                    meta.parse_nested_meta(|inner| {
                        if inner.path.is_ident("fk") {
                            fk = Some(inner.value()?.parse::<LitStr>()?.value());
                        } else if inner.path.is_ident("as") {
                            rel_name = Some(inner.value()?.parse::<LitStr>()?.value());
                        } else if let Some(id) = inner.path.get_ident() {
                            ty = Some(id.clone());
                        }
                        Ok(())
                    })?;
                    if let Some(ty) = ty {
                        relations.push(Relation {
                            kind,
                            ty,
                            fk,
                            name: rel_name,
                        });
                    }
                } else if meta.path.is_ident("table") {
                    table = meta.value()?.parse::<LitStr>()?.value();
                } else if meta.path.is_ident("timestamps") {
                    timestamps = true;
                }
                Ok(())
            });
        }
    }

    // Per-field: #[model(id)] / #[model(column = "..")] / #[model(has_many(..))] etc.
    let mut infos: Vec<ModelField> = Vec::new();
    let mut field_relations: Vec<FieldRelation> = Vec::new();
    for field in &fields {
        let ident = field.ident.clone().unwrap();
        let mut column = ident.to_string();
        let mut is_pk = false;
        let mut rel: Option<(RelKind, Ident, Option<String>)> = None;
        for attr in &field.attrs {
            if attr.path().is_ident("model") {
                let _ = attr.parse_nested_meta(|meta| {
                    let kind = if meta.path.is_ident("has_many") {
                        Some(RelKind::HasMany)
                    } else if meta.path.is_ident("has_one") {
                        Some(RelKind::HasOne)
                    } else if meta.path.is_ident("belongs_to") {
                        Some(RelKind::BelongsTo)
                    } else {
                        None
                    };
                    if let Some(kind) = kind {
                        let mut ty: Option<Ident> = None;
                        let mut fk = None;
                        meta.parse_nested_meta(|inner| {
                            if inner.path.is_ident("fk") {
                                fk = Some(inner.value()?.parse::<LitStr>()?.value());
                            } else if let Some(id) = inner.path.get_ident() {
                                ty = Some(id.clone());
                            }
                            Ok(())
                        })?;
                        if let Some(ty) = ty {
                            rel = Some((kind, ty, fk));
                        }
                    } else if meta.path.is_ident("id") {
                        is_pk = true;
                    } else if meta.path.is_ident("column") {
                        column = meta.value()?.parse::<LitStr>()?.value();
                    }
                    Ok(())
                });
            }
        }
        if let Some((kind, ty, fk)) = rel {
            // A relation field is hydrated, not stored: skip it as a column.
            field_relations.push(FieldRelation {
                field: ident,
                kind,
                ty,
                fk,
            });
            continue;
        }
        infos.push(ModelField {
            is_bool: is_bool(&field.ty),
            ident,
            ty: field.ty.clone(),
            column,
            is_pk,
        });
    }

    // Primary key: flagged field, else one whose column is "id".
    if !infos.iter().any(|f| f.is_pk) {
        if let Some(f) = infos.iter_mut().find(|f| f.column == "id") {
            f.is_pk = true;
        }
    }
    let (pk_ident, pk_col, pk_is_bool) = match infos.iter().find(|f| f.is_pk) {
        Some(f) => (f.ident.clone(), f.column.clone(), f.is_bool),
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
    let col_str = all_cols.join(", ");

    let insert: Vec<&ModelField> = infos.iter().filter(|f| !f.is_pk).collect();
    let insert_cols: Vec<String> = insert.iter().map(|f| f.column.clone()).collect();
    let insert_cols_str = insert_cols.join(", ");
    let n_insert = insert.len();

    let mut from_row_fields: Vec<_> = infos
        .iter()
        .map(|f| {
            let ident = &f.ident;
            let col = &f.column;
            if f.is_bool {
                quote! { #ident: ::elyra::db::sqlx::Row::try_get::<i64, _>(__row, #col)? != 0 }
            } else {
                let ty = &f.ty;
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

    let bind_of = |f: &ModelField| {
        let ident = &f.ident;
        if f.is_bool {
            quote! { .bind(if self.#ident { 1i64 } else { 0i64 }) }
        } else {
            quote! { .bind(::std::clone::Clone::clone(&self.#ident)) }
        }
    };
    let insert_binds: Vec<_> = insert.iter().map(|f| bind_of(f)).collect();
    let pk_bind = if pk_is_bool {
        quote! { .bind(if self.#pk_ident { 1i64 } else { 0i64 }) }
    } else {
        quote! { .bind(::std::clone::Clone::clone(&self.#pk_ident)) }
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

    // Relation accessor methods.
    let self_lower = name.to_string().to_lowercase();
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
            }
        })
        .collect();

    // Field-relation hydrators: fill `self.<field>` from a batch of parents.
    let field_relation_methods: Vec<_> = field_relations
        .iter()
        .map(|rel| {
            let field = &rel.field;
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
            }
        })
        .collect();

    let expanded = quote! {
        impl ::elyra::db::model::Model for #name {
            const TABLE: &'static str = #table;
            const PK: &'static str = #pk_col;
            const COLUMNS: &'static [&'static str] = &[ #(#all_cols),* ];

            fn from_row(__row: &::elyra::db::sqlx::any::AnyRow) -> ::elyra::db::Result<Self> {
                ::std::result::Result::Ok(Self { #( #from_row_fields ),* })
            }

            fn get_i64(&self, __column: &str) -> ::std::option::Option<i64> {
                match __column {
                    #( #i64_arms )*
                    _ => ::std::option::Option::None,
                }
            }
        }

        impl #name {
            /// All rows.
            pub async fn all(__db: &::elyra::db::Database) -> ::elyra::db::Result<::std::vec::Vec<Self>> {
                let __sql = ::std::format!("SELECT {} FROM {}", #col_str, #table);
                let __rows = ::elyra::db::sqlx::query(::elyra::db::sqlx::AssertSqlSafe(__sql)).fetch_all(__db.pool()).await?;
                __rows.iter().map(<Self as ::elyra::db::model::Model>::from_row).collect()
            }

            /// Find one row by primary key.
            pub async fn find(__db: &::elyra::db::Database, __id: i64) -> ::elyra::db::Result<::std::option::Option<Self>> {
                let __sql = ::std::format!(
                    "SELECT {} FROM {} WHERE {} = {}",
                    #col_str, #table, #pk_col, ::elyra::db::model::placeholder(__db.driver(), 1)
                );
                let __row = ::elyra::db::sqlx::query(::elyra::db::sqlx::AssertSqlSafe(__sql)).bind(__id).fetch_optional(__db.pool()).await?;
                match __row {
                    ::std::option::Option::Some(__r) =>
                        ::std::result::Result::Ok(::std::option::Option::Some(
                            <Self as ::elyra::db::model::Model>::from_row(&__r)?)),
                    ::std::option::Option::None =>
                        ::std::result::Result::Ok(::std::option::Option::None),
                }
            }

            /// Start a typed query.
            pub fn query() -> ::elyra::db::model::Query<Self> {
                ::elyra::db::model::Query::new()
            }

            /// Insert as a new row, setting the primary key from the database.
            pub async fn insert(&mut self, __db: &::elyra::db::Database) -> ::elyra::db::Result<()> {
                #insert_ts
                let __phs = ::elyra::db::model::placeholders(__db.driver(), #n_insert);
                match __db.driver() {
                    ::elyra::db::Driver::MySql => {
                        let __sql = ::std::format!("INSERT INTO {} ({}) VALUES ({})", #table, #insert_cols_str, __phs);
                        let __res = ::elyra::db::sqlx::query(::elyra::db::sqlx::AssertSqlSafe(__sql))
                            #( #insert_binds )*
                            .execute(__db.pool()).await?;
                        if let ::std::option::Option::Some(__id) = __res.last_insert_id() {
                            self.#pk_ident = __id;
                        }
                    }
                    _ => {
                        let __sql = ::std::format!(
                            "INSERT INTO {} ({}) VALUES ({}) RETURNING {}",
                            #table, #insert_cols_str, __phs, #pk_col
                        );
                        let __row = ::elyra::db::sqlx::query(::elyra::db::sqlx::AssertSqlSafe(__sql))
                            #( #insert_binds )*
                            .fetch_one(__db.pool()).await?;
                        self.#pk_ident = ::elyra::db::sqlx::Row::try_get::<i64, _>(&__row, #pk_col)?;
                    }
                }
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
                ::elyra::db::sqlx::query(::elyra::db::sqlx::AssertSqlSafe(__sql))
                    #( #insert_binds )*
                    #pk_bind
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

            /// Insert if unsaved (`id == 0`), otherwise update.
            pub async fn save(&mut self, __db: &::elyra::db::Database) -> ::elyra::db::Result<()> {
                if self.#pk_ident == 0 {
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
pub fn command(_attr: TokenStream, item: TokenStream) -> TokenStream {
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

    let expanded = quote! {
        #[allow(non_camel_case_types)]
        #[derive(::std::clone::Clone, ::std::marker::Copy)]
        #vis struct #fn_ident;

        impl ::elyra::command::Command for #fn_ident {
            fn name(&self) -> &'static str { #fn_name }

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
