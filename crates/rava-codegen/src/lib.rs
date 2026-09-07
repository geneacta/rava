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
    /// Identifiant stable, sur lequel les outils accrochent une correction.
    pub code: Option<&'static str>,
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

/// Emplacement réservé aux masques, remplacé une fois la génération finie.
const PRELUDE_MARKER: &str = "//__RAVA_PRELUDE__\n";

/// Masque de `>>>` : Java n'a que des entiers signés, Rust a les deux. On
/// reproduit le décalage logique en passant par le type non signé de même
/// largeur — exactement ce qu'on écrirait à la main.
const USHR_MASK: &str = r#"mod __rava {
    /// Décalage à droite logique (`>>>` de Java).
    pub trait UShr {
        fn ushr(self, n: u32) -> Self;
    }
    macro_rules! ushr_impl {
        ($($signed:ty => $unsigned:ty),* $(,)?) => {$(
            impl UShr for $signed {
                #[inline]
                fn ushr(self, n: u32) -> Self { ((self as $unsigned) >> n) as Self }
            }
        )*};
    }
    ushr_impl!(
        i8 => u8, i16 => u16, i32 => u32, i64 => u64, i128 => u128, isize => usize,
        u8 => u8, u16 => u16, u32 => u32, u64 => u64, u128 => u128, usize => usize,
    );
}

"#;

fn err<T>(span: Span, msg: impl Into<String>) -> R<T> {
    Err(Diag { message: msg.into(), line: span.line, col: span.col, note: None, code: None })
}

fn err_note<T>(span: Span, msg: impl Into<String>, note: impl Into<String>) -> R<T> {
    Err(Diag {
        message: msg.into(),
        line: span.line,
        col: span.col,
        note: Some(note.into()),
        code: None,
    })
}

/// Comme `err_note`, avec un code stable exploitable par un éditeur.
fn err_fix<T>(
    span: Span,
    msg: impl Into<String>,
    note: impl Into<String>,
    code: &'static str,
) -> R<T> {
    Err(Diag {
        message: msg.into(),
        line: span.line,
        col: span.col,
        note: Some(note.into()),
        code: Some(code),
    })
}

/// Rust généré, plus la ligne `.rava` d'origine de chaque ligne produite.
///
/// C'est ce qui permet de ramener les erreurs de `rustc` — emprunt, durées de
/// vie, typage — sur le fichier que l'on a réellement écrit.
pub struct Output {
    pub rust: String,
    /// Indexée par ligne générée (0-basée) ; `0` quand l'origine est inconnue.
    pub map: Vec<u32>,
}

/// Marqueur déposé en fin de ligne pendant la génération, puis retiré.
///
/// Le codegen assemble certains blocs en les déplaçant (bras de `match`, corps
/// de fermeture) : un compteur de lignes se désynchroniserait. Un marqueur
/// voyage avec sa ligne, quoi qu'il arrive ensuite.
const LINE_MARK: &str = "//~rava:";

/// Ce que le fichier doit savoir du projet qui l'entoure.
///
/// Hors projet (un `.rava` isolé), tout est vide : les `import` passent tels
/// quels, ce qui laisse `std` et les crates externes fonctionner.
#[derive(Debug, Clone, Default)]
pub struct Context {
    /// Premiers segments des paquets du projet. Un `import` qui commence par
    /// l'un d'eux est résolu en `crate::…`.
    pub crate_roots: Vec<String>,
    /// Le fichier appartient-il à un paquet ? Si oui, il ouvre par
    /// `use super::*;` — en Java, le paquet est le seul espace de noms, les
    /// autres fichiers du même paquet sont visibles sans import.
    pub in_package: bool,
}

pub fn generate(unit: &Unit) -> R<String> {
    Ok(generate_with_map(unit)?.rust)
}

pub fn generate_with_map(unit: &Unit) -> R<Output> {
    generate_with_map_in(unit, &Context::default())
}

pub fn generate_with_map_in(unit: &Unit, ctx: &Context) -> R<Output> {
    let mut cg = Codegen {
        out: String::new(),
        indent: 0,
        tmp: 0,
        needs_ushr: false,
        current_trait: None,
        current_method: None,
        cur_line: 0,
    };
    cg.unit(unit, ctx)?;
    let prelude = if cg.needs_ushr { USHR_MASK } else { "" };
    Ok(split_marks(&cg.out.replace(PRELUDE_MARKER, prelude)))
}

