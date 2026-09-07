//! Codegen : AST Rava (forme Java) -> code source Rust.
//!
//! Le générateur ne fait aucune analyse sémantique : il traduit la syntaxe et
//! laisse `rustc` faire le typage, l'emprunt et la vérification de durée de vie.
//! C'est le principe de Rava : la sémantique reste exactement celle de Rust.

use rava_ast::*;
use std::fmt::Write as _;

#[derive(Debug, Clone)]
pub struct Diag {
    pub message: String,
    pub line: u32,
    pub col: u32,
    pub note: Option<String>,
}

impl std::fmt::Display for Diag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}: {}", self.line, self.col, self.message)?;
        if let Some(n) = &self.note {
            write!(f, "\n  note: {n}")?;
        }
        Ok(())
    }
}

type R<T> = Result<T, Diag>;

fn err<T>(span: Span, msg: impl Into<String>) -> R<T> {
    Err(Diag { message: msg.into(), line: span.line, col: span.col, note: None })
}

fn err_note<T>(span: Span, msg: impl Into<String>, note: impl Into<String>) -> R<T> {
    Err(Diag {
        message: msg.into(),
        line: span.line,
        col: span.col,
        note: Some(note.into()),
    })
}

pub fn generate(unit: &Unit) -> R<String> {
    let mut cg = Codegen { out: String::new(), indent: 0, tmp: 0 };
    cg.unit(unit)?;
    Ok(cg.out)
}

struct Codegen {
    out: String,
    indent: usize,
    tmp: u32,
}

impl Codegen {
    // ------------------------------------------------------------- sortie

    fn line(&mut self, s: &str) {
        if !s.is_empty() {
            for _ in 0..self.indent {
                self.out.push_str("    ");
            }
            self.out.push_str(s);
        }
        self.out.push('\n');
    }

    fn blank(&mut self) {
        if !self.out.ends_with("\n\n") && !self.out.is_empty() {
            self.out.push('\n');
        }
    }

    fn open(&mut self, s: &str) {
        self.line(s);
        self.indent += 1;
    }

    fn close(&mut self, s: &str) {
        self.indent = self.indent.saturating_sub(1);
        self.line(s);
    }

    fn fresh(&mut self, base: &str) -> String {
        self.tmp += 1;
        format!("__rava_{base}{}", self.tmp)
    }

    // ------------------------------------------------------------- unité

    fn unit(&mut self, u: &Unit) -> R<()> {
        self.line("// Généré par ravac depuis un source Rava (syntaxe Java, sémantique Rust).");
        self.line("// Ne pas éditer : modifiez le fichier .rava correspondant.");
        // Les identifiants Rava sont repris tels quels : on garde le camelCase Java.
        self.line("#![allow(non_snake_case, non_camel_case_types, unused_parens, dead_code)]");
        self.blank();

        for imp in &u.imports {
            let mut path = imp.path.join("::");
            if imp.glob {
                path.push_str("::*");
            }
            self.line(&format!("use {path};"));
        }
        if !u.imports.is_empty() {
            self.blank();
        }

        for item in &u.items {
            self.item(item)?;
            self.blank();
        }
        Ok(())
    }

    fn item(&mut self, item: &Item) -> R<()> {
        match item {
            Item::Class(c) => self.class(c),
            Item::Interface(i) => self.interface(i),
            Item::Enum(e) => self.enum_(e),
            Item::Record(r) => self.record(r),
        }
    }

    // ------------------------------------------------------------- attributs

    /// Traduit les annotations « méta » en attributs Rust.
    fn attrs(&mut self, annots: &[Annot], doc: Option<&str>) {
        if let Some(d) = doc {
            for l in d.lines() {
                self.line(&format!("/// {l}"));
            }
        }
        if let Some(a) = find_annot(annots, "Doc") {
            for t in a.texts() {
                for l in t.lines() {
                    self.line(&format!("/// {l}"));
                }
            }
        }
        if let Some(a) = find_annot(annots, "Derive") {
            let list = a.texts().join(", ");
            if !list.is_empty() {
                self.line(&format!("#[derive({list})]"));
            }
        }
        if let Some(a) = find_annot(annots, "Repr") {
            self.line(&format!("#[repr({})]", a.texts().join(", ")));
        }
        if has_annot(annots, "Inline") {
            self.line("#[inline]");
        }
        if has_annot(annots, "Test") {
            self.line("#[test]");
        }
        for a in annots.iter().filter(|a| a.name == "Attr") {
            for t in a.texts() {
                self.line(&format!("#[{t}]"));
            }
        }
        for a in annots.iter().filter(|a| a.name == "Rust") {
            for t in a.texts() {
                self.line(&t);
            }
        }
    }

