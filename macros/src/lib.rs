//! Proc macros for the Elyra desktop framework: `#[command]` and `#[derive(Model)]`.

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::spanned::Spanned;
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

/// `Option<T>`'s `T`: a nullable column's value type. `None` for any other type.
fn option_inner(ty: &Type) -> Option<&Type> {
    let Type::Path(p) = ty else { return None };
    let last = p.path.segments.last()?;
    if last.ident != "Option" {
        return None;
    }
    match &last.arguments {
        syn::PathArguments::AngleBracketed(args) => match args.args.first() {
            Some(syn::GenericArgument::Type(inner)) => Some(inner),
            _ => None,
        },
        _ => None,
    }
}

/// `Option<bool>` — a nullable INTEGER `0/1` column.
fn is_opt_bool(ty: &Type) -> bool {
    let Type::Path(p) = ty else { return false };
    let Some(last) = p.path.segments.last() else {
        return false;
    };
    if last.ident != "Option" {
        return false;
    }
    match &last.arguments {
        syn::PathArguments::AngleBracketed(args) => {
            matches!(args.args.first(), Some(syn::GenericArgument::Type(inner)) if is_bool(inner))
        }
        _ => false,
    }
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
    /// The foreign key's columns: one, or a tuple for a composite parent
    /// key, in that key's order (RFC 0047).
    fk: Option<Vec<String>>,
    /// `as = ".."`: the method name. Struct-level relations only — a field
    /// relation is named by its field.
    name: Option<String>,
    /// `belongs_to_many` only: the join table, and its key to the related side.
    pivot: Option<String>,
    related_fk: Option<String>,
    /// `belongs_to_many` only: `using = T`, the pivot's own model (RFC 0046).
    using: Option<syn::Path>,
}

/// A relation declared on a field, hydrated straight into it.
struct FieldRelation {
    field: Ident,
    rel: Relation,
    /// The field holds `Vec<(R, T)>`: each related row with its pivot row.
    pairs: bool,
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
    let mut using: Option<syn::Path> = None;
    meta.parse_nested_meta(|inner| {
        if inner.path.is_ident("fk") {
            let value = inner.value()?;
            if value.peek(syn::token::Paren) {
                let content;
                syn::parenthesized!(content in value);
                let lits =
                    syn::punctuated::Punctuated::<LitStr, syn::Token![,]>::parse_terminated(&content)?;
                let columns = lits
                    .into_iter()
                    .map(lit_ident)
                    .collect::<syn::Result<Vec<_>>>()?;
                if columns.len() < 2 {
                    return Err(inner.error(
                        "a tuple `fk` names two or more columns; write `fk = \"col\"` for one",
                    ));
                }
                if kind == RelKind::BelongsToMany {
                    return Err(inner.error(
                        "`belongs_to_many` keys on one `i64` column: its `fk` is one column",
                    ));
                }
                fk = Some(columns);
            } else {
                fk = Some(vec![lit_ident(value.parse()?)?]);
            }
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
        } else if inner.path.is_ident("using") {
            if kind != RelKind::BelongsToMany {
                return Err(inner.error("`using` only applies to `belongs_to_many`"));
            }
            using = Some(inner.value()?.parse()?);
        } else if let Some(id) = inner.path.get_ident() {
            if ty.is_some() {
                return Err(inner.error("a relation names exactly one related model"));
            }
            ty = Some(id.clone());
        } else {
            return Err(inner.error(
                "expected the related model, `fk`, `as`, `pivot`, `related_fk` or `using`",
            ));
        }
        Ok(())
    })?;
    let ty =
        ty.ok_or_else(|| meta.error("a relation needs the related model, e.g. `has_many(Post)`"))?;
    if pivot.is_some() && using.is_some() {
        return Err(
            meta.error("`pivot` and `using` together: with `using = T`, the pivot table is `T`'s")
        );
    }
    Ok(Relation {
        kind,
        ty,
        fk,
        name,
        pivot,
        related_fk,
        using,
    })
}