/// Sépare le texte de ses marqueurs de ligne.
fn split_marks(marked: &str) -> Output {
    let mut rust = String::new();
    let mut map = Vec::new();
    for line in marked.split('\n') {
        let (text, origin) = match line.rfind(LINE_MARK) {
            // Un marqueur est un suffixe entièrement numérique : s'il ne l'est
            // pas, c'est du texte de l'utilisateur, on n'y touche pas.
            Some(i) => match line[i + LINE_MARK.len()..].parse::<u32>() {
                Ok(n) => (line[..i].trim_end(), n),
                Err(_) => (line, 0),
            },
            None => (line, 0),
        };
        rust.push_str(text);
        rust.push('\n');
        map.push(origin);
    }
    // `split` produit un dernier morceau vide après le `\n` final.
    rust.pop();
    map.pop();
    Output { rust, map }
}

struct Codegen {
    out: String,
    indent: usize,
    tmp: u32,
    /// `>>>` s'appuie sur un petit masque de décalage logique, inséré dans le
    /// fichier généré seulement s'il sert.
    needs_ushr: bool,
    /// Trait courant, pour traduire `super.m(...)` en `Trait::m(self, ...)`.
    current_trait: Option<String>,
    /// Méthode courante : `super.m()` depuis `m` serait une récursion infinie.
    current_method: Option<String>,
    /// Ligne `.rava` en cours de traduction, reportée sur chaque ligne produite.
    cur_line: u32,
}

/// `import a.b.C;` -> `use a::b::C;`, préfixé de `crate::` si `a` est un
/// paquet du projet.
fn use_path(imp: &Import, ctx: &Context) -> String {
    let mut path = imp.path.join("::");
    if imp.glob {
        path.push_str("::*");
    }
    match imp.path.first() {
        Some(head) if ctx.crate_roots.iter().any(|r| r == head) => format!("crate::{path}"),
        _ => path,
    }
}

impl Codegen {
    // ------------------------------------------------------------- sortie

    fn line(&mut self, s: &str) {
        if !s.is_empty() {
            for _ in 0..self.indent {
                self.out.push_str("    ");
            }
            self.out.push_str(s);
            self.mark();
        }
        self.out.push('\n');
    }

    /// Étiquette la ligne en cours d'écriture avec son origine dans le `.rava`.
    fn mark(&mut self) {
        if self.cur_line > 0 {
            let _ = write!(self.out, "  {LINE_MARK}{}", self.cur_line);
        }
    }