    fn vis(modifiers: &[Modifier]) -> &'static str {
        if modifiers.contains(&Modifier::Public) {
            "pub "
        } else if modifiers.contains(&Modifier::Protected) {
            "pub(crate) "
        } else {
            ""
        }
    }

    // ------------------------------------------------------------- class

    fn class(&mut self, c: &Class) -> R<()> {
        if let Some(sup) = &c.extends {
            return err_note(
                c.span,
                format!(
                    "`class {} extends {}` : Rust n'a pas d'héritage de classe",
                    c.name,
                    sup.last_segment().unwrap_or("?")
                ),
                "composez (un champ du type parent) ou factorisez le comportement dans une interface. Voir docs/IMPOSSIBLE.md#heritage",
            );
        }

        self.attrs(&c.annots, None);
        let gen = self.generics_decl(&c.generics);
        let wher = Self::where_clause(&c.generics);
        let vis = Self::vis(&c.modifiers);

        if c.fields.is_empty() {
            self.line(&format!("{vis}struct {}{gen}{wher};", c.name));
        } else {
            self.open(&format!("{vis}struct {}{gen}{wher} {{", c.name));
            for f in &c.fields {
                let fvis = Self::vis(&f.modifiers);
                self.line(&format!("{fvis}{}: {},", f.name, self.type_(&f.ty)?));
            }
            self.close("}");
        }

        let (inherent, by_trait) = Self::split_methods(c, &c.implements)?;

        if !inherent.is_empty() || !c.consts.is_empty() {
            self.blank();
            self.open(&format!("impl{gen} {}{}{wher} {{", c.name, Self::generics_use(&c.generics)));
            for k in &c.consts {
                self.const_decl(k)?;
            }
            for m in inherent {
                self.method(m, MethodCtx::Inherent { owner: c, fields: &c.fields })?;
            }
            self.close("}");
        }

        let single_trait = c.implements.len() == 1;
        for (trait_name, methods) in by_trait {
            self.blank();
            self.open(&format!(
                "impl{gen} {trait_name} for {}{}{wher} {{",
                c.name,
                Self::generics_use(&c.generics)
            ));
            for a in Self::assoc_for_trait(&c.assoc_types, &trait_name, single_trait) {
                let Some(d) = &a.default else {
                    return err_note(
                        a.span,
                        format!("le type associé `{}` doit être défini ici", a.name),
                        "écrivez `type Cle = String;` dans la classe qui implémente le trait",
                    );
                };
                self.line(&format!("type {} = {};", a.name, self.type_(d)?));
            }
            for m in methods {
                self.method(m, MethodCtx::TraitImpl { owner: c, fields: &c.fields })?;
            }
            self.close("}");
        }

        // `public static void main(String[] args)` devient le point d'entrée libre.
        for m in c.methods.iter().filter(|m| Self::is_main(m)) {
            self.blank();
            self.main_fn(m)?;
        }

        for n in &c.nested {
            self.blank();
            self.item(n)?;
        }
        Ok(())
    }

    fn is_main(m: &Method) -> bool {
        m.name == "main"
            && m.modifiers.contains(&Modifier::Static)
            && matches!(m.ret.kind, TypeKind::Void)
    }

    fn main_fn(&mut self, m: &Method) -> R<()> {
        self.open("fn main() {");
        if let Some(p) = m.params.first() {
            self.line("#[allow(unused_variables)]");
            self.line(&format!(
                "let {}: Vec<String> = std::env::args().collect();",
                p.name
            ));
        }
        if let Some(b) = &m.body {
            for s in &b.stmts {
                self.stmt(s)?;
            }
        }
        self.close("}");
        Ok(())
    }

    /// Répartit les méthodes entre bloc inhérent et blocs `impl Trait for`.
    fn split_methods<'a>(
        c: &'a Class,
        implements: &'a [Type],
    ) -> R<(Vec<&'a Method>, Vec<(String, Vec<&'a Method>)>)> {
        let mut inherent = Vec::new();
        let mut groups: Vec<(String, Vec<&Method>)> = implements
            .iter()
            .map(|t| (render_path_only(t), Vec::new()))
            .collect();

        for m in &c.methods {
            if Self::is_main(m) {
                continue;
            }
            let Some(ov) = find_annot(&m.annots, "Override") else {
                inherent.push(m);
                continue;
            };
            let target = match ov.first_text() {
                Some(t) => t.to_string(),
                None => {
                    if groups.len() == 1 {
                        groups[0].0.clone()
                    } else if groups.is_empty() {
                        return err_note(
                            m.span,
                            format!("`@Override` sur `{}` mais la classe n'implémente aucune interface", m.name),
                            "retirez `@Override`, ou ajoutez `implements <Trait>`",
                        );
                    } else {
                        return err_note(
                            m.span,
                            format!("`@Override` ambigu sur `{}` : plusieurs interfaces implémentées", m.name),
                            "précisez le trait : `@Override(Display.class)`",
                        );
                    }
                }
            };
            let short = target.rsplit("::").next().unwrap_or(&target).to_string();
            match groups
                .iter_mut()
                .find(|(n, _)| *n == target || n.rsplit("::").next() == Some(short.as_str()))
            {
                Some((_, v)) => v.push(m),
                None => groups.push((target, vec![m])),
            }
        }
        groups.retain(|(_, v)| !v.is_empty() || true);
        Ok((inherent, groups))
    }

    // ------------------------------------------------------------- record


    /// Répartit les types associés vers les blocs `impl Trait for` : même règle
    /// que `@Override` — le trait unique par défaut, `@Override(T.class)` sinon.
    fn assoc_for_trait<'a>(
        assoc: &'a [AssocType],
        trait_name: &str,
        single: bool,
    ) -> Vec<&'a AssocType> {
        let short = trait_name.rsplit("::").next().unwrap_or(trait_name);
        assoc
            .iter()
            .filter(|a| match find_annot(&a.annots, "Override").and_then(|o| o.first_text()) {
                Some(t) => t.rsplit("::").next() == Some(short) || t == trait_name,
                None => single,
            })
            .collect()
    }

    fn record(&mut self, r: &Record) -> R<()> {
        self.attrs(&r.annots, None);
        let gen = self.generics_decl(&r.generics);
        let wher = Self::where_clause(&r.generics);
        let vis = Self::vis(&r.modifiers);
        self.open(&format!("{vis}struct {}{gen}{wher} {{", r.name));
        for p in &r.components {
            self.line(&format!("pub {}: {},", p.name, self.type_(&p.ty)?));
        }
        self.close("}");

        self.blank();
        self.open(&format!("impl{gen} {}{}{wher} {{", r.name, Self::generics_use(&r.generics)));
        let args = r
            .components
            .iter()
            .map(|p| Ok(format!("{}: {}", p.name, self.type_(&p.ty)?)))
            .collect::<R<Vec<_>>>()?
            .join(", ");
        self.open(&format!("pub fn new({args}) -> Self {{"));
        let init = r.components.iter().map(|p| p.name.clone()).collect::<Vec<_>>().join(", ");
        self.line(&format!("Self {{ {init} }}"));
        self.close("}");
        // Accesseurs, comme les records Java.
        for p in &r.components {
            if r.methods.iter().any(|m| m.name == p.name) {
                continue;
            }
            self.open(&format!("pub fn {}(&self) -> &{} {{", p.name, self.type_(&p.ty)?));
            self.line(&format!("&self.{}", p.name));
            self.close("}");
        }
        for k in &r.consts {
            self.const_decl(k)?;
        }
        let shell = Class {
            annots: Vec::new(),
            modifiers: Vec::new(),
            name: r.name.clone(),
            generics: r.generics.clone(),
            implements: r.implements.clone(),
            extends: None,
            fields: r
                .components
                .iter()
                .map(|p| Field {
                    annots: Vec::new(),
                    modifiers: vec![Modifier::Public],
                    ty: p.ty.clone(),
                    name: p.name.clone(),
                    init: None,
                    span: p.span,
                })
                .collect(),
            methods: r.methods.clone(),
            consts: Vec::new(),
            assoc_types: r.assoc_types.clone(),
            nested: Vec::new(),
            span: r.span,
        };
        let (inherent, by_trait) = Self::split_methods(&shell, &r.implements)?;
        for m in inherent {
            self.method(m, MethodCtx::Inherent { owner: &shell, fields: &shell.fields })?;
        }
        self.close("}");

        for (trait_name, methods) in by_trait {
            if methods.is_empty() {
                continue;
            }
            self.blank();
            self.open(&format!(
                "impl{gen} {trait_name} for {}{}{wher} {{",
                r.name,
                Self::generics_use(&r.generics)
            ));
            for m in methods {
                self.method(m, MethodCtx::TraitImpl { owner: &shell, fields: &shell.fields })?;
            }
            self.close("}");
        }
        Ok(())
    }

    // ------------------------------------------------------------- interface

    fn interface(&mut self, i: &Interface) -> R<()> {
        self.attrs(&i.annots, None);
        let gen = self.generics_decl(&i.generics);
        let wher = Self::where_clause(&i.generics);
        let vis = Self::vis(&i.modifiers);
        let supers = if i.extends.is_empty() {
            String::new()
        } else {
            let mut parts = Vec::new();
            for t in &i.extends {
                parts.push(self.type_(t)?);
            }
            format!(": {}", parts.join(" + "))
        };
        self.open(&format!("{vis}trait {}{gen}{supers}{wher} {{", i.name));
        for a in &i.assoc_types {
            let bounds = if a.bounds.is_empty() {
                String::new()
            } else {
                let mut parts = Vec::new();
                for b in &a.bounds {
                    parts.push(self.type_(b)?);
                }
                format!(": {}", parts.join(" + "))
            };
            match &a.default {
                Some(d) => self.line(&format!("type {}{bounds} = {};", a.name, self.type_(d)?)),
                None => self.line(&format!("type {}{bounds};", a.name)),
            }
        }
        for k in &i.consts {
            let value = self.expr(&k.value)?;
            self.line(&format!("const {}: {} = {value};", k.name, self.type_(&k.ty)?));
        }
        for m in &i.methods {
            self.method(m, MethodCtx::TraitDecl)?;
        }
        self.close("}");
        Ok(())
    }

    // ------------------------------------------------------------- enum

    fn enum_(&mut self, e: &EnumDecl) -> R<()> {
        self.attrs(&e.annots, None);
        let gen = self.generics_decl(&e.generics);
        let wher = Self::where_clause(&e.generics);
        let vis = Self::vis(&e.modifiers);
        self.open(&format!("{vis}enum {}{gen}{wher} {{", e.name));
        for v in &e.variants {
            self.attrs(&v.annots, None);
            if v.payload.is_empty() {
                match &v.discriminant {
                    Some(d) => {
                        let d = self.expr(d)?;
                        self.line(&format!("{} = {d},", v.name));
                    }
                    None => self.line(&format!("{},", v.name)),
                }
            } else if has_annot(&v.annots, "Struct") {
                self.open(&format!("{} {{", v.name));
                for p in &v.payload {
                    self.line(&format!("{}: {},", p.name, self.type_(&p.ty)?));
                }
                self.close("},");
            } else {
                let mut parts = Vec::new();
                for p in &v.payload {
                    parts.push(self.type_(&p.ty)?);
                }
                self.line(&format!("{}({}),", v.name, parts.join(", ")));
            }
        }
        self.close("}");

        let shell = Class {
            annots: Vec::new(),
            modifiers: Vec::new(),
            name: e.name.clone(),
            generics: e.generics.clone(),
            implements: e.implements.clone(),
            extends: None,
            fields: Vec::new(),
            methods: e.methods.clone(),
            consts: e.consts.clone(),
            assoc_types: e.assoc_types.clone(),
            nested: Vec::new(),
            span: e.span,
        };
        let (inherent, by_trait) = Self::split_methods(&shell, &e.implements)?;
        if !inherent.is_empty() || !e.consts.is_empty() {
            self.blank();
            self.open(&format!("impl{gen} {}{}{wher} {{", e.name, Self::generics_use(&e.generics)));
            for k in &e.consts {
                self.const_decl(k)?;
            }
            for m in inherent {
                self.method(m, MethodCtx::Inherent { owner: &shell, fields: &[] })?;
            }
            self.close("}");
        }
        for (trait_name, methods) in by_trait {
            if methods.is_empty() {
                continue;
            }
            self.blank();
            self.open(&format!(
                "impl{gen} {trait_name} for {}{}{wher} {{",
                e.name,
                Self::generics_use(&e.generics)
            ));
            for m in methods {
                self.method(m, MethodCtx::TraitImpl { owner: &shell, fields: &[] })?;
            }
            self.close("}");
        }
        Ok(())
    }

    fn const_decl(&mut self, k: &ConstDecl) -> R<()> {
        self.attrs(&k.annots, None);
        let vis = Self::vis(&k.modifiers);
        let value = self.expr(&k.value)?;
        self.line(&format!("{vis}const {}: {} = {value};", k.name, self.type_(&k.ty)?));
        Ok(())
    }

    // ------------------------------------------------------------- méthodes

    fn method(&mut self, m: &Method, ctx: MethodCtx<'_>) -> R<()> {
        if m.is_ctor {
            return self.constructor(m, ctx);
        }
        self.attrs(&m.annots, None);

        let name = find_annot(&m.annots, "Named")
            .and_then(|a| a.first_text().map(str::to_string))
            .unwrap_or_else(|| m.name.clone());

        let mut sig = String::new();
        if Self::vis(&m.modifiers) == "pub "
            && matches!(ctx, MethodCtx::Inherent { .. })
        {
            sig.push_str("pub ");
        }
        if has_annot(&m.annots, "Unsafe") {
            sig.push_str("unsafe ");
        }
        if has_annot(&m.annots, "Async") {
            sig.push_str("async ");
        }
        if let Some(a) = find_annot(&m.annots, "Extern") {
            let abi = a.first_text().unwrap_or("C");
            let _ = write!(sig, "extern \"{abi}\" ");
        }
        let gen = self.generics_decl(&m.generics);
        let _ = write!(sig, "fn {name}{gen}(");

        let mut params = Vec::new();
        if let Some(sp) = Self::self_param(m) {
            params.push(sp);
        }
        for p in &m.params {
            if p.varargs {
                return err_note(
                    p.span,
                    "les varargs `...` n'existent pas en Rust",
                    "passez un slice : `@Ref T[] items`. Voir docs/IMPOSSIBLE.md#varargs",
                );
            }
            let binding = if has_annot(&p.annots, "Mut") && !has_annot(&p.annots, "Ref") {
                format!("mut {}", p.name)
            } else {
                p.name.clone()
            };
            params.push(format!("{binding}: {}", self.type_(&p.ty)?));
        }
        sig.push_str(&params.join(", "));
        sig.push(')');

        if !matches!(m.ret.kind, TypeKind::Void) {
            let _ = write!(sig, " -> {}", self.type_(&m.ret)?);
        }
        sig.push_str(&Self::where_clause(&m.generics));

        match &m.body {
            None => {
                self.line(&format!("{sig};"));
            }
            Some(b) => {
                self.open(&format!("{sig} {{"));
                self.body_stmts(b, !matches!(m.ret.kind, TypeKind::Void))?;
                self.close("}");
            }
        }
        Ok(())
    }

    /// Détermine le receveur : `&self`, `&mut self`, `self` ou aucun.
    fn self_param(m: &Method) -> Option<String> {
        if m.modifiers.contains(&Modifier::Static) {
            return None;
        }
        let lt = find_annot(&m.annots, "Ref")
            .and_then(|a| a.first_text().map(|l| format!("'{l} ")))
            .unwrap_or_default();
        let mutable = has_annot(&m.annots, "Mut");
        if has_annot(&m.annots, "Owned") {
            return Some(if mutable { "mut self".into() } else { "self".into() });
        }
        Some(if mutable {
            format!("&{lt}mut self")
        } else {
            format!("&{lt}self")
        })
    }

    /// Constructeur Java -> `fn new(..) -> Self`, les `this.f = e;` devenant
    /// les champs du littéral de structure final.
    fn constructor(&mut self, m: &Method, ctx: MethodCtx<'_>) -> R<()> {
        let (MethodCtx::Inherent { owner, fields } | MethodCtx::TraitImpl { owner, fields }) = ctx
        else {
            return err(m.span, "constructeur hors d'un corps de classe");
        };
        self.attrs(&m.annots, None);
        let name = find_annot(&m.annots, "Named")
            .and_then(|a| a.first_text().map(str::to_string))
            .unwrap_or_else(|| "new".to_string());

        let vis = Self::vis(&m.modifiers);
        let gen = self.generics_decl(&m.generics);
        let mut params = Vec::new();
        for p in &m.params {
            let binding = if has_annot(&p.annots, "Mut") && !has_annot(&p.annots, "Ref") {
                format!("mut {}", p.name)
            } else {
                p.name.clone()
            };
            params.push(format!("{binding}: {}", self.type_(&p.ty)?));
        }
        self.open(&format!(
            "{vis}fn {name}{gen}({}) -> Self{} {{",
            params.join(", "),
            Self::where_clause(&m.generics)
        ));

        let body = m.body.clone().unwrap_or_default();
        let mut assigned: Vec<(String, String)> = Vec::new();
        for s in &body.stmts {
            if let Stmt::Expr(Expr::Assign { op: None, target, value, .. }) = s {
                if let Expr::Field { recv, name, .. } = target.as_ref() {
                    if matches!(recv.as_ref(), Expr::This(_)) {
                        assigned.push((name.clone(), self.expr(value)?));
                        continue;
                    }
                }
            }
            self.stmt(s)?;
        }

        let mut init = Vec::new();
        for f in fields {
            if let Some((_, v)) = assigned.iter().find(|(n, _)| n == &f.name) {
                init.push(if v == &f.name {
                    f.name.clone()
                } else {
                    format!("{}: {v}", f.name)
                });
            } else if let Some(d) = &f.init {
                init.push(format!("{}: {}", f.name, self.expr(d)?));
            } else {
                return err_note(
                    m.span,
                    format!(
                        "le constructeur de `{}` n'initialise pas le champ `{}`",
                        owner.name, f.name
                    ),
                    "affectez `this.<champ> = ...;` dans le constructeur, ou donnez une valeur par défaut au champ",
                );
            }
        }
        if init.is_empty() {
            self.line("Self");
        } else {
            self.line(&format!("Self {{ {} }}", init.join(", ")));
        }
        self.close("}");
        Ok(())
    }

    // ------------------------------------------------------------- génériques

    fn generics_decl(&self, g: &Generics) -> String {
        if g.is_empty() {
            return String::new();
        }
        let mut parts: Vec<String> = g.lifetimes.iter().map(|l| format!("'{l}")).collect();
        for p in &g.params {
            if let Some(ct) = &p.const_ty {
                let ty = self.type_(ct).unwrap_or_else(|_| "usize".into());
                parts.push(format!("const {}: {ty}", p.name));
                continue;
            }
            if p.bounds.is_empty() {
                parts.push(p.name.clone());
            } else {
                let bounds: Vec<String> =
                    p.bounds.iter().map(|b| self.type_(b).unwrap_or_default()).collect();
                parts.push(format!("{}: {}", p.name, bounds.join(" + ")));
            }
        }
        format!("<{}>", parts.join(", "))
    }

    /// Forme « à l'usage » : `<'a, T>` sans les bornes.
    fn generics_use(g: &Generics) -> String {
        if g.is_empty() {
            return String::new();
        }
        let mut parts: Vec<String> = g.lifetimes.iter().map(|l| format!("'{l}")).collect();
        parts.extend(g.params.iter().map(|p| p.name.clone()));
        format!("<{}>", parts.join(", "))
    }

    fn where_clause(g: &Generics) -> String {
        if g.where_clauses.is_empty() {
            String::new()
        } else {
            format!(" where {}", g.where_clauses.join(", "))
        }
    }

    // ------------------------------------------------------------- types

    fn type_(&self, t: &Type) -> R<String> {
        let mut prefix = String::new();
        let borrowed = has_annot(&t.annots, "Ref");
        if borrowed {
            prefix.push('&');
            if let Some(l) = find_annot(&t.annots, "Ref").and_then(|a| a.first_text()) {
                let _ = write!(prefix, "'{l} ");
            }
            if has_annot(&t.annots, "Mut") {
                prefix.push_str("mut ");
            }
        }
        if let Some(p) = find_annot(&t.annots, "Ptr") {
            match p.first_text() {
                Some("mut") => prefix.push_str("*mut "),
                _ => prefix.push_str("*const "),
            }
        }
        if has_annot(&t.annots, "Dyn") {
            prefix.push_str("dyn ");
        }
        if has_annot(&t.annots, "Impl") {
            prefix.push_str("impl ");
        }

        let base = match &t.kind {
            TypeKind::Void => "()".to_string(),
            TypeKind::Never => "!".to_string(),
            TypeKind::Infer => "_".to_string(),
            TypeKind::Array(inner) => {
                let i = self.type_(inner)?;
                // `@Ref T[]` est un slice emprunté, `T[]` un Vec possédé.
                if borrowed {
                    format!("[{i}]")
                } else {
                    format!("Vec<{i}>")
                }
            }
            TypeKind::Named { path, args } => self.named_type(path, args, t.span)?,
        };
        Ok(format!("{prefix}{base}"))
    }

    fn named_type(&self, path: &[Ident], args: &[Type], span: Span) -> R<String> {
        let last = path.last().map(String::as_str).unwrap_or("");

        // Tuple<A, B, ...> -> (A, B, ...)
        if last == "Tuple" && path.len() == 1 {
            let mut parts = Vec::new();
            for a in args {
                parts.push(self.type_(a)?);
            }
            return Ok(format!("({})", parts.join(", ")));
        }
        // Unit -> ()
        if last == "Unit" && path.len() == 1 {
            return Ok("()".into());
        }
        // FnN / FnMutN / FnOnceN : le dernier argument est le type de retour.
        if path.len() == 1 {
            for (prefix, kw) in [("FnOnce", "FnOnce"), ("FnMut", "FnMut"), ("Fn", "Fn")] {
                if let Some(rest) = last.strip_prefix(prefix) {
                    if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
                        let n: usize = rest.parse().unwrap();
                        if args.len() != n + 1 {
                            return err(
                                span,
                                format!(
                                    "`{last}` attend {} arguments de type ({n} paramètres + le retour), {} fournis",
                                    n + 1,
                                    args.len()
                                ),
                            );
                        }
                        let mut ps = Vec::new();
                        for a in &args[..n] {
                            ps.push(self.type_(a)?);
                        }
                        let ret = self.type_(&args[n])?;
                        let ret = if ret == "()" { String::new() } else { format!(" -> {ret}") };
                        return Ok(format!("{kw}({}){ret}", ps.join(", ")));
                    }
                }
            }
        }

        let mapped: Vec<String> = path
            .iter()
            .enumerate()
            .map(|(i, s)| {
                if i + 1 == path.len() {
                    map_primitive(s).to_string()
                } else {
                    s.clone()
                }
            })
            .collect();
        let mut out = mapped.join("::");
        if !args.is_empty() {
            let mut parts = Vec::new();
            for a in args {
                parts.push(self.type_(a)?);
            }
            let _ = write!(out, "<{}>", parts.join(", "));
        }
        Ok(out)
    }

    // ------------------------------------------------------------- statements

    /// Corps de fonction : la dernière expression peut être la valeur de retour.
    fn body_stmts(&mut self, b: &Block, _has_ret: bool) -> R<()> {
        for s in &b.stmts {
            self.stmt(s)?;
        }
        Ok(())
    }

    fn stmt(&mut self, s: &Stmt) -> R<()> {
        match s {
            Stmt::Empty => Ok(()),
            Stmt::Block(b) => {
                self.open("{");
                for s in &b.stmts {
                    self.stmt(s)?;
                }
                self.close("}");
                Ok(())
            }
            Stmt::Unsafe(b, _) => {
                self.open("unsafe {");
                for s in &b.stmts {
                    self.stmt(s)?;
                }
                self.close("}");
                Ok(())
            }
            Stmt::Local { annots, modifiers, ty, name, init, .. } => {
                let mutable = has_annot(annots, "Mut") && !has_annot(annots, "Ref");
                let _ = modifiers;
                let mut line = format!("let {}{name}", if mutable { "mut " } else { "" });
                if !matches!(ty.kind, TypeKind::Infer) {
                    let _ = write!(line, ": {}", self.type_(ty)?);
                }
                if let Some(e) = init {
                    let _ = write!(line, " = {}", self.expr(e)?);
                }
                line.push(';');
                self.line(&line);
                Ok(())
            }
            Stmt::LocalPattern { annots, pat, init, .. } => {
                let mutable = has_annot(annots, "Mut");
                let pat_s = self.pattern(pat)?;
                let init_s = self.expr(init)?;
                self.line(&format!(
                    "let {}{pat_s} = {init_s};",
                    if mutable { "mut " } else { "" }
                ));
                Ok(())
            }
            Stmt::Expr(e) => {
                // `x++;` en position d'instruction devient `x += 1;`.
                if matches!(
                    e,
                    Expr::PostIncDec { .. }
                        | Expr::Unary { op: UnOp::PreInc | UnOp::PreDec, .. }
                ) {
                    let line = self.expr_as_stmt(e)?;
                    self.line(&line);
                    return Ok(());
                }
                let rendered = self.expr(e)?;
                self.line(&format!("{rendered};"));
                Ok(())
            }
            Stmt::Return(e, _) => {
                match e {
                    Some(e) => {
                        let v = self.expr(e)?;
                        self.line(&format!("return {v};"));
                    }
                    None => self.line("return;"),
                }
                Ok(())
            }
            Stmt::If { cond, then, otherwise, .. } => {
                let c = self.expr(cond)?;
                self.open(&format!("if {c} {{"));
                self.stmt_as_body(then)?;
                match otherwise {
                    None => self.close("}"),
                    // `else if` / `else unless` restent sur la même ligne.
                    Some(e) if matches!(e.as_ref(), Stmt::If { .. }) => {
                        self.indent -= 1;
                        self.line_raw_else();
                        self.indent += 1;
                        self.stmt_inline_if(e)?;
                    }
                    Some(e) => {
                        self.indent -= 1;
                        self.line("} else {");
                        self.indent += 1;
                        self.stmt_as_body(e)?;
                        self.close("}");
                    }
                }
                Ok(())
            }
            Stmt::While { label, cond, body, .. } => {
                let c = self.expr(cond)?;
                self.open(&format!("{}while {c} {{", label_prefix(label)));
                self.stmt_as_body(body)?;
                self.close("}");
                Ok(())
            }
            Stmt::Loop { label, body, .. } => {
                self.open(&format!("{}loop {{", label_prefix(label)));
                self.stmt_as_body(body)?;
                self.close("}");
                Ok(())
            }
            Stmt::DoWhile { label, body, cond, .. } => {
                let c = self.expr(cond)?;
                self.open(&format!("{}loop {{", label_prefix(label)));
                self.stmt_as_body(body)?;
                self.open(&format!("if !({c}) {{"));
                self.line("break;");
                self.close("}");
                self.close("}");
                Ok(())
            }
            Stmt::ForEach { label, annots, ty, name, iter, body, .. } => {
                let mut it = self.expr(iter)?;
                if has_annot(annots, "Ref") || has_annot(&ty.annots, "Ref") {
                    let m = has_annot(annots, "Mut") || has_annot(&ty.annots, "Mut");
                    it = format!("&{}{it}", if m { "mut " } else { "" });
                }
                let binding = if has_annot(annots, "Mut") && !has_annot(annots, "Ref") {
                    format!("mut {name}")
                } else {
                    name.clone()
                };
                self.open(&format!("{}for {binding} in {it} {{", label_prefix(label)));
                self.stmt_as_body(body)?;
                self.close("}");
                Ok(())
            }
            Stmt::For { label, init, cond, update, body, .. } => {
                self.open("{");
                for s in init {
                    self.stmt(s)?;
                }
                let c = match cond {
                    Some(c) => self.expr(c)?,
                    None => "true".to_string(),
                };
                if update.is_empty() {
                    self.open(&format!("{}while {c} {{", label_prefix(label)));
                    self.stmt_as_body(body)?;
                    self.close("}");
                } else if contains_continue(body) {
                    // `continue` doit exécuter le pas d'itération : on le place en
                    // tête de boucle, sauté au premier tour.
                    let first = self.fresh("first");
                    self.line(&format!("let mut {first} = true;"));
                    self.open(&format!("{}loop {{", label_prefix(label)));
                    self.open(&format!("if {first} {{"));
                    self.line(&format!("{first} = false;"));
                    self.indent -= 1;
                    self.line("} else {");
                    self.indent += 1;
                    for u in update {
                        let u = self.expr_as_stmt(u)?;
                        self.line(&u);
                    }
                    self.close("}");
                    self.open(&format!("if !({c}) {{"));
                    self.line("break;");
                    self.close("}");
                    self.stmt_as_body(body)?;
                    self.close("}");
                } else {
                    self.open(&format!("{}while {c} {{", label_prefix(label)));
                    self.stmt_as_body(body)?;
                    for u in update {
                        let u = self.expr_as_stmt(u)?;
                        self.line(&u);
                    }
                    self.close("}");
                }
                self.close("}");
                Ok(())
            }
            Stmt::Break(label, value, _) => {
                let mut l = String::from("break");
                if let Some(lb) = label {
                    let _ = write!(l, " '{lb}");
                }
                if let Some(v) = value {
                    let _ = write!(l, " {}", self.expr(v)?);
                }
                l.push(';');
                self.line(&l);
                Ok(())
            }
            Stmt::Continue(label, _) => {
                match label {
                    Some(l) => self.line(&format!("continue '{l};")),
                    None => self.line("continue;"),
                }
                Ok(())
            }
            Stmt::Switch(sw) => {
                let m = self.switch(sw)?;
                self.line(&format!("{m};"));
                Ok(())
            }
        }
    }

    fn line_raw_else(&mut self) {
        for _ in 0..self.indent {
            self.out.push_str("    ");
        }
        self.out.push_str("} else ");
    }

    /// Émet `if ... { }` à la suite d'un `} else ` déjà écrit sans saut de ligne.
    fn stmt_inline_if(&mut self, s: &Stmt) -> R<()> {
        let Stmt::If { cond, then, otherwise, .. } = s else {
            return err(Span::default(), "if attendu");
        };
        let c = self.expr(cond)?;
        self.out.push_str(&format!("if {c} {{\n"));
        self.stmt_as_body(then)?;
        match otherwise {
            None => self.close("}"),
            Some(e) if matches!(e.as_ref(), Stmt::If { .. }) => {
                self.indent -= 1;
                self.line_raw_else();
                self.indent += 1;
                self.stmt_inline_if(e)?;
            }
            Some(e) => {
                self.indent -= 1;
                self.line("} else {");
                self.indent += 1;
                self.stmt_as_body(e)?;
                self.close("}");
            }
        }
        Ok(())
    }

    /// Corps d'une structure de contrôle : un bloc est aplati (pas de `{}` en trop).
    fn stmt_as_body(&mut self, s: &Stmt) -> R<()> {
        match s {
            Stmt::Block(b) => {
                for s in &b.stmts {
                    self.stmt(s)?;
                }
                Ok(())
            }
            other => self.stmt(other),
        }
    }

    fn expr_as_stmt(&mut self, e: &Expr) -> R<String> {
        if let Expr::PostIncDec { op, expr, .. } = e {
            let d = if *op == UnOp::PostInc { "+=" } else { "-=" };
            return Ok(format!("{} {d} 1;", self.expr(expr)?));
        }
        if let Expr::Unary { op: op @ (UnOp::PreInc | UnOp::PreDec), expr, .. } = e {
            let d = if *op == UnOp::PreInc { "+=" } else { "-=" };
            return Ok(format!("{} {d} 1;", self.expr(expr)?));
        }
        Ok(format!("{};", self.expr(e)?))
    }

    // ------------------------------------------------------------- match

    fn switch(&mut self, sw: &SwitchExpr) -> R<String> {
        let scrut = self.expr(&sw.scrutinee)?;
        let mut out = format!("match {scrut} {{\n");
        self.indent += 1;
        for arm in &sw.arms {
            let mut pats = Vec::new();
            for p in &arm.patterns {
                pats.push(self.pattern(p)?);
            }
            let guard = match &arm.guard {
                Some(g) => format!(" if {}", self.expr(g)?),
                None => String::new(),
            };
            let pad = "    ".repeat(self.indent);
            match &arm.body {
                SwitchArmBody::Expr(e) => {
                    let _ = write!(out, "{pad}{}{guard} => {},\n", pats.join(" | "), self.expr(e)?);
                }
                SwitchArmBody::Block(b) => {
                    let _ = write!(out, "{pad}{}{guard} => {{\n", pats.join(" | "));
                    let mark = self.out.len();
                    self.indent += 1;
                    for s in &b.stmts {
                        self.stmt(s)?;
                    }
                    self.indent -= 1;
                    let block = self.out.split_off(mark);
                    out.push_str(&block);
                    let _ = write!(out, "{pad}}}\n");
                }
            }
        }
        self.indent -= 1;
        let _ = write!(out, "{}}}", "    ".repeat(self.indent));
        Ok(out)
    }

    fn pattern(&self, p: &Pattern) -> R<String> {
        Ok(match p {
            Pattern::Wildcard(_) => "_".into(),
            Pattern::Lit(l, span) => render_lit(l, *span)?,
            Pattern::Binding { name, .. } => name.clone(),
            Pattern::Path(path, _) => path.join("::"),
            Pattern::TupleStruct { path, elems, .. } => {
                let mut parts = Vec::new();
                for e in elems {
                    parts.push(self.pattern(e)?);
                }
                format!("{}({})", path.join("::"), parts.join(", "))
            }
            Pattern::Struct { path, fields, rest, .. } => {
                let mut parts = Vec::new();
                for (n, p) in fields {
                    parts.push(format!("{n}: {}", self.pattern(p)?));
                }
                if *rest {
                    parts.push("..".into());
                }
                format!("{} {{ {} }}", path.join("::"), parts.join(", "))
            }
            Pattern::Range { start, end, inclusive, .. } => format!(
                "{}{}{}",
                self.pattern(start)?,
                if *inclusive { "..=" } else { ".." },
                self.pattern(end)?
            ),
            Pattern::Ref { mutable, inner, .. } => {
                format!("ref {}{}", if *mutable { "mut " } else { "" }, self.pattern(inner)?)
            }
        })
    }

    // ------------------------------------------------------------- expressions

    fn expr(&mut self, e: &Expr) -> R<String> {
        Ok(match e {
            Expr::Lit(l, span) => render_lit(l, *span)?,
            Expr::This(_) => "self".into(),
            Expr::Super(span) => {
                return err_note(
                    *span,
                    "`super` n'existe pas en Rust",
                    "il n'y a pas d'héritage : appelez explicitement la méthode du trait, p. ex. `Trait::method(self)`. Voir docs/IMPOSSIBLE.md#heritage",
                )
            }
            Expr::Name(path, _) => render_name(path),
            Expr::Paren(inner, _) => format!("({})", self.expr(inner)?),
            Expr::Unary { op, expr, span } => {
                let inner = self.expr(expr)?;
                match op {
                    UnOp::Neg => format!("-{inner}"),
                    UnOp::Plus => inner,
                    UnOp::Not => format!("!{inner}"),
                    UnOp::BitNot => format!("!{inner}"),
                    UnOp::PreInc | UnOp::PreDec => {
                        return err_note(
                            *span,
                            "`++x` / `--x` ne sont pas des expressions en Rust",
                            "utilisez-les comme instruction (`x++;`) ou écrivez `x += 1`. Voir docs/IMPOSSIBLE.md#incrementation",
                        )
                    }
                    UnOp::PostInc | UnOp::PostDec => unreachable!(),
                }
            }
            Expr::PostIncDec { span, .. } => {
                return err_note(
                    *span,
                    "`x++` / `x--` ne sont pas des expressions en Rust",
                    "utilisez-les comme instruction à part entière. Voir docs/IMPOSSIBLE.md#incrementation",
                )
            }
            Expr::Binary { op, lhs, rhs, span } => {
                if *op == BinOp::UShr {
                    return err_note(
                        *span,
                        "`>>>` n'existe pas en Rust",
                        "le décalage est déjà logique sur les types non signés : utilisez `u32`, `u64`, ... Voir docs/IMPOSSIBLE.md#ushr",
                    );
                }
                format!("{} {} {}", self.expr(lhs)?, op.as_rust(), self.expr(rhs)?)
            }
            Expr::Assign { op, target, value, .. } => {
                let o = match op {
                    None => "=".to_string(),
                    Some(b) => format!("{}=", b.as_rust()),
                };
                format!("{} {o} {}", self.expr(target)?, self.expr(value)?)
            }
            Expr::Ternary { cond, then, otherwise, .. } => format!(
                "if {} {{ {} }} else {{ {} }}",
                self.expr(cond)?,
                self.expr(then)?,
                self.expr(otherwise)?
            ),
            Expr::Field { recv, name, .. } => {
                let r = self.expr(recv)?;
                if is_type_name(&r) {
                    format!("{r}::{name}")
                } else {
                    format!("{r}.{name}")
                }
            }
            Expr::Index { recv, index, .. } => {
                format!("{}[{}]", self.expr(recv)?, self.expr(index)?)
            }
            Expr::Cast { ty, expr, .. } => format!("({} as {})", self.expr(expr)?, self.type_(ty)?),
            Expr::New { ty, args, .. } => {
                let mut parts = Vec::new();
                for a in args {
                    parts.push(self.expr(a)?);
                }
                format!("{}::new({})", render_path_only(ty), parts.join(", "))
            }
            Expr::StructLit { ty, fields, rest, .. } => {
                let mut parts = Vec::new();
                for (n, v) in fields {
                    let v = self.expr(v)?;
                    parts.push(if v == *n { n.clone() } else { format!("{n}: {v}") });
                }
                if let Some(r) = rest {
                    parts.push(format!("..{}", self.expr(r)?));
                }
                format!("{} {{ {} }}", render_path_only(ty), parts.join(", "))
            }
            Expr::ArrayLit { elems, .. } => {
                let mut parts = Vec::new();
                for a in elems {
                    parts.push(self.expr(a)?);
                }
                format!("vec![{}]", parts.join(", "))
            }
            Expr::Lambda { params, body, is_move, span: _ } => {
                let mut ps = Vec::new();
                for p in params {
                    match &p.ty {
                        Some(t) => ps.push(format!("{}: {}", p.name, self.type_(t)?)),
                        None => ps.push(p.name.clone()),
                    }
                }
                let head = format!("{}|{}|", if *is_move { "move " } else { "" }, ps.join(", "));
                match body.as_ref() {
                    LambdaBody::Expr(e) => format!("{head} {}", self.expr(e)?),
                    LambdaBody::Block(b) => {
                        let mark = self.out.len();
                        self.indent += 1;
                        for s in &b.stmts {
                            self.stmt(s)?;
                        }
                        self.indent -= 1;
                        let block = self.out.split_off(mark);
                        format!("{head} {{\n{block}{}}}", "    ".repeat(self.indent))
                    }
                }
            }
            Expr::MethodRef { ty, name, .. } => format!("{}::{name}", ty.join("::")),
            Expr::Switch(sw) => self.switch(sw)?,
            Expr::Block(b, _) => {
                let mark = self.out.len();
                self.indent += 1;
                for s in &b.stmts {
                    self.stmt(s)?;
                }
                self.indent -= 1;
                let block = self.out.split_off(mark);
                format!("{{\n{block}{}}}", "    ".repeat(self.indent))
            }
            Expr::Try(inner, _) => format!("{}?", self.expr(inner)?),
            Expr::Await(inner, _) => format!("{}.await", self.expr(inner)?),
            Expr::Borrow { mutable, expr, .. } => {
                format!("&{}{}", if *mutable { "mut " } else { "" }, self.expr(expr)?)
            }
            Expr::Deref(inner, _) => format!("*{}", self.expr(inner)?),
            Expr::Range { start, end, inclusive, .. } => {
                let s = match start {
                    Some(s) => self.expr(s)?,
                    None => String::new(),
                };
                let e = match end {
                    Some(e) => self.expr(e)?,
                    None => String::new(),
                };
                format!("{s}{}{e}", if *inclusive { "..=" } else { ".." })
            }
            Expr::Macro { name, args, .. } => {
                let mut parts = Vec::new();
                for a in args {
                    parts.push(self.expr(a)?);
                }
                format!("{name}!({})", parts.join(", "))
            }
            Expr::RawRust(s, _) => s.clone(),
            Expr::InstanceOf { span, .. } => {
                return err_note(
                    *span,
                    "`instanceof` n'existe pas en Rust",
                    "Rust n'a pas de sous-typage nominal : utilisez `switch` sur une enum, ou `downcast_ref` sur `dyn Any`. Voir docs/IMPOSSIBLE.md#instanceof",
                )
            }
            Expr::Call { recv, name, generics, args, span } => {
                self.call(recv.as_deref(), name, generics, args, *span)?
            }
        })
    }

    fn call(
        &mut self,
        recv: Option<&Expr>,
        name: &str,
        generics: &[Type],
        args: &[Expr],
        span: Span,
    ) -> R<String> {
        let mut rendered = Vec::new();
        for a in args {
            rendered.push(self.expr(a)?);
        }

        // Sortie standard : `System.out.println(...)` -> `println!(...)`.
        if let Some(Expr::Name(path, _)) = recv {
            if let Some(mac) = std_stream_macro(path, name) {
                return Ok(format!("{mac}!({})", rendered.join(", ")));
            }
            if path.as_slice() == ["Move"] && name == "of" {
                let Some(Expr::Lambda { params, body, span, .. }) = args.first() else {
                    return err(span, "Move.of attend une lambda");
                };
                return self.expr(&Expr::Lambda {
                    params: params.clone(),
                    body: body.clone(),
                    is_move: true,
                    span: *span,
                });
            }
            if path.as_slice() == ["Tuple"] && name == "of" {
                return Ok(format!("({})", rendered.join(", ")));
            }
        }

        let turbofish = if generics.is_empty() {
            String::new()
        } else {
            let mut parts = Vec::new();
            for g in generics {
                parts.push(self.type_(g)?);
            }
            format!("::<{}>", parts.join(", "))
        };

        Ok(match recv {
            None => format!("{name}{turbofish}({})", rendered.join(", ")),
            Some(r) => {
                let rs = self.expr(r)?;
                if is_type_name(&rs) {
                    format!("{rs}::{name}{turbofish}({})", rendered.join(", "))
                } else {
                    format!("{rs}.{name}{turbofish}({})", rendered.join(", "))
                }
            }
        })
    }
}