/// `Vec<(A, B)>`: a `belongs_to_many` field holding each related row with its
/// pivot row.
fn is_vec_of_pairs(ty: &Type) -> bool {
    let Type::Path(p) = ty else { return false };
    let Some(last) = p.path.segments.last() else {
        return false;
    };
    if last.ident != "Vec" {
        return false;
    }
    let syn::PathArguments::AngleBracketed(args) = &last.arguments else {
        return false;
    };
    matches!(
        args.args.first(),
        Some(syn::GenericArgument::Type(Type::Tuple(t))) if t.elems.len() == 2
    )
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
/// Notes: `bool` (and `Option<bool>`) fields map to an INTEGER `0/1` column (the `Any` driver can't
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
            let pairs = is_vec_of_pairs(&field.ty);
            if pairs && rel.using.is_none() {
                return syn::Error::new_spanned(
                    &field.ty,
                    "a `Vec<(R, T)>` relation field needs `belongs_to_many(R, using = T)`",
                )
                .to_compile_error()
                .into();
            }
            // A relation field is hydrated, not stored: skip it as a column.
            field_relations.push(FieldRelation {
                field: ident,
                rel,
                pairs,
            });
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
    let (pk_ident, pk_col, pk_ty) = match infos.iter().find(|f| f.is_pk) {
        Some(f) => (f.ident.clone(), f.column.clone(), f.ty.clone()),
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

    // The key: one field, or more than one marked `#[model(id)]` (RFC 0044),
    // in declaration order.
    let keys: Vec<&ModelField> = infos.iter().filter(|f| f.is_pk).collect();
    let composite = keys.len() > 1;
    if keys.len() > 4 {
        return syn::Error::new_spanned(&input, "a composite key has at most four columns")
            .to_compile_error()
            .into();
    }
    let key_cols: Vec<String> = keys.iter().map(|f| f.column.clone()).collect();
    let key_ty = if composite {
        let tys = keys.iter().map(|f| &f.ty);
        quote! { ( #(#tys),* ) }
    } else {
        quote! { #pk_ty }
    };

    // `i64` is the default auto-increment key; any other type, and every
    // composite key, is app-supplied.
    let pk_is_i64 = !composite && is_i64(&pk_ty);

    let has_many_to_many = relations
        .iter()
        .chain(field_relations.iter().map(|f| &f.rel))
        .any(|r| r.kind == RelKind::BelongsToMany);
    if has_many_to_many && !pk_is_i64 {
        return syn::Error::new_spanned(
            &input,
            format!(
                "`belongs_to_many` needs an `i64` key on both sides (the pivot holds \
                 integers); this model's key is `{}`",
                key_ty.to_string().replace(" ", "")
            ),
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
            } else if is_opt_bool(ty) {
                quote! {
                    #ident: ::elyra::db::sqlx::Row::try_get::<::std::option::Option<i64>, _>(__row, #col)?
                        .map(|__v| __v != 0)
                }
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
            } else if is_opt_bool(&f.ty) {
                Some(quote! { #col => self.#ident.map(|__b| __b as i64), })
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
        } else if is_opt_bool(&f.ty) {
            quote! {
                ::elyra::db::model::bind_arg(&mut __args, self.#ident.map(|__b| if __b { 1i64 } else { 0i64 }))?;
            }
        } else {
            quote! { ::elyra::db::model::bind_arg(&mut __args, ::std::clone::Clone::clone(&self.#ident))?; }
        }
    };
    let update_args: Vec<_> = insert.iter().map(|f| arg_of(f)).collect();
    let key_args: Vec<_> = keys.iter().map(|f| arg_of(f)).collect();

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
    let create_col_names: Vec<String> = create.iter().map(|f| f.column.clone()).collect();
    let create_args: Vec<_> = create.iter().map(|f| arg_of(f)).collect();

    // Cast columns may need converting in SQL — read through `Cast::select`,
    // written through `Cast::bind` (a JSON column on Postgres, say).
    let cast_items = {
        let casts: Vec<&ModelField> = infos.iter().filter(|f| f.cast.is_some()).collect();
        if casts.is_empty() {
            quote! {}
        } else {
            let select_arms = casts.iter().map(|f| {
                let col = &f.column;
                let cast = f.cast.as_ref().expect("a cast");
                let ty = &f.ty;
                quote! {
                    #col => ::std::option::Option::Some(
                        <#cast as ::elyra::db::cast::Cast<#ty>>::select(
                            __driver,
                            &::std::format!("{}.{}", #table, #col),
                            #col,
                        ),
                    ),
                }
            });
            let bind_arms = casts.iter().map(|f| {
                let col = &f.column;
                let cast = f.cast.as_ref().expect("a cast");
                let ty = &f.ty;
                quote! {
                    #col => <#cast as ::elyra::db::cast::Cast<#ty>>::bind(__driver, &__placeholder),
                }
            });
            quote! {
                fn select_column(
                    __driver: ::elyra::db::Driver,
                    __column: &str,
                ) -> ::std::option::Option<::std::string::String> {
                    match __column {
                        #( #select_arms )*
                        _ => ::std::option::Option::None,
                    }
                }

                fn bind_column(
                    __driver: ::elyra::db::Driver,
                    __column: &str,
                    __placeholder: ::std::string::String,
                ) -> ::std::string::String {
                    match __column {
                        #( #bind_arms )*
                        _ => __placeholder,
                    }
                }
            }
        }
    };

    // The insert body differs by key strategy: an i64 PK is read back from the
    // database (RETURNING / last_insert_id); any other PK is supplied by the app.
    let insert_body = if pk_is_i64 {
        quote! {
            let __phs = ::elyra::db::model::placeholders_for::<Self>(__db.driver(), &[ #(#create_col_names),* ]);
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
            let __phs = ::elyra::db::model::placeholders_for::<Self>(__db.driver(), &[ #(#create_col_names),* ]);
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

    // A composite key has no "unsaved" value: `save()` is an upsert on the key
    // (RFC 0044). `created_at` is set only when it isn't yet, and never
    // overwritten in the row; `updated_at` always.
    let save_body = if composite {
        let mut ts = quote! {};
        if created.is_some() || updated.is_some() {
            ts = quote! { let __now = #now_expr; };
        }
        if let Some(i) = &created {
            ts = quote! { #ts if self.#i == 0 { self.#i = __now; } };
        }
        if let Some(i) = &updated {
            ts = quote! { #ts self.#i = __now; };
        }
        let set_cols: Vec<String> = insert
            .iter()
            .filter(|f| f.column != "created_at" || !timestamps)
            .map(|f| f.column.clone())
            .collect();
        let conflict = key_cols.join(", ");
        let first_key = key_cols[0].clone();
        quote! {
            __db.before_write(#table)?;
            #ts
            let __phs = ::elyra::db::model::placeholders_for::<Self>(__db.driver(), &[ #(#create_col_names),* ]);
            let mut __args = ::elyra::db::sqlx::any::AnyArguments::default();
            #( #create_args )*
            let __set: &[&str] = &[ #(#set_cols),* ];
            let __sql = match __db.driver() {
                ::elyra::db::Driver::MySql => {
                    let __update = if __set.is_empty() {
                        ::std::format!("{0} = {0}", #first_key)
                    } else {
                        __set.iter().map(|__c| ::std::format!("{0} = VALUES({0})", __c)).collect::<::std::vec::Vec<_>>().join(", ")
                    };
                    ::std::format!(
                        "INSERT INTO {} ({}) VALUES ({}) ON DUPLICATE KEY UPDATE {}",
                        #table, #create_cols_str, __phs, __update
                    )
                }
                _ => {
                    let __update = if __set.is_empty() {
                        ::std::string::String::from("NOTHING")
                    } else {
                        ::std::format!(
                            "UPDATE SET {}",
                            __set.iter().map(|__c| ::std::format!("{0} = excluded.{0}", __c)).collect::<::std::vec::Vec<_>>().join(", ")
                        )
                    };
                    ::std::format!(
                        "INSERT INTO {} ({}) VALUES ({}) ON CONFLICT ({}) DO {}",
                        #table, #create_cols_str, __phs, #conflict, __update
                    )
                }
            };
            ::elyra::db::sqlx::query_with(::elyra::db::sqlx::AssertSqlSafe(__sql), __args)
                .execute(__db.pool()).await?;
            __db.wrote(#table);
            ::std::result::Result::Ok(())
        }
    } else {
        quote! {
            if #save_is_new {
                self.insert(__db).await
            } else {
                self.update(__db).await
            }
        }
    };

    let self_lower = name.to_string().to_lowercase();

    // The row's key, as `Model::key` returns it.
    let key_idents: Vec<&Ident> = keys.iter().map(|f| &f.ident).collect();
    let key_expr = if composite {
        quote! { ( #( ::std::clone::Clone::clone(&self.#key_idents) ),* ) }
    } else {
        quote! { ::std::clone::Clone::clone(&self.#pk_ident) }
    };

    // Foreign keys on any key (RFC 0047): a `has_*` names the child's columns
    // for this model's key, in its order; a `belongs_to` names this model's
    // own columns for the owner's key.
    let child_fks = |rel: &Relation| -> Vec<String> {
        rel.fk
            .clone()
            .unwrap_or_else(|| vec![format!("{self_lower}_id")])
    };
    let owner_fks = |rel: &Relation| -> Vec<String> {
        rel.fk
            .clone()
            .unwrap_or_else(|| vec![format!("{}_id", rel.ty.to_string().to_lowercase())])
    };
    for rel in relations
        .iter()
        .chain(field_relations.iter().map(|f| &f.rel))
    {
        let problem =
            match rel.kind {
                RelKind::HasMany | RelKind::HasOne => {
                    match &rel.fk {
                        None if composite => Some(format!(
                    "`{}` from a composite key needs `fk = (..)`: the `{}` columns that hold \
                     ({})",
                    if rel.kind == RelKind::HasMany { "has_many" } else { "has_one" },
                    rel.ty,
                    key_cols.join(", ")
                )),
                        Some(fk) if fk.len() != keys.len() => Some(format!(
                            "`fk` names {} column(s), and this model's key has {} ({})",
                            fk.len(),
                            keys.len(),
                            key_cols.join(", ")
                        )),
                        _ => None,
                    }
                }
                RelKind::BelongsTo => owner_fks(rel)
                    .iter()
                    .find(|c| !infos.iter().any(|f| &f.column == *c))
                    .map(|c| {
                        format!(
                        "`belongs_to({})`: `{c}` isn't a column of this model; `fk` names this \
                         model's columns that hold the owner's key",
                        rel.ty
                    )
                    }),
                RelKind::BelongsToMany => None,
            };
        if let Some(problem) = problem {
            return syn::Error::new_spanned(&rel.ty, problem)
                .to_compile_error()
                .into();
        }
    }

    // `belongs_to`: the owner's key from this model's fk fields, `None` when a
    // nullable one is `NULL`. `FromForeignKey` is implemented only for an fk
    // as wide as the owner's key, so a mismatch is an error that says so.
    let owner_key_fn = |rel: &Relation, fn_name: &Ident| {
        let ty = &rel.ty;
        let fks = owner_fks(rel);
        let width = fks.len();
        let mut parts = Vec::new();
        let mut types = Vec::new();
        for (i, c) in fks.iter().enumerate() {
            let f = infos
                .iter()
                .find(|f| &f.column == c)
                .expect("checked above");
            let ident = &f.ident;
            let var = format_ident!("__k{}", i);
            match option_inner(&f.ty) {
                Some(inner) => {
                    parts.push(quote! { let #var = ::std::clone::Clone::clone(&self.#ident)?; });
                    types.push(inner.clone());
                }
                None => {
                    parts.push(quote! { let #var = ::std::clone::Clone::clone(&self.#ident); });
                    types.push(f.ty.clone());
                }
            }
        }
        let vars: Vec<Ident> = (0..width).map(|i| format_ident!("__k{}", i)).collect();
        quote! {
            #[doc(hidden)]
            fn #fn_name(&self) -> ::std::option::Option<<#ty as ::elyra::db::model::Model>::Key> {
                #( #parts )*
                ::std::option::Option::Some(<<#ty as ::elyra::db::model::Model>::Key as ::elyra::db::model::FromForeignKey<( #( #types, )* )>>::from_foreign_key(( #( #vars, )* )))
            }
        }
    };

    // belongs_to_many: the pivot (defaults follow Laravel — the two model names
    // in alphabetical order, `<model>_id` keys) and its write methods, shared by
    // struct- and field-level declarations.
    let pivot_of = |rel: &Relation| {
        let ty_lower = rel.ty.to_string().to_lowercase();
        let table = match &rel.using {
            // `using = T`: the pivot is `T`'s table (RFC 0046).
            Some(using) => quote! { <#using as ::elyra::db::model::Model>::TABLE },
            None => {
                let table = rel.pivot.clone().unwrap_or_else(|| {
                    let mut pair = [self_lower.clone(), ty_lower.clone()];
                    pair.sort();
                    pair.join("_")
                });
                quote! { #table }
            }
        };
        let fk = rel
            .fk
            .as_ref()
            .map(|v| v[0].clone())
            .unwrap_or_else(|| format!("{self_lower}_id"));
        let related_fk = rel
            .related_fk
            .clone()
            .unwrap_or_else(|| format!("{ty_lower}_id"));
        quote! {
            ::elyra::db::model::Pivot { table: #table, parent_fk: #fk, related_fk: #related_fk }
        }
    };
    let pivot_methods = |suffix: &str,
                         ty: &Ident,
                         pivot: &proc_macro2::TokenStream,
                         using: Option<&syn::Path>| {
        let query = format_ident!("{}_query", suffix);
        let attach = format_ident!("attach_{}", suffix);
        let detach = format_ident!("detach_{}", suffix);
        let detach_all = format_ident!("detach_all_{}", suffix);
        let sync = format_ident!("sync_{}", suffix);
        let sync_wd = format_ident!("sync_{}_without_detaching", suffix);
        // The pivot holds integers: the related model's key is `i64`, said at
        // the related type.
        let needs_i64 = quote::quote_spanned! {ty.span()=>
            fn __elyra_belongs_to_many_needs_an_i64_key<__T: ::elyra::db::model::Model<Key = i64>>() {}
            let _ = __elyra_belongs_to_many_needs_an_i64_key::<#ty>;
        };
        // With `using = T`, attaching by ids goes through `T::default()`, so
        // the pivot's timestamps and defaults are filled (RFC 0046).
        let (attach_body, sync_body, sync_wd_body) = match using {
            Some(using) => (
                {
                    // `using` must be `Default` for the id-only writers: said at
                    // the `using`, rather than deep in a generated body.
                    let needs = quote::quote_spanned! {using.span()=>
                        fn __elyra_using_model_must_implement_default<__T: ::std::default::Default + ::elyra::db::model::PivotRow>() {}
                        let _ = __elyra_using_model_must_implement_default::<#using>;
                    };
                    quote! {
                        #needs
                        #pivot.attach_defaults::<#using>(__db, self.#pk_ident, __ids).await
                    }
                },
                quote! { #pivot.sync_defaults::<#using>(__db, self.#pk_ident, __ids, true).await },
                quote! { #pivot.sync_defaults::<#using>(__db, self.#pk_ident, __ids, false).await },
            ),
            None => (
                quote! { #pivot.attach(__db, self.#pk_ident, __ids).await },
                quote! { #pivot.sync(__db, self.#pk_ident, __ids).await },
                quote! { #pivot.sync_without_detaching(__db, self.#pk_ident, __ids).await },
            ),
        };
        let with_pivot = using.map(|using| {
            let attach_with = format_ident!("attach_{}_with", suffix);
            let sync_with = format_ident!("sync_{}_with", suffix);
            let one = suffix.strip_suffix('s').filter(|s| !s.is_empty()).unwrap_or(suffix);
            let update = format_ident!("update_{}_pivot", one);
            quote! {
                /// Attach related ids with their pivot rows, in one transaction; each row's
                /// two keys are set from the relation (belongs_to_many, `using`).
                pub async fn #attach_with(&self, __db: &::elyra::db::Database, __rows: impl ::std::iter::IntoIterator<Item = (i64, #using)>) -> ::elyra::db::Result<u64> {
                    #pivot.attach_rows(__db, self.#pk_ident, __rows).await
                }
                /// Make exactly these attached with this pivot data, in one transaction:
                /// detach the rest, attach the missing, update the attached rows whose
                /// data differ (belongs_to_many, `using`).
                pub async fn #sync_with(&self, __db: &::elyra::db::Database, __rows: impl ::std::iter::IntoIterator<Item = (i64, #using)>) -> ::elyra::db::Result<::elyra::db::model::SyncChanges> {
                    #pivot.sync_rows(__db, self.#pk_ident, __rows).await
                }
                /// Change the pivot row for one related id; `false` when it isn't
                /// attached (belongs_to_many, `using`).
                pub async fn #update(&self, __db: &::elyra::db::Database, __id: i64, __change: impl ::std::ops::FnOnce(&mut #using)) -> ::elyra::db::Result<bool> {
                    #pivot.update_row(__db, self.#pk_ident, __id, __change).await
                }
            }
        });
        quote! {
            /// The related rows as a query to keep narrowing (belongs_to_many).
            pub fn #query(&self) -> ::elyra::db::model::Query<#ty> {
                #needs_i64
                #pivot.query::<#ty>(self.#pk_ident)
            }
            /// Attach related ids through the pivot (belongs_to_many).
            pub async fn #attach(&self, __db: &::elyra::db::Database, __ids: impl ::std::iter::IntoIterator<Item = i64>) -> ::elyra::db::Result<u64> {
                #attach_body
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
                #sync_body
            }
            /// Attach whichever of these ids are missing; detach nothing (belongs_to_many).
            pub async fn #sync_wd(&self, __db: &::elyra::db::Database, __ids: impl ::std::iter::IntoIterator<Item = i64>) -> ::elyra::db::Result<::elyra::db::model::SyncChanges> {
                #sync_wd_body
            }
            #with_pivot
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
                    let fks = child_fks(rel);
                    let mname = format_ident!("{}", rel.name.clone().unwrap_or_else(|| format!("{ty_lower}s")));
                    let load = format_ident!("load_{}", mname);
                    quote! {
                        /// Related rows (has_many).
                        pub async fn #mname(&self, __db: &::elyra::db::Database) -> ::elyra::db::Result<::std::vec::Vec<#ty>> {
                            #ty::query() #( .where_eq(#fks, ::std::clone::Clone::clone(&self.#key_idents)) )* .get(__db).await
                        }
                        /// Eager-load this relation for a batch of parents, keyed by their key.
                        pub async fn #load(__db: &::elyra::db::Database, __parents: &[Self]) -> ::elyra::db::Result<::std::collections::HashMap<#key_ty, ::std::vec::Vec<#ty>>> {
                            ::elyra::db::model::eager_many::<Self, #ty>(__db, __parents, &[ #(#fks),* ]).await
                        }
                    }
                }
                RelKind::HasOne => {
                    let fks = child_fks(rel);
                    let mname = format_ident!("{}", rel.name.clone().unwrap_or_else(|| ty_lower.clone()));
                    let load = format_ident!("load_{}", mname);
                    quote! {
                        /// Related row (has_one).
                        pub async fn #mname(&self, __db: &::elyra::db::Database) -> ::elyra::db::Result<::std::option::Option<#ty>> {
                            #ty::query() #( .where_eq(#fks, ::std::clone::Clone::clone(&self.#key_idents)) )* .first(__db).await
                        }
                        /// Eager-load this relation for a batch of parents, keyed by their key.
                        pub async fn #load(__db: &::elyra::db::Database, __parents: &[Self]) -> ::elyra::db::Result<::std::collections::HashMap<#key_ty, #ty>> {
                            ::elyra::db::model::eager_one::<Self, #ty>(__db, __parents, &[ #(#fks),* ]).await
                        }
                    }
                }
                RelKind::BelongsTo => {
                    let mname = format_ident!("{}", rel.name.clone().unwrap_or_else(|| ty_lower.clone()));
                    let load = format_ident!("load_{}", mname);
                    let owner_key = format_ident!("__elyra_owner_key_{}", mname);
                    let owner_key_fn = owner_key_fn(rel, &owner_key);
                    quote! {
                        #owner_key_fn
                        /// Owning row (belongs_to): the owner whose key this row's
                        /// foreign key holds; `None` when a nullable one is `NULL`.
                        pub async fn #mname(&self, __db: &::elyra::db::Database) -> ::elyra::db::Result<::std::option::Option<#ty>> {
                            match self.#owner_key() {
                                ::std::option::Option::Some(__key) => #ty::query().find(__db, __key).await,
                                ::std::option::Option::None => ::std::result::Result::Ok(::std::option::Option::None),
                            }
                        }
                        /// Eager-load owners for a batch of children, keyed by the owner's key.
                        pub async fn #load(__db: &::elyra::db::Database, __children: &[Self]) -> ::elyra::db::Result<::std::collections::HashMap<<#ty as ::elyra::db::model::Model>::Key, #ty>> {
                            ::elyra::db::model::eager_owners::<#ty>(__db, __children.iter().filter_map(Self::#owner_key)).await
                        }
                    }
                }
                RelKind::BelongsToMany => {
                    let method = rel.name.clone().unwrap_or_else(|| format!("{ty_lower}s"));
                    let mname = format_ident!("{}", method);
                    let load = format_ident!("load_{}", method);
                    let pivot = pivot_of(rel);
                    let writes = pivot_methods(&method, ty, &pivot, rel.using.as_ref());
                    let reads = rel.using.as_ref().map(|using| {
                        let with_pivot = format_ident!("{}_with_pivot", method);
                        let load_with_pivot = format_ident!("load_{}_with_pivot", method);
                        quote! {
                            /// Related rows, each with its pivot row (belongs_to_many, `using`).
                            pub async fn #with_pivot(&self, __db: &::elyra::db::Database) -> ::elyra::db::Result<::std::vec::Vec<(#ty, #using)>> {
                                #pivot.with_pivot::<#ty, #using>(__db, self.#pk_ident).await
                            }
                            /// Eager-load the related rows with their pivot rows for a batch of
                            /// parents, keyed by primary key (two queries).
                            pub async fn #load_with_pivot(__db: &::elyra::db::Database, __parents: &[Self]) -> ::elyra::db::Result<::std::collections::HashMap<i64, ::std::vec::Vec<(#ty, #using)>>> {
                                #pivot.eager_with_pivot::<Self, #ty, #using>(__db, __parents, #pk_col).await
                            }
                        }
                    });
                    quote! {
                        /// Related rows through the pivot table (belongs_to_many).
                        pub async fn #mname(&self, __db: &::elyra::db::Database) -> ::elyra::db::Result<::std::vec::Vec<#ty>> {
                            #pivot.query::<#ty>(self.#pk_ident).get(__db).await
                        }
                        /// Eager-load this relation for a batch of parents in one query, keyed by primary key.
                        pub async fn #load(__db: &::elyra::db::Database, __parents: &[Self]) -> ::elyra::db::Result<::std::collections::HashMap<i64, ::std::vec::Vec<#ty>>> {
                            #pivot.eager::<Self, #ty>(__db, __parents, #pk_col).await
                        }
                        #reads
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
            match rel.kind {
                RelKind::HasMany => {
                    let fks = child_fks(rel);
                    quote! {
                        /// Eager-load this `has_many` relation into `self` for a batch of parents (one query).
                        pub async fn #with(__db: &::elyra::db::Database, __parents: &mut [Self]) -> ::elyra::db::Result<()> {
                            let mut __map = ::elyra::db::model::eager_many::<Self, #ty>(__db, __parents, &[ #(#fks),* ]).await?;
                            for __p in __parents.iter_mut() {
                                __p.#field = __map.remove(&<Self as ::elyra::db::model::Model>::key(__p)).unwrap_or_default();
                            }
                            ::std::result::Result::Ok(())
                        }
                    }
                }
                RelKind::HasOne => {
                    let fks = child_fks(rel);
                    quote! {
                        /// Eager-load this `has_one` relation into `self` for a batch of parents (one query).
                        pub async fn #with(__db: &::elyra::db::Database, __parents: &mut [Self]) -> ::elyra::db::Result<()> {
                            let mut __map = ::elyra::db::model::eager_one::<Self, #ty>(__db, __parents, &[ #(#fks),* ]).await?;
                            for __p in __parents.iter_mut() {
                                __p.#field = __map.remove(&<Self as ::elyra::db::model::Model>::key(__p));
                            }
                            ::std::result::Result::Ok(())
                        }
                    }
                }
                RelKind::BelongsTo => {
                    let owner_key = format_ident!("__elyra_owner_key_{}", field);
                    let owner_key_fn = owner_key_fn(rel, &owner_key);
                    quote! {
                        #owner_key_fn
                        /// Eager-load this `belongs_to` relation into `self` for a batch of children
                        /// (one query; requires the related type to be `Clone`).
                        pub async fn #with(__db: &::elyra::db::Database, __children: &mut [Self]) -> ::elyra::db::Result<()> {
                            let __map = ::elyra::db::model::eager_owners::<#ty>(__db, __children.iter().filter_map(Self::#owner_key)).await?;
                            for __c in __children.iter_mut() {
                                __c.#field = __c.#owner_key().and_then(|__k| __map.get(&__k).cloned());
                            }
                            ::std::result::Result::Ok(())
                        }
                    }
                }
                RelKind::BelongsToMany => {
                    let pivot = pivot_of(rel);
                    let writes = pivot_methods(&field.to_string(), ty, &pivot, rel.using.as_ref());
                    let eager = match (&rel.using, fr.pairs) {
                        (Some(using), true) => quote! { #pivot.eager_with_pivot::<Self, #ty, #using>(__db, __parents, #pk_col).await? },
                        _ => quote! { #pivot.eager::<Self, #ty>(__db, __parents, #pk_col).await? },
                    };
                    quote! {
                        /// Eager-load this `belongs_to_many` relation into `self` for a batch of
                        /// parents (through the pivot; with its rows for a `Vec<(R, T)>` field).
                        pub async fn #with(__db: &::elyra::db::Database, __parents: &mut [Self]) -> ::elyra::db::Result<()> {
                            let mut __map = #eager;
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

    // A composite-key model can be a `belongs_to_many` pivot's own model
    // (RFC 0046): written on the relation's connection, inside its
    // transaction, and updated only when its data differ.
    let pivot_row_impl = if composite {
        let set_arms = infos
            .iter()
            .filter(|f| f.cast.is_none() && is_i64(&f.ty))
            .map(|f| {
                let ident = &f.ident;
                let col = &f.column;
                quote! { #col => { self.#ident = __value; true } }
            });
        // Compared and set: the columns that aren't the key or the timestamps.
        let data: Vec<&&ModelField> = insert
            .iter()
            .filter(|f| !(timestamps && (f.column == "created_at" || f.column == "updated_at")))
            .collect();
        let data_cols: Vec<String> = data.iter().map(|f| f.column.clone()).collect();
        let data_args: Vec<_> = data.iter().map(|f| arg_of(f)).collect();
        let (touch_col, touch_arg, touch_set) = match &updated {
            Some(i) => (
                quote! { ::std::option::Option::Some("updated_at") },
                quote! { ::elyra::db::model::bind_arg(&mut __args, __now)?; },
                quote! { self.#i = __now; },
            ),
            None => (
                quote! { ::std::option::Option::<&str>::None },
                quote! {},
                quote! {},
            ),
        };
        quote! {
            impl ::elyra::db::model::PivotRow for #name {
                fn set_i64(&mut self, __column: &str, __value: i64) -> bool {
                    match __column {
                        #( #set_arms )*
                        _ => false,
                    }
                }

                fn insert_on<'__a>(
                    &'__a mut self,
                    __conn: &'__a mut ::elyra::db::sqlx::AnyConnection,
                    __driver: ::elyra::db::Driver,
                ) -> impl ::std::future::Future<Output = ::elyra::db::Result<()>> + ::std::marker::Send + '__a {
                    async move {
                        #insert_ts
                        let __phs = ::elyra::db::model::placeholders_for::<Self>(__driver, &[ #(#create_col_names),* ]);
                        let mut __args = ::elyra::db::sqlx::any::AnyArguments::default();
                        #( #create_args )*
                        let __sql = ::std::format!("INSERT INTO {} ({}) VALUES ({})", #table, #create_cols_str, __phs);
                        ::elyra::db::sqlx::query_with(::elyra::db::sqlx::AssertSqlSafe(__sql), __args)
                            .execute(&mut *__conn).await?;
                        ::std::result::Result::Ok(())
                    }
                }

                fn update_changed_on<'__a>(
                    &'__a mut self,
                    __conn: &'__a mut ::elyra::db::sqlx::AnyConnection,
                    __driver: ::elyra::db::Driver,
                ) -> impl ::std::future::Future<Output = ::elyra::db::Result<bool>> + ::std::marker::Send + '__a {
                    async move {
                        let __data: &[&str] = &[ #(#data_cols),* ];
                        if __data.is_empty() {
                            return ::std::result::Result::Ok(false);
                        }
                        let __now = #now_expr;
                        let __touch: ::std::option::Option<&str> = #touch_col;
                        let mut __i = 1usize;
                        let mut __next = || { let __n = __i; __i += 1; ::elyra::db::model::placeholder(__driver, __n) };
                        let mut __sets: ::std::vec::Vec<::std::string::String> = __data.iter().map(|__c| {
                            ::std::format!("{} = {}", __c, <Self as ::elyra::db::model::Model>::bind_column(__driver, __c, __next()))
                        }).collect();
                        if let ::std::option::Option::Some(__t) = __touch {
                            __sets.push(::std::format!("{} = {}", __t, __next()));
                        }
                        let __keys: &[&str] = &[ #(#key_cols),* ];
                        let __wheres: ::std::vec::Vec<::std::string::String> = __keys.iter().map(|__k| {
                            ::std::format!("{} = {}", __k, __next())
                        }).collect();
                        let __same = ::elyra::db::model::same_as(__driver);
                        let __equal: ::std::vec::Vec<::std::string::String> = __data.iter().map(|__c| {
                            ::std::format!("{} {} {}", __c, __same, <Self as ::elyra::db::model::Model>::bind_column(__driver, __c, __next()))
                        }).collect();
                        let __sql = ::std::format!(
                            "UPDATE {} SET {} WHERE {} AND NOT ({})",
                            #table, __sets.join(", "), __wheres.join(" AND "), __equal.join(" AND ")
                        );
                        let mut __args = ::elyra::db::sqlx::any::AnyArguments::default();
                        #( #data_args )*
                        #touch_arg
                        #( #key_args )*
                        #( #data_args )*
                        let __done = ::elyra::db::sqlx::query_with(::elyra::db::sqlx::AssertSqlSafe(__sql), __args)
                            .execute(&mut *__conn).await?;
                        if __done.rows_affected() > 0 {
                            #touch_set
                            return ::std::result::Result::Ok(true);
                        }
                        ::std::result::Result::Ok(false)
                    }
                }
            }
        }
    } else {
        quote! {}
    };

    let expanded = quote! {
        impl ::elyra::db::model::Model for #name {
            const TABLE: &'static str = #table;
            const PK: &'static str = #pk_col;
            const KEY: &'static [&'static str] = &[ #(#key_cols),* ];
            const COLUMNS: &'static [&'static str] = &[ #(#all_cols),* ];
            type Key = #key_ty;
            const SOFT_DELETE: ::std::option::Option<&'static str> = #soft_delete_col;

            fn from_row(__row: &::elyra::db::sqlx::any::AnyRow) -> ::elyra::db::Result<Self> {
                ::std::result::Result::Ok(Self { #( #from_row_fields ),* })
            }

            fn key(&self) -> #key_ty {
                #key_expr
            }

            fn get_i64(&self, __column: &str) -> ::std::option::Option<i64> {
                match __column {
                    #( #i64_arms )*
                    _ => ::std::option::Option::None,
                }
            }

            #global_scope_items
            #cast_items
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

            /// Find one row by its key (respecting soft deletes and global
            /// scopes — `Self::query().with_trashed().find(..)` reaches a trashed
            /// row). A composite key is a tuple in declaration order.
            pub async fn find(__db: &::elyra::db::Database, __id: #key_ty) -> ::elyra::db::Result<::std::option::Option<Self>> {
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
                __db.before_write(#table)?;
                #insert_ts
                #insert_body
                __db.wrote(#table);
                ::std::result::Result::Ok(())
            }

            /// Update this row by its key.
            pub async fn update(&mut self, __db: &::elyra::db::Database) -> ::elyra::db::Result<()> {
                __db.before_write(#table)?;
                #update_ts
                let __cols: &[&str] = &[ #(#insert_cols),* ];
                if __cols.is_empty() {
                    // Every column is the key's: nothing to set.
                    return ::std::result::Result::Ok(());
                }
                let mut __i = 1usize;
                let __sets: ::std::vec::Vec<::std::string::String> = __cols.iter().map(|__c| {
                    let __p = <Self as ::elyra::db::model::Model>::bind_column(
                        __db.driver(),
                        __c,
                        ::elyra::db::model::placeholder(__db.driver(), __i),
                    );
                    __i += 1;
                    ::std::format!("{} = {}", __c, __p)
                }).collect();
                let __keys: &[&str] = &[ #(#key_cols),* ];
                let __wheres: ::std::vec::Vec<::std::string::String> = __keys.iter().map(|__k| {
                    let __p = ::elyra::db::model::placeholder(__db.driver(), __i);
                    __i += 1;
                    ::std::format!("{} = {}", __k, __p)
                }).collect();
                let __sql = ::std::format!(
                    "UPDATE {} SET {} WHERE {}",
                    #table, __sets.join(", "), __wheres.join(" AND ")
                );
                let mut __args = ::elyra::db::sqlx::any::AnyArguments::default();
                #( #update_args )*
                #( #key_args )*
                ::elyra::db::sqlx::query_with(::elyra::db::sqlx::AssertSqlSafe(__sql), __args)
                    .execute(__db.pool()).await?;
                __db.wrote(#table);
                ::std::result::Result::Ok(())
            }

            /// Delete this row by its key.
            pub async fn delete(&self, __db: &::elyra::db::Database) -> ::elyra::db::Result<()> {
                __db.before_write(#table)?;
                let __keys: &[&str] = &[ #(#key_cols),* ];
                let __wheres: ::std::vec::Vec<::std::string::String> = __keys.iter().enumerate().map(|(__n, __k)| {
                    ::std::format!("{} = {}", __k, ::elyra::db::model::placeholder(__db.driver(), __n + 1))
                }).collect();
                let __sql = ::std::format!("DELETE FROM {} WHERE {}", #table, __wheres.join(" AND "));
                let mut __args = ::elyra::db::sqlx::any::AnyArguments::default();
                #( #key_args )*
                ::elyra::db::sqlx::query_with(::elyra::db::sqlx::AssertSqlSafe(__sql), __args)
                    .execute(__db.pool()).await?;
                __db.wrote(#table);
                ::std::result::Result::Ok(())
            }

            /// Insert if unsaved (the key is its type's default), otherwise
            /// update. A composite key has no unsaved value: an upsert on the
            /// key, which needs a primary key or unique index on its columns.
            pub async fn save(&mut self, __db: &::elyra::db::Database) -> ::elyra::db::Result<()> {
                #save_body
            }

            #( #relation_methods )*
            #( #field_relation_methods )*
        }

        #pivot_row_impl
    };

    expanded.into()
}

// ---------------------------------------------------------------------------
// #[command]
// ---------------------------------------------------------------------------

/// Whether `link` is an app path whose `{placeholders}` name one of `args`
/// or `result.<field>`.
fn check_link(link: &str, args: &[syn::Ident]) -> Result<(), String> {
    if !link.starts_with('/') || link.contains(char::is_whitespace) {
        return Err(format!(
            "a link is an app path without spaces, starting with `/`: `/customers/{{id}}`, not `{link}`"
        ));
    }
    let mut rest = link;
    while let Some(open) = rest.find(['{', '}']) {
        if &rest[open..=open] == "}" {
            return Err(format!("a `}}` without its `{{` in `{link}`"));
        }
        let after = &rest[open + 1..];
        let close = after
            .find('}')
            .ok_or_else(|| format!("a `{{` without its `}}` in `{link}`"))?;
        let name = &after[..close];
        let known = match name.strip_prefix("result.") {
            Some(field) => {
                !field.is_empty() && field.chars().all(|c| c.is_alphanumeric() || c == '_')
            }
            None => args.iter().any(|a| a == name),
        };
        if !known {
            let names: Vec<String> = args.iter().map(|a| a.to_string()).collect();
            return Err(format!(
                "`{{{name}}}` in the link isn't an argument ({}) or `result.<field>`",
                names.join(", ")
            ));
        }
        rest = &after[close + 1..];
    }
    Ok(())
}

#[proc_macro_attribute]
pub fn command(attr: TokenStream, item: TokenStream) -> TokenStream {
    // `#[command(can = "posts.delete")]` — the ability the *frontend* must hold
    // to reach this command. Anything else in the attribute is a typo, so it is
    // an error rather than silently ignored (as the whole attribute used to be).
    let mut ability: Option<syn::LitStr> = None;
    let mut middleware: Vec<syn::LitStr> = Vec::new();
    // `#[command(live)]`: the frontend may subscribe to it (RFC 0002).
    let mut live = false;
    // `#[command(link = "/customers/{id}")]`: where its record is in the app,
    // for the assistant's panel (RFC 0013).
    let mut link: Option<syn::LitStr> = None;
    if !attr.is_empty() {
        let parser =
            syn::meta::parser(|meta| {
                if meta.path.is_ident("can") {
                    ability = Some(meta.value()?.parse()?);
                    Ok(())
                } else if meta.path.is_ident("live") {
                    live = true;
                    Ok(())
                } else if meta.path.is_ident("link") {
                    link = Some(meta.value()?.parse()?);
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
                        "unknown #[command] option; expected `can = \"ability\"`, \
                     `middleware = [\"name\", ..]`, `live` or `link = \"/path/{arg}\"`",
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

    // The doc comment is the command's description — an MCP tool's, for one.
    let description = func
        .attrs
        .iter()
        .filter(|a| a.path().is_ident("doc"))
        .filter_map(|a| match &a.meta {
            syn::Meta::NameValue(nv) => match &nv.value {
                syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(s),
                    ..
                }) => Some(s.value()),
                _ => None,
            },
            _ => None,
        })
        .map(|line| line.strip_prefix(' ').unwrap_or(&line).to_owned())
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned();
    let description_impl = if description.is_empty() {
        quote! {}
    } else {
        quote! {
            fn description(&self) -> &'static str { #description }
        }
    };

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

    // A link is an app path whose `{placeholders}` are arguments or
    // `result.<field>`: checked here, so a typo is a compile error.
    if let Some(lit) = &link {
        if let Err(why) = check_link(&lit.value(), &arg_names) {
            return syn::Error::new_spanned(lit, why).to_compile_error().into();
        }
    }
    let link_impl = match &link {
        Some(lit) => quote! {
            fn link(&self) -> ::std::option::Option<&'static str> {
                ::std::option::Option::Some(#lit)
            }
        },
        None => quote! {},
    };

    let await_tok = asyncness.map(|_| quote!(.await)).unwrap_or_default();
    // `call` runs the function itself: `name::run` (RFC 0025).
    let inner_call = quote! { #fn_ident::run(__ctx, #(#arg_names),*) #await_tok };
    // The function's own attributes (its doc comment, an `#[allow(..)]`) go
    // on `run`; the doc comment also on the struct, for rustdoc.
    let fn_attrs = &func.attrs;
    let doc_attrs = func.attrs.iter().filter(|a| a.path().is_ident("doc"));

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
                    // An `elyra::Error` keeps its variant (and kind); any other
                    // error becomes a command error.
                    #[allow(unused_imports)]
                    use ::elyra::__private::{DisplayError as _, KeepError as _};
                    ::elyra::Result::Err(
                        (&::elyra::__private::ErrorOf(::std::cell::Cell::new(
                            ::std::option::Option::Some(__e),
                        )))
                        .take_error(),
                    )
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

    let live_impl = if live {
        quote! {
            fn live(&self) -> bool { true }
        }
    } else {
        quote! {}
    };

    let expanded = quote! {
        #(#doc_attrs)*
        #[allow(non_camel_case_types)]
        #[derive(::std::clone::Clone, ::std::marker::Copy)]
        #vis struct #fn_ident;

        impl #fn_ident {
            #(#fn_attrs)*
            ///
            /// The function itself (RFC 0025): called from Rust, it doesn't
            /// go through the pipeline (middleware, abilities, the window's
            /// scope, rate limits), which limit what the webview may reach.
            #vis #asyncness fn run(#inputs) #output #block
        }

        impl ::elyra::command::Command for #fn_ident {
            fn name(&self) -> &'static str { #fn_name }

            #ability_impl
            #middleware_impl
            #live_impl
            #link_impl
            #description_impl

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
                ::std::boxed::Box::pin(async move {
                    #decode
                    #encode_out
                })
            }
        }
    };

    expanded.into()
}

#[cfg(test)]
mod link_tests {
    use super::check_link;

    fn args(names: &[&str]) -> Vec<syn::Ident> {
        names
            .iter()
            .map(|n| syn::Ident::new(n, proc_macro2::Span::call_site()))
            .collect()
    }

    #[test]
    fn a_link_names_arguments_or_result_fields() {
        let id = args(&["id", "input"]);
        assert!(check_link("/customers", &id).is_ok());
        assert!(check_link("/customers/{id}", &id).is_ok());
        assert!(check_link("/customers/{result.id}/edit?from={input}", &id).is_ok());
        let unknown = check_link("/customers/{customer_id}", &id).unwrap_err();
        assert!(
            unknown.contains("`{customer_id}`") && unknown.contains("id, input"),
            "{unknown}"
        );
        assert!(check_link("/customers/{result.}", &id).is_err());
        assert!(check_link("/customers/{result.a-b}", &id).is_err());
    }

    #[test]
    fn a_link_is_an_app_path_with_closed_braces() {
        let id = args(&["id"]);
        assert!(check_link("customers/{id}", &id).is_err());
        assert!(check_link("https://example.com/{id}", &id).is_err());
        assert!(check_link("/customers/ {id}", &id).is_err());
        assert!(check_link("/customers/{id", &id).is_err());
        assert!(check_link("/customers/id}", &id).is_err());
    }
}