    /// Fixe l'origine des lignes à venir, et rend la précédente.
    fn at(&mut self, span: Span) -> u32 {
        std::mem::replace(&mut self.cur_line, span.line)
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

    fn unit(&mut self, u: &Unit, ctx: &Context) -> R<()> {
        self.line("// Généré par ravac depuis un source Rava (syntaxe Java, sémantique Rust).");
        self.line("// Ne pas éditer : modifiez le fichier .rava correspondant.");
        // Les identifiants Rava sont repris tels quels : on garde le camelCase Java.
        self.line("#![allow(non_snake_case, non_camel_case_types, unused_parens, dead_code)]");
        self.blank();
        self.out.push_str(PRELUDE_MARKER);

        // En Java, le paquet est le seul espace de noms : les autres fichiers du
        // même paquet sont visibles sans import. En Rust, cela s'écrit ainsi.
        if ctx.in_package {
            self.line("use super::*;");
        }
        for imp in &u.imports {
            self.line(&format!("use {};", use_path(imp, ctx)));
        }
        if ctx.in_package || !u.imports.is_empty() {
            self.blank();
        }

        for item in &u.items {
            self.item(item)?;
            self.blank();
        }
        Ok(())
    }

    fn item(&mut self, item: &Item) -> R<()> {
        self.cur_line = match item {
            Item::Class(c) => c.span.line,
            Item::Interface(i) => i.span.line,
            Item::Enum(e) => e.span.line,
            Item::Record(r) => r.span.line,
        };
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
                self.cur_line = f.span.line;
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
            for m in &inherent {
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
            self.current_trait = Some(trait_name.clone());
            for m in &methods {
                self.method(m, MethodCtx::TraitImpl { owner: c, fields: &c.fields })?;
            }
            self.current_trait = None;
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
        // `pub` pour qu'un projet multi-fichiers puisse la réexporter depuis
        // la racine du crate — c'est ainsi que Rust accepte un `main` en module.
        self.open("pub fn main() {");
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
    ///
    /// `finalize()` est routé vers `impl Drop` : c'est ce que le programmeur
    /// Java voulait dire, et en Rust cela s'exécute de façon déterministe.
    fn split_methods(
        c: &Class,
        implements: &[Type],
    ) -> R<(Vec<Method>, Vec<(String, Vec<Method>)>)> {
        let mut inherent = Vec::new();
        let mut groups: Vec<(String, Vec<Method>)> = implements
            .iter()
            .map(|t| (render_path_only(t), Vec::new()))
            .collect();

        let push_to = |groups: &mut Vec<(String, Vec<Method>)>, target: String, m: Method| {
            let short = target.rsplit("::").next().unwrap_or(&target).to_string();
            match groups
                .iter_mut()
                .find(|(n, _)| *n == target || n.rsplit("::").next() == Some(short.as_str()))
            {
                Some((_, v)) => v.push(m),
                None => groups.push((target, vec![m])),
            }
        };

        for m in &c.methods {
            if Self::is_main(m) {
                continue;
            }
            // `protected void finalize()` -> `impl Drop for T { fn drop(&mut self) }`
            if m.name == "finalize"
                && m.params.is_empty()
                && matches!(m.ret.kind, TypeKind::Void)
                && !m.modifiers.contains(&Modifier::Static)
            {
                let mut d = m.clone();
                d.name = "drop".into();
                d.modifiers.retain(|x| !matches!(x, Modifier::Public | Modifier::Protected));
                if !has_annot(&d.annots, "Mut") {
                    d.annots.push(Annot { name: "Mut".into(), args: Vec::new(), span: m.span });
                }
                d.annots.retain(|a| a.name != "Override");
                push_to(&mut groups, "Drop".to_string(), d);
                continue;
            }
            let Some(ov) = find_annot(&m.annots, "Override") else {
                inherent.push(m.clone());
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
            push_to(&mut groups, target, m.clone());
        }
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
        for m in &inherent {
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
            self.current_trait = Some(trait_name.clone());
            for m in &methods {
                self.method(m, MethodCtx::TraitImpl { owner: &shell, fields: &shell.fields })?;
            }
            self.current_trait = None;
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
            for m in &inherent {
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
            self.current_trait = Some(trait_name.clone());
            for m in &methods {
                self.method(m, MethodCtx::TraitImpl { owner: &shell, fields: &[] })?;
            }
            self.current_trait = None;
            self.close("}");
        }
        Ok(())
    }

    fn const_decl(&mut self, k: &ConstDecl) -> R<()> {
        self.cur_line = k.span.line;
        self.attrs(&k.annots, None);
        let vis = Self::vis(&k.modifiers);
        let value = self.expr(&k.value)?;
        self.line(&format!("{vis}const {}: {} = {value};", k.name, self.type_(&k.ty)?));
        Ok(())
    }

    // ------------------------------------------------------------- méthodes

    fn method(&mut self, m: &Method, ctx: MethodCtx<'_>) -> R<()> {
        self.cur_line = m.span.line;
        if m.is_ctor {
            return self.constructor(m, ctx);
        }
        if m.modifiers.contains(&Modifier::Synchronized) {
            return err_fix(
                m.span,
                "`synchronized` n'a pas d'effet en Rust",
                "il n'y a pas de moniteur par objet : protégez la donnée avec `Mutex<T>` ou `RwLock<T>`. Voir docs/IMPOSSIBLE.md#concurrence",
                "rava.synchronized",
            );
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
                let outer = self.current_method.replace(m.name.clone());
                let r = self.body_stmts(b, !matches!(m.ret.kind, TypeKind::Void));
                self.current_method = outer;
                r?;
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
        self.cur_line = m.span.line;
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

        // Array<T, N> -> [T; N] (tableau de taille fixe)
        if last == "Array" && path.len() == 1 && args.len() == 2 {
            return Ok(format!("[{}; {}]", self.type_(&args[0])?, self.type_(&args[1])?));
        }
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
        if let Some(sp) = stmt_span(s) {
            self.cur_line = sp.line;
        }
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
            Stmt::Group(v) => {
                for s in v {
                    self.stmt(s)?;
                }
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

    /// Termine une ligne poussée à la main (`} else if … {`).
    fn end_raw_line(&mut self) {
        self.mark();
        self.out.push('\n');
    }

    /// Émet `if ... { }` à la suite d'un `} else ` déjà écrit sans saut de ligne.
    fn stmt_inline_if(&mut self, s: &Stmt) -> R<()> {
        let Stmt::If { cond, then, otherwise, span } = s else {
            return err(Span::default(), "if attendu");
        };
        let outer = self.at(*span);
        let c = self.expr(cond)?;
        self.out.push_str(&format!("if {c} {{"));
        self.end_raw_line();
        self.cur_line = outer;
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
                    "`super` seul n'a pas de sens en Rust",
                    "seul `super.methode(...)`, dans une classe qui implémente une interface, est traduit — en `Trait::methode(self, ...)`. Voir docs/IMPOSSIBLE.md#heritage",
                )
            }
            Expr::Name(path, _) => render_name(path),
            Expr::TypePath { path, args, .. } => {
                let mut parts = Vec::new();
                for a in args {
                    parts.push(self.type_(a)?);
                }
                format!("{}::<{}>", render_name(path), parts.join(", "))
            }
            Expr::Paren(inner, _) => format!("({})", self.expr(inner)?),
            Expr::Unary { op, expr, span } => {
                let inner = self.expr(expr)?;
                match op {
                    UnOp::Neg => format!("-{inner}"),
                    UnOp::Plus => inner,
                    UnOp::Not => format!("!{inner}"),
                    UnOp::BitNot => format!("!{inner}"),
                    UnOp::PreInc | UnOp::PreDec => {
                        let d = if *op == UnOp::PreInc { "+=" } else { "-=" };
                        let _ = span;
                        format!("{{ {inner} {d} 1; {inner} }}")
                    }
                    UnOp::PostInc | UnOp::PostDec => unreachable!(),
                }
            }
            // Rust n'a pas d'opérateur d'incrémentation : on produit le
            // bloc-expression que l'on écrirait à la main.
            Expr::PostIncDec { op, expr, .. } => {
                let place = self.expr(expr)?;
                let d = if *op == UnOp::PostInc { "+=" } else { "-=" };
                let t = self.fresh("post");
                format!("{{ let {t} = {place}; {place} {d} 1; {t} }}")
            }
            Expr::Binary { op, lhs, rhs, span } => {
                let _ = span;
                let l = self.expr(lhs)?;
                let r = self.expr(rhs)?;
                if *op == BinOp::UShr {
                    self.needs_ushr = true;
                    format!("__rava::UShr::ushr({l}, ({r}) as u32)")
                } else {
                    format!("{l} {} {r}", op.as_rust())
                }
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
            // `Arr.of(a, b, c)` -> `[a, b, c]` ; `Arr.fill(v, n)` -> `[v; n]`.
            if path.as_slice() == ["Arr"] && name == "of" {
                return Ok(format!("[{}]", rendered.join(", ")));
            }
            if path.as_slice() == ["Arr"] && name == "fill" && rendered.len() == 2 {
                return Ok(format!("[{}; {}]", rendered[0], rendered[1]));
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

        // `super.m(args)` : il n'y a pas de classe parente, mais il y a la
        // méthode par défaut du trait — c'est `Trait::m(self, args)`.
        if let Some(Expr::Super(sp)) = recv {
            // `Trait::m(self)` repasse par la table de dispatch : depuis `m`,
            // c'est un appel récursif, pas un appel au corps par défaut. Rust
            // n'offre aucun moyen d'atteindre une valeur par défaut redéfinie.
            if self.current_method.as_deref() == Some(name) {
                return err_note(
                    *sp,
                    format!("`super.{name}(...)` depuis `{name}` boucle à l'infini"),
                    "Rust ne permet pas d'appeler le corps par défaut d'une méthode qu'on redéfinit : `Trait::m(self)` repasse par votre propre implémentation. Extrayez la partie commune dans une autre méthode. Voir docs/IMPOSSIBLE.md#heritage",
                );
            }
            let Some(t) = self.current_trait.clone() else {
                return err_note(
                    *sp,
                    "`super` hors d'une implémentation d'interface",
                    "`super.m(...)` ne se traduit que dans une classe qui implémente une interface : il devient `Trait::m(self, ...)`. Voir docs/IMPOSSIBLE.md#heritage",
                );
            };
            let mut all = vec!["self".to_string()];
            all.extend(rendered);
            return Ok(format!("{t}::{name}{turbofish}({})", all.join(", ")));
        }

        Ok(match recv {
            None => format!("{name}{turbofish}({})", rendered.join(", ")),
            Some(r) => {
                let rs = self.expr(r)?;
                if matches!(r, Expr::TypePath { .. }) || is_type_name(&rs) {
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
        Lit::Unit => "()".to_string(),
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

/// Ligne source d'une instruction, quand elle en porte une.
fn stmt_span(s: &Stmt) -> Option<Span> {
    Some(match s {
        Stmt::Local { span, .. }
        | Stmt::LocalPattern { span, .. }
        | Stmt::Return(_, span)
        | Stmt::If { span, .. }
        | Stmt::While { span, .. }
        | Stmt::DoWhile { span, .. }
        | Stmt::For { span, .. }
        | Stmt::ForEach { span, .. }
        | Stmt::Loop { span, .. }
        | Stmt::Break(_, _, span)
        | Stmt::Continue(_, span)
        | Stmt::Unsafe(_, span) => *span,
        Stmt::Block(b) => b.span,
        Stmt::Group(v) => return v.first().and_then(stmt_span),
        Stmt::Switch(sw) => sw.span,
        Stmt::Expr(e) => e.span(),
        Stmt::Empty => return None,
    })
}

fn contains_continue(s: &Stmt) -> bool {
    match s {
        Stmt::Continue(..) => true,
        Stmt::Block(b) => b.stmts.iter().any(contains_continue),
        Stmt::Group(v) => v.iter().any(contains_continue),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Traduit un corps de méthode et renvoie les lignes de son bloc.
    fn body(src: &str) -> String {
        let unit = rava_parser::parse(&format!("class T {{ void f() {{ {src} }} }}"))
            .expect("syntaxe");
        let rust = generate(&unit).expect("génération");
        let start = rust.find("fn f(&self) {").expect("fn f") + "fn f(&self) {".len();
        let end = rust[start..].find("\n    }").expect("fin de f") + start;
        rust[start..end].trim().to_string()
    }

    fn fails(src: &str) -> Diag {
        let unit = rava_parser::parse(&format!("class T {{ void f() {{ {src} }} }}"))
            .expect("syntaxe");
        generate(&unit).expect_err("une erreur était attendue")
    }

    #[test]
    fn incrementation_en_expression_devient_un_bloc() {
        assert!(body("var a = i++;").starts_with("let a = { let __rava_post1 = i;"));
        assert_eq!(body("var a = ++i;"), "let a = { i += 1; i };");
    }

    #[test]
    fn incrementation_en_instruction_reste_simple() {
        assert_eq!(body("i++;"), "i += 1;");
    }

    #[test]
    fn ushr_passe_par_le_masque() {
        assert_eq!(body("var a = x >>> 2;"), "let a = __rava::UShr::ushr(x, (2) as u32);");
        let unit = rava_parser::parse("class T { void f() { var a = x >>> 2; } }").unwrap();
        assert!(generate(&unit).unwrap().contains("trait UShr"));
    }

    #[test]
    fn le_masque_ushr_nest_pas_emis_sil_ne_sert_pas() {
        let unit = rava_parser::parse("class T { void f() { var a = x >> 2; } }").unwrap();
        assert!(!generate(&unit).unwrap().contains("trait UShr"));
    }

    #[test]
    fn tableaux_de_taille_fixe() {
        assert_eq!(body("Array<i32, 3> a = Arr.of(1, 2, 3);"), "let a: [i32; 3] = [1, 2, 3];");
        assert_eq!(body("var a = Arr.fill(0, 4);"), "let a = [0; 4];");
    }

    #[test]
    fn arguments_de_type_portes_par_le_type() {
        assert_eq!(body("var v = Vec::<String>::new();"), "let v = Vec::<String>::new();");
    }

    #[test]
    fn finalize_devient_drop() {
        let unit = rava_parser::parse(
            "class T { protected void finalize() { g(); } }",
        )
        .unwrap();
        let rust = generate(&unit).unwrap();
        assert!(rust.contains("impl Drop for T {"), "{rust}");
        assert!(rust.contains("fn drop(&mut self) {"), "{rust}");
    }

    #[test]
    fn super_hors_interface_est_refuse() {
        let d = fails("var x = super.m();");
        assert!(d.note.unwrap().contains("implémente une interface"));
    }

    #[test]
    fn super_recursif_est_refuse() {
        let unit = rava_parser::parse(
            "interface I { default int m() { return 1; } }
             class T implements I { @Override public int m() { return super.m(); } }",
        )
        .unwrap();
        let d = generate(&unit).unwrap_err();
        assert!(d.message.contains("boucle à l'infini"), "{}", d.message);
    }

    #[test]
    fn synchronized_est_refuse_plutot_quignore() {
        let unit = rava_parser::parse("class T { public synchronized void f() { } }").unwrap();
        let d = generate(&unit).unwrap_err();
        assert!(d.note.unwrap().contains("Mutex"));
    }
}

#[cfg(test)]
mod layout_tests {
    use super::*;

    /// Comme en Java : l'indentation ne porte aucun sens, les instructions sont
    /// terminées par `;` et les blocs délimités par des accolades. Un programme
    /// écrit sur une ligne doit produire exactement le même Rust.
    #[test]
    fn lindentation_ne_change_rien() {
        let aere = "\
class Compte {

    i64 solde;

    @Mut
    public void crediter(i64 m) {
        unless (m > 0) {
            return;
        }
        this.solde += m;
    }
}";
        let compacte = "class Compte{i64 solde;@Mut public void crediter(i64 m){unless(m>0){return;}this.solde+=m;}}";
        let sur_plusieurs_lignes_absurdes = "\
        class
Compte
      {
i64
   solde
;
@Mut public
void crediter(i64
m) { unless
( m > 0 )
{ return
; } this
.solde
+= m ; } }";

        let gen = |src: &str| generate(&rava_parser::parse(src).unwrap()).unwrap();
        assert_eq!(gen(aere), gen(compacte));
        assert_eq!(gen(aere), gen(sur_plusieurs_lignes_absurdes));
    }

    /// L'inverse : un `;` manquant est une erreur, pas un saut de ligne implicite.
    #[test]
    fn un_point_virgule_manquant_est_une_erreur() {
        let e = rava_parser::parse("class A { void f() { var x = 1\n var y = 2; } }").unwrap_err();
        assert!(e.message.contains("`;`"), "{}", e.message);
    }

    /// Un retour à la ligne au milieu d'une expression ne la termine pas.
    #[test]
    fn une_expression_peut_courir_sur_plusieurs_lignes() {
        let src = "class A { i32 f() { return 1\n + 2\n + 3; } }";
        assert!(generate(&rava_parser::parse(src).unwrap()).unwrap().contains("1 + 2 + 3"));
    }
}

#[cfg(test)]
mod map_tests {
    use super::*;

    /// Chaque ligne générée doit pointer sur la ligne `.rava` qui l'a produite.
    #[test]
    fn la_table_suit_les_lignes() {
        let src = "\
class Compte {
    i64 solde;
    public void crediter(i64 m) {
        this.solde += m;
        g();
    }
}";
        let unit = rava_parser::parse(src).unwrap();
        let out = generate_with_map(&unit).unwrap();
        let find = |needle: &str| {
            let i = out.rust.lines().position(|l| l.contains(needle)).expect(needle);
            out.map[i]
        };
        assert_eq!(find("struct Compte"), 1);
        assert_eq!(find("solde: i64"), 2);
        assert_eq!(find("fn crediter"), 3);
        assert_eq!(find("self.solde += m"), 4);
        assert_eq!(find("g();"), 5);
    }

    /// Les bras de `match` sont assemblés en déplaçant du texte : le marqueur
    /// doit voyager avec sa ligne.
    #[test]
    fn la_table_survit_au_deplacement_des_blocs() {
        let src = "\
class A {
    void f(E e) {
        switch (e) {
            case E.X -> {
                un();
            }
            case E.Y -> deux();
        }
    }
}";
        let unit = rava_parser::parse(src).unwrap();
        let out = generate_with_map(&unit).unwrap();
        let i = out.rust.lines().position(|l| l.contains("un();")).unwrap();
        assert_eq!(out.map[i], 5);
    }

    #[test]
    fn aucun_marqueur_ne_subsiste_dans_le_rust() {
        for src in [
            "class A { void f() { if (a) { x(); } else if (b) { y(); } else { z(); } } }",
            "class A { void f() { var g = (x) -> { return x + 1; }; } }",
        ] {
            let unit = rava_parser::parse(src).unwrap();
            let out = generate_with_map(&unit).unwrap();
            assert!(!out.rust.contains(LINE_MARK), "{}", out.rust);
            assert_eq!(out.map.len(), out.rust.lines().count());
        }
    }

    /// Un `//~rava:` écrit par l'utilisateur ne doit pas être pris pour un marqueur.
    #[test]
    fn un_faux_marqueur_dans_une_chaine_est_preserve() {
        let unit = rava_parser::parse(
            r#"class A { void f() { var s = "voir //~rava:oui"; } }"#,
        )
        .unwrap();
        let out = generate_with_map(&unit).unwrap();
        assert!(out.rust.contains(r#""voir //~rava:oui""#), "{}", out.rust);
    }
}