#[derive(Clone, Copy)]
enum MethodCtx<'a> {
    Inherent { owner: &'a Class, fields: &'a [Field] },
    /// Dans un `impl Trait for T`, la visibilité vient du trait : pas de `pub`.
    TraitImpl { owner: &'a Class, fields: &'a [Field] },
    TraitDecl,
}

fn label_prefix(l: &Option<Ident>) -> String {
    match l {
        Some(l) => format!("'{l}: "),
        None => String::new(),
    }
}

/// Un chemin dont un segment commence par une majuscule est un chemin de type
/// (`Color.RED` -> `Color::RED`) ; sinon c'est un accès à un champ (`p.x`).
fn render_name(path: &[Ident]) -> String {
    if path.iter().any(|s| s.chars().next().is_some_and(char::is_uppercase)) {
        path.iter()
            .enumerate()
            .map(|(i, s)| if i + 1 == path.len() { map_primitive(s).to_string() } else { s.clone() })
            .collect::<Vec<_>>()
            .join("::")
    } else {
        path.join(".")
    }
}

fn is_type_name(s: &str) -> bool {
    s.rsplit("::")
        .next()
        .and_then(|seg| seg.chars().next())
        .is_some_and(char::is_uppercase)
        && !s.contains(['.', '(', ' ', '['])
}

fn render_path_only(t: &Type) -> String {
    match &t.kind {
        TypeKind::Named { path, .. } => path
            .iter()
            .enumerate()
            .map(|(i, s)| if i + 1 == path.len() { map_primitive(s).to_string() } else { s.clone() })
            .collect::<Vec<_>>()
            .join("::"),
        _ => "()".into(),
    }
}

/// Les primitives Java sont des alias des types Rust ; tout autre nom passe tel quel,
/// ce qui rend `i32`, `u64`, `usize`, `str`, `Vec`, ... utilisables directement.
fn map_primitive(s: &str) -> &str {
    match s {
        "int" => "i32",
        "long" => "i64",
        "short" => "i16",
        "byte" => "i8",
        "float" => "f32",
        "double" => "f64",
        "boolean" => "bool",
        "Boolean" => "bool",
        "Integer" => "i32",
        "Long" => "i64",
        "Double" => "f64",
        "Float" => "f32",
        "Character" => "char",
        "Byte" => "i8",
        "Short" => "i16",
        "Void" => "()",
        other => other,
    }
}

fn std_stream_macro(path: &[Ident], name: &str) -> Option<&'static str> {
    match (path, name) {
        (p, "println") if p == ["System", "out"] => Some("println"),
        (p, "print") if p == ["System", "out"] => Some("print"),
        (p, "println") if p == ["System", "err"] => Some("eprintln"),
        (p, "print") if p == ["System", "err"] => Some("eprint"),
        _ => None,
    }
}

fn render_lit(l: &Lit, span: Span) -> R<String> {
    Ok(match l {
        Lit::Bool(b) => b.to_string(),
        Lit::Str(s) => format!("\"{}\"", java_escapes_to_rust(s)),
        Lit::Char(c) => format!("'{}'", java_escapes_to_rust(c)),
        Lit::Int(s) => java_int_literal(s),
        Lit::Float(s) => java_float_literal(s),
        Lit::Null => {
            return err_note(
                span,
                "`null` n'existe pas en Rust",
                "utilisez `Option<T>`. Voir docs/IMPOSSIBLE.md#null",
            )
        }
    })
}

/// `\uXXXX` (Java) -> `\u{XXXX}` (Rust) ; les autres échappements coïncident.
fn java_escapes_to_rust(s: &str) -> String {
    let b: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == '\\' && i + 1 < b.len() && b[i + 1] == 'u' {
            let hex: String = b[i + 2..].iter().take(4).collect();
            if hex.len() == 4 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
                out.push_str(&format!("\\u{{{hex}}}"));
                i += 6;
                continue;
            }
        }
        if b[i] == '\\' && i + 1 < b.len() {
            out.push(b[i]);
            out.push(b[i + 1]);
            i += 2;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

/// Suffixes Java (`L`, `l`) -> suffixes Rust (`i64`).
fn java_int_literal(s: &str) -> String {
    if let Some(base) = s.strip_suffix(['L', 'l']) {
        format!("{base}i64")
    } else {
        s.to_string()
    }
}

fn java_float_literal(s: &str) -> String {
    if let Some(base) = s.strip_suffix(['f', 'F']) {
        format!("{base}f32")
    } else if let Some(base) = s.strip_suffix(['d', 'D']) {
        format!("{base}f64")
    } else {
        s.to_string()
    }
}

fn contains_continue(s: &Stmt) -> bool {
    match s {
        Stmt::Continue(..) => true,
        Stmt::Block(b) => b.stmts.iter().any(contains_continue),
        Stmt::Unsafe(b, _) => b.stmts.iter().any(contains_continue),
        Stmt::If { then, otherwise, .. } => {
            contains_continue(then) || otherwise.as_deref().is_some_and(contains_continue)
        }
        Stmt::Switch(sw) => sw.arms.iter().any(|a| match &a.body {
            SwitchArmBody::Block(b) => b.stmts.iter().any(contains_continue),
            SwitchArmBody::Expr(_) => false,
        }),
        // Une boucle interne capture ses propres `continue`.
        _ => false,
    }
}
