//! Parser Rava : grammaire Java, arbre destiné au codegen Rust.

use rava_ast::*;
use rava_lexer::{lex, TokKind, Token};

#[derive(Debug, Clone)]
pub struct ParseError {
    pub message: String,
    pub line: u32,
    pub col: u32,
    /// Renvoi vers la documentation quand la construction est volontairement absente.
    pub note: Option<String>,
    /// Identifiant stable, sur lequel les outils accrochent une correction.
    pub code: Option<&'static str>,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}: {}", self.line, self.col, self.message)?;
        if let Some(n) = &self.note {
            write!(f, "\n  note: {n}")?;
        }
        Ok(())
    }
}

type PResult<T> = Result<T, ParseError>;

pub fn parse(src: &str) -> PResult<Unit> {
    let toks = lex(src).map_err(|e| ParseError {
        message: e.message,
        line: e.span.line,
        col: e.span.col,
        note: None,
        code: None,
    })?;
    {
        let mut p = Parser { toks, i: 0, in_guard: 0, furthest: None };
        match p.unit() {
            Ok(u) => Ok(u),
            Err(e) => Err(match p.furthest {
                Some(f) if (f.line, f.col) > (e.line, e.col) => f,
                _ => e,
            }),
        }
    }
}

const MODIFIERS: &[&str] = &[
    "public",
    "private",
    "protected",
    "static",
    "final",
    "abstract",
    "default",
    "native",
    "synchronized",
    "transient",
    "volatile",
    "strictfp",
    "sealed",
];

/// Constructions Java sans contrepartie en Rust : on les refuse avec un renvoi
/// vers la documentation plutôt qu'avec une erreur de syntaxe opaque.
const UNSUPPORTED_KEYWORDS: &[(&str, &str)] = &[
    ("try", "Rust n'a pas d'exceptions. Utilisez Result<T, E> et l'opérateur `.q()`. Voir docs/IMPOSSIBLE.md#exceptions"),
    ("catch", "Rust n'a pas d'exceptions. Voir docs/IMPOSSIBLE.md#exceptions"),
    ("finally", "Rust n'a pas d'exceptions ; utilisez Drop. Voir docs/IMPOSSIBLE.md#exceptions"),
    ("throw", "Rust n'a pas de `throw`. Retournez un Err(...). Voir docs/IMPOSSIBLE.md#exceptions"),
    ("throws", "Rust n'a pas de `throws`. Déclarez le type de retour Result<T, E>. Voir docs/IMPOSSIBLE.md#exceptions"),
];

struct Parser {
    toks: Vec<Token>,
    i: usize,
    /// Dans une garde `case X when …`, `->` termine le motif : il ne peut donc
    /// pas ouvrir une lambda. `x == y -> …` se lit `x == y`, puis la flèche.
    in_guard: u32,
    /// L'erreur la plus avancée rencontrée, y compris dans une tentative
    /// abandonnée. La grammaire Java demande du retour arrière : sans cela, on
    /// rapporterait l'échec du repli plutôt que la vraie cause.
    furthest: Option<ParseError>,
}

impl Parser {
    // ------------------------------------------------------------- primitives

    fn tok(&self) -> &Token {
        &self.toks[self.i.min(self.toks.len() - 1)]
    }

    fn at(&self, off: usize) -> &Token {
        &self.toks[(self.i + off).min(self.toks.len() - 1)]
    }

    fn is_eof(&self) -> bool {
        self.tok().kind == TokKind::Eof
    }

    fn bump(&mut self) -> Token {
        let t = self.tok().clone();
        if !self.is_eof() {
            self.i += 1;
        }
        t
    }

    fn span(&self) -> Span {
        let s = self.tok().span;
        Span { line: s.line, col: s.col }
    }

    fn err<T>(&self, msg: impl Into<String>) -> PResult<T> {
        let s = self.tok().span;
        Err(ParseError { message: msg.into(), line: s.line, col: s.col, note: None, code: None })
    }

    fn err_note<T>(&self, msg: impl Into<String>, note: impl Into<String>) -> PResult<T> {
        let s = self.tok().span;
        Err(ParseError {
            message: msg.into(),
            line: s.line,
            col: s.col,
            note: Some(note.into()),
            code: None,
        })
    }

    /// Comme `err_note`, avec un code stable que les éditeurs peuvent traduire
    /// en correction rapide.
    fn err_fix<T>(
        &self,
        msg: impl Into<String>,
        note: impl Into<String>,
        code: &'static str,
    ) -> PResult<T> {
        let s = self.tok().span;
        Err(ParseError {
            message: msg.into(),
            line: s.line,
            col: s.col,
            note: Some(note.into()),
            code: Some(code),
        })
    }

    fn eat_punct(&mut self, p: &str) -> bool {
        if self.tok().is_punct(p) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn eat_kw(&mut self, k: &str) -> bool {
        if self.tok().is_ident(k) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn expect_punct(&mut self, p: &str) -> PResult<()> {
        if self.eat_punct(p) {
            Ok(())
        } else {
            self.err(format!("`{p}` attendu, trouvé `{}`", self.tok().text))
        }
    }

    fn expect_ident(&mut self) -> PResult<Ident> {
        if self.tok().kind == TokKind::Ident {
            Ok(self.bump().text)
        } else {
            self.err(format!("identifiant attendu, trouvé `{}`", self.tok().text))
        }
    }

    /// Consomme un `>` fermant, en scindant `>>`, `>>>` et `>=` au besoin.
    fn expect_gt(&mut self) -> PResult<()> {
        let t = self.tok().clone();
        if t.kind != TokKind::Punct || !t.text.starts_with('>') {
            return self.err(format!("`>` attendu, trouvé `{}`", t.text));
        }
        if t.text == ">" {
            self.bump();
        } else {
            // On remplace le token par sa queue : `>>` devient `>`.
            let rest = t.text[1..].to_string();
            self.toks[self.i].text = rest;
            self.toks[self.i].span.col += 1;
        }
        Ok(())
    }

    fn at_gt(&self) -> bool {
        self.tok().kind == TokKind::Punct && self.tok().text.starts_with('>')
    }

    /// Exécute `f` ; en cas d'échec, restaure la position et renvoie `None`.
    ///
    /// L'erreur abandonnée est retenue si elle va plus loin que les
    /// précédentes : c'est presque toujours celle qui décrit le vrai problème.
    fn attempt<T>(&mut self, f: impl FnOnce(&mut Self) -> PResult<T>) -> Option<T> {
        let save = self.i;
        match f(self) {
            Ok(v) => Some(v),
            Err(e) => {
                self.note_error(e);
                self.i = save;
                None
            }
        }
    }

    fn note_error(&mut self, e: ParseError) {
        // Une erreur sans note est un simple échec de forme ; on privilégie
        // celles qui expliquent, à position égale.
        let better = match &self.furthest {
            None => true,
            Some(f) => {
                (e.line, e.col) > (f.line, f.col)
                    || ((e.line, e.col) == (f.line, f.col)
                        && f.note.is_none()
                        && e.note.is_some())
            }
        };
        if better {
            self.furthest = Some(e);
        }
    }

    fn check_unsupported(&self) -> PResult<()> {
        if self.tok().kind == TokKind::Ident {
            if let Some((kw, note)) =
                UNSUPPORTED_KEYWORDS.iter().find(|(k, _)| *k == self.tok().text)
            {
                return self.err_note(format!("`{kw}` n'existe pas en Rava"), *note);
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------- unité

    fn unit(&mut self) -> PResult<Unit> {
        let mut package = None;
        let mut imports = Vec::new();
        let mut items = Vec::new();

        if self.tok().is_ident("package") {
            self.bump();
            package = Some(self.qualified_name()?);
            self.expect_punct(";")?;
        }

        while self.tok().is_ident("import") {
            let span = self.span();
            self.bump();
            let is_static = self.eat_kw("static");
            let mut path = vec![self.expect_ident()?];
            let mut glob = false;
            while self.eat_punct(".") {
                if self.eat_punct("*") {
                    glob = true;
                    break;
                }
                path.push(self.expect_ident()?);
            }
            self.expect_punct(";")?;
            imports.push(Import { path, glob, is_static, span });
        }

        while !self.is_eof() {
            if self.eat_punct(";") {
                continue;
            }
            items.push(self.item()?);
        }

        Ok(Unit { package, imports, items })
    }

    fn qualified_name(&mut self) -> PResult<Vec<Ident>> {
        let mut path = vec![self.expect_ident()?];
        while self.tok().is_punct(".") && self.at(1).kind == TokKind::Ident {
            self.bump();
            path.push(self.expect_ident()?);
        }
        Ok(path)
    }

    // ------------------------------------------------------------- annotations

    /// Javadoc (`/** */`) ou `///` en tête -> annotation `@Doc`, que le codegen
    /// transforme en commentaire de documentation Rust.
    fn leading_doc(&mut self) -> Vec<Annot> {
        let span = self.span();
        match self.tok().doc.clone() {
            Some(d) if !d.trim().is_empty() => {
                vec![Annot { name: "Doc".into(), args: vec![AnnotArg::Str(d)], span }]
            }
            _ => Vec::new(),
        }
    }

    fn annots(&mut self) -> PResult<Vec<Annot>> {
        let mut out = self.leading_doc();
        while self.tok().is_punct("@") && self.at(1).kind == TokKind::Ident {
            let span = self.span();
            self.bump();
            let name = self.expect_ident()?;
            let mut args = Vec::new();
            if self.eat_punct("(") {
                if !self.tok().is_punct(")") {
                    loop {
                        args.push(self.annot_arg()?);
                        if !self.eat_punct(",") {
                            break;
                        }
                    }
                }
                self.expect_punct(")")?;
            }
            out.push(Annot { name, args, span });
        }
        Ok(out)
    }

    fn annot_arg(&mut self) -> PResult<AnnotArg> {
        if self.eat_punct("{") {
            let mut items = Vec::new();
            while !self.tok().is_punct("}") {
                items.push(self.annot_arg()?);
                if !self.eat_punct(",") {
                    break;
                }
            }
            self.expect_punct("}")?;
            return Ok(AnnotArg::Array(items));
        }
        if self.tok().kind == TokKind::Ident && self.at(1).is_punct("=") {
            let key = self.bump().text;
            self.bump();
            return Ok(AnnotArg::Named(key, Box::new(self.annot_arg()?)));
        }
        let t = self.tok().clone();
        match t.kind {
            TokKind::StrLit | TokKind::TextBlock => {
                self.bump();
                Ok(AnnotArg::Str(t.text))
            }
            TokKind::IntLit | TokKind::FloatLit => {
                self.bump();
                Ok(AnnotArg::Num(t.text))
            }
            TokKind::Ident => {
                let path = self.qualified_name()?;
                // `@Derive(Debug.class)` : on ne garde que le nom du type.
                let name = if path.last().map(|s| s.as_str()) == Some("class") {
                    path[path.len().saturating_sub(2)].clone()
                } else {
                    path.join("::")
                };
                Ok(AnnotArg::Ident(name))
            }
            _ => self.err(format!("argument d'annotation invalide : `{}`", t.text)),
        }
    }

    fn modifiers(&mut self) -> Vec<Modifier> {
        let mut out = Vec::new();
        loop {
            let t = self.tok();
            if t.kind != TokKind::Ident {
                break;
            }
            let Some(m) = modifier_of(&t.text) else { break };
            // `sealed` et `default` sont contextuels : ce sont des identifiants
            // valides si un `(`, `.` ou `=` suit.
            if matches!(t.text.as_str(), "sealed" | "default")
                && !matches!(self.at(1).kind, TokKind::Ident)
            {
                break;
            }
            self.bump();
            out.push(m);
        }
        out
    }

    // ------------------------------------------------------------- items

    fn item(&mut self) -> PResult<Item> {
        let annots = self.annots()?;
        let modifiers = self.modifiers();
        self.item_after_head(annots, modifiers)
    }

    fn item_after_head(&mut self, annots: Vec<Annot>, modifiers: Vec<Modifier>) -> PResult<Item> {
        let t = self.tok().clone();
        match t.text.as_str() {
            "class" => Ok(Item::Class(self.class_decl(annots, modifiers)?)),
            "interface" => Ok(Item::Interface(self.interface_decl(annots, modifiers)?)),
            "enum" => Ok(Item::Enum(self.enum_decl(annots, modifiers)?)),
            "record" => Ok(Item::Record(self.record_decl(annots, modifiers)?)),
            _ => self.err(format!(
                "déclaration attendue (class, interface, enum, record), trouvé `{}`",
                t.text
            )),
        }
    }

    fn generic_params(&mut self) -> PResult<Generics> {
        let mut g = Generics::default();
        if !self.eat_punct("<") {
            return Ok(g);
        }
        loop {
            let annots = self.annots()?;
            let const_ty = if has_annot(&annots, "Const") {
                Some(self.type_()?)
            } else {
                None
            };
            let name = self.expect_ident()?;
            let mut bounds = Vec::new();
            if self.eat_kw("extends") {
                loop {
                    bounds.push(self.type_()?);
                    if !self.eat_punct("&") {
                        break;
                    }
                }
            }
            g.params.push(GenericParam { name, bounds, const_ty });
            if !self.eat_punct(",") {
                break;
            }
        }
        self.expect_gt()?;
        Ok(g)
    }

    /// Applique `@Lifetime` et `@Where` portées par la déclaration.
    fn apply_generic_annots(g: &mut Generics, annots: &[Annot]) {
        if let Some(a) = find_annot(annots, "Lifetime") {
            g.lifetimes.extend(a.texts());
        }
        if let Some(a) = find_annot(annots, "Where") {
            g.where_clauses.extend(a.texts());
        }
    }

    fn class_decl(&mut self, annots: Vec<Annot>, modifiers: Vec<Modifier>) -> PResult<Class> {
        let span = self.span();
        self.bump(); // class
        let name = self.expect_ident()?;
        let mut generics = self.generic_params()?;
        Self::apply_generic_annots(&mut generics, &annots);

        let mut extends = None;
        if self.eat_kw("extends") {
            extends = Some(self.type_()?);
        }
        let mut implements = Vec::new();
        if self.eat_kw("implements") || self.eat_kw("permits") {
            loop {
                implements.push(self.type_()?);
                if !self.eat_punct(",") {
                    break;
                }
            }
        }

        let mut class = Class {
            annots,
            modifiers,
            name: name.clone(),
            generics,
            implements,
            extends,
            fields: Vec::new(),
            methods: Vec::new(),
            consts: Vec::new(),
            assoc_types: Vec::new(),
            nested: Vec::new(),
            span,
        };
        self.expect_punct("{")?;
        while !self.tok().is_punct("}") && !self.is_eof() {
            self.member(&name, &mut class)?;
        }
        self.expect_punct("}")?;
        Ok(class)
    }

    fn record_decl(&mut self, annots: Vec<Annot>, modifiers: Vec<Modifier>) -> PResult<Record> {
        let span = self.span();
        self.bump(); // record
        let name = self.expect_ident()?;
        let mut generics = self.generic_params()?;
        Self::apply_generic_annots(&mut generics, &annots);
        self.expect_punct("(")?;
        let mut components = Vec::new();
        if !self.tok().is_punct(")") {
            loop {
                components.push(self.param()?);
                if !self.eat_punct(",") {
                    break;
                }
            }
        }
        self.expect_punct(")")?;
        let mut implements = Vec::new();
        if self.eat_kw("implements") {
            loop {
                implements.push(self.type_()?);
                if !self.eat_punct(",") {
                    break;
                }
            }
        }
        let mut shell = Class {
            annots: Vec::new(),
            modifiers: Vec::new(),
            name: name.clone(),
            generics: Generics::default(),
            implements: Vec::new(),
            extends: None,
            fields: Vec::new(),
            methods: Vec::new(),
            consts: Vec::new(),
            assoc_types: Vec::new(),
            nested: Vec::new(),
            span,
        };
        self.expect_punct("{")?;
        while !self.tok().is_punct("}") && !self.is_eof() {
            self.member(&name, &mut shell)?;
        }
        self.expect_punct("}")?;
        Ok(Record {
            annots,
            modifiers,
            name,
            generics,
            components,
            implements,
            methods: shell.methods,
            consts: shell.consts,
            assoc_types: shell.assoc_types,
            span,
        })
    }

    fn interface_decl(
        &mut self,
        annots: Vec<Annot>,
        modifiers: Vec<Modifier>,
    ) -> PResult<Interface> {
        let span = self.span();
        self.bump(); // interface
        let name = self.expect_ident()?;
        let mut generics = self.generic_params()?;
        Self::apply_generic_annots(&mut generics, &annots);
        let mut extends = Vec::new();
        if self.eat_kw("extends") {
            loop {
                extends.push(self.type_()?);
                if !self.eat_punct(",") {
                    break;
                }
            }
        }
        let mut shell = Class {
            annots: Vec::new(),
            modifiers: Vec::new(),
            name: name.clone(),
            generics: Generics::default(),
            implements: Vec::new(),
            extends: None,
            fields: Vec::new(),
            methods: Vec::new(),
            consts: Vec::new(),
            assoc_types: Vec::new(),
            nested: Vec::new(),
            span,
        };
        let mut assoc_types = Vec::new();
        self.expect_punct("{")?;
        while !self.tok().is_punct("}") && !self.is_eof() {
            if let Some(a) = self.attempt(Self::assoc_type) {
                assoc_types.push(a);
                continue;
            }
            self.member(&name, &mut shell)?;
        }
        self.expect_punct("}")?;
        Ok(Interface {
            annots,
            modifiers,
            name,
            generics,
            extends,
            methods: shell.methods,
            consts: shell.consts,
            assoc_types,
            span,
        })
    }

    /// `type Item extends Display;` dans une interface -> type associé Rust.
    fn assoc_type(&mut self) -> PResult<AssocType> {
        let span = self.span();
        let annots = self.annots()?;
        if !self.tok().is_ident("type") || self.at(1).kind != TokKind::Ident {
            return self.err("type associé attendu");
        }
        self.assoc_type_body(annots, span)
    }

    /// Partie commune : `type Nom [extends Bornes] [= Defaut];`
    fn assoc_type_body(&mut self, annots: Vec<Annot>, span: Span) -> PResult<AssocType> {
        self.bump(); // type
        let name = self.expect_ident()?;
        let mut bounds = Vec::new();
        if self.eat_kw("extends") {
            loop {
                bounds.push(self.type_()?);
                if !self.eat_punct("&") {
                    break;
                }
            }
        }
        let default = if self.eat_punct("=") { Some(self.type_()?) } else { None };
        self.expect_punct(";")?;
        Ok(AssocType { annots, name, bounds, default, span })
    }

    fn enum_decl(&mut self, annots: Vec<Annot>, modifiers: Vec<Modifier>) -> PResult<EnumDecl> {
        let span = self.span();
        self.bump(); // enum
        let name = self.expect_ident()?;
        let mut generics = self.generic_params()?;
        Self::apply_generic_annots(&mut generics, &annots);
        let mut implements = Vec::new();
        if self.eat_kw("implements") {
            loop {
                implements.push(self.type_()?);
                if !self.eat_punct(",") {
                    break;
                }
            }
        }
        self.expect_punct("{")?;

        let mut variants = Vec::new();
        while !self.tok().is_punct(";") && !self.tok().is_punct("}") && !self.is_eof() {
            let vspan = self.span();
            let vannots = self.annots()?;
            let vname = self.expect_ident()?;
            let mut payload = Vec::new();
            if self.eat_punct("(") {
                if !self.tok().is_punct(")") {
                    loop {
                        payload.push(self.param()?);
                        if !self.eat_punct(",") {
                            break;
                        }
                    }
                }
                self.expect_punct(")")?;
            }
            let discriminant = if self.eat_punct("=") { Some(self.expr()?) } else { None };
            variants.push(Variant { annots: vannots, name: vname, payload, discriminant, span: vspan });
            if !self.eat_punct(",") {
                break;
            }
        }

        let mut shell = Class {
            annots: Vec::new(),
            modifiers: Vec::new(),
            name: name.clone(),
            generics: Generics::default(),
            implements: Vec::new(),
            extends: None,
            fields: Vec::new(),
            methods: Vec::new(),
            consts: Vec::new(),
            assoc_types: Vec::new(),
            nested: Vec::new(),
            span,
        };
        if self.eat_punct(";") {
            while !self.tok().is_punct("}") && !self.is_eof() {
                self.member(&name, &mut shell)?;
            }
        }
        self.expect_punct("}")?;
        Ok(EnumDecl {
            annots,
            modifiers,
            name,
            generics,
            implements,
            variants,
            methods: shell.methods,
            consts: shell.consts,
            assoc_types: shell.assoc_types,
            span,
        })
    }

    /// Membre de corps de type : champ, constante, constructeur, méthode ou type imbriqué.
    fn member(&mut self, owner: &str, out: &mut Class) -> PResult<()> {
        if self.eat_punct(";") {
            return Ok(());
        }
        let span = self.span();
        let annots = self.annots()?;
        let modifiers = self.modifiers();

        if matches!(self.tok().text.as_str(), "class" | "interface" | "enum" | "record") {
            out.nested.push(self.item_after_head(annots, modifiers)?);
            return Ok(());
        }

        // `type Cle = String;` : type associé fourni à un trait implémenté.
        if self.tok().is_ident("type") && self.at(1).kind == TokKind::Ident {
            out.assoc_types.push(self.assoc_type_body(annots, span)?);
            return Ok(());
        }

        let generics_head = if self.tok().is_punct("<") {
            self.generic_params()?
        } else {
            Generics::default()
        };

        // Constructeur : `Owner(` sans type de retour.
        if self.tok().is_ident(owner) && self.at(1).is_punct("(") {
            let ctor_name = self.expect_ident()?;
            let mut m = self.method_rest(
                annots,
                modifiers,
                generics_head,
                Type { annots: Vec::new(), kind: TypeKind::Void, span },
                ctor_name,
                span,
            )?;
            m.is_ctor = true;
            out.methods.push(m);
            return Ok(());
        }

        let ty = self.type_()?;
        let name = self.expect_ident()?;

        if self.tok().is_punct("(") {
            let m = self.method_rest(annots, modifiers, generics_head, ty, name, span)?;
            out.methods.push(m);
            return Ok(());
        }

        // Champ ou constante ; `int a = 1, b = 2;` autorisé.
        let mut cur_name = name;
        loop {
            let init = if self.eat_punct("=") { Some(self.expr()?) } else { None };
            let is_const = modifiers.contains(&Modifier::Static)
                && modifiers.contains(&Modifier::Final);
            if is_const {
                let Some(value) = init else {
                    return self.err("une constante `static final` doit être initialisée");
                };
                out.consts.push(ConstDecl {
                    annots: annots.clone(),
                    modifiers: modifiers.clone(),
                    ty: ty.clone(),
                    name: cur_name,
                    value,
                    span,
                });
            } else {
                out.fields.push(Field {
                    annots: annots.clone(),
                    modifiers: modifiers.clone(),
                    ty: ty.clone(),
                    name: cur_name,
                    init,
                    span,
                });
            }
            if self.eat_punct(",") {
                cur_name = self.expect_ident()?;
                continue;
            }
            break;
        }
        self.expect_punct(";")?;
        Ok(())
    }

    fn method_rest(
        &mut self,
        annots: Vec<Annot>,
        modifiers: Vec<Modifier>,
        mut generics: Generics,
        ret: Type,
        name: Ident,
        span: Span,
    ) -> PResult<Method> {
        Self::apply_generic_annots(&mut generics, &annots);
        self.expect_punct("(")?;
        let mut params = Vec::new();
        if !self.tok().is_punct(")") {
            loop {
                params.push(self.param()?);
                if !self.eat_punct(",") {
                    break;
                }
            }
        }
        self.expect_punct(")")?;
        self.check_unsupported()?;
        let body = if self.tok().is_punct("{") {
            Some(self.block()?)
        } else {
            self.expect_punct(";")?;
            None
        };
        Ok(Method { annots, modifiers, generics, ret, name, params, body, is_ctor: false, span })
    }

    fn param(&mut self) -> PResult<Param> {
        let span = self.span();
        let annots = self.annots()?;
        let _ = self.eat_kw("final");
        let mut ty = self.type_()?;
        // `@Ref String s` : l'annotation est écrite devant le paramètre mais
        // porte sur le type. On la propage pour que le codegen la trouve là.
        let mut merged = annots.clone();
        merged.append(&mut ty.annots);
        ty.annots = merged;
        let varargs = self.eat_punct("...");
        let name = self.expect_ident()?;
        Ok(Param { annots, ty, name, varargs, span })
    }

    // ------------------------------------------------------------- types

    fn type_(&mut self) -> PResult<Type> {
        let span = self.span();
        let annots = self.annots()?;
        let t = self.tok().clone();
        if t.kind != TokKind::Ident {
            return self.err(format!("type attendu, trouvé `{}`", t.text));
        }
        let kind = match t.text.as_str() {
            "void" => {
                self.bump();
                TypeKind::Void
            }
            "var" => {
                self.bump();
                TypeKind::Infer
            }
            _ => {
                let path = self.qualified_name()?;
                let args = self.type_args()?;
                TypeKind::Named { path, args }
            }
        };
        let mut ty = Type { annots, kind, span };
        while self.tok().is_punct("[") && self.at(1).is_punct("]") {
            self.bump();
            self.bump();
            ty = Type {
                annots: Vec::new(),
                kind: TypeKind::Array(Box::new(ty)),
                span,
            };
        }
        Ok(ty)
    }

    fn type_args(&mut self) -> PResult<Vec<Type>> {
        if !self.tok().is_punct("<") {
            return Ok(Vec::new());
        }
        let save = self.i;
        self.bump();
        if self.at_gt() {
            // Diamant `<>` : rien à propager, Rust infère.
            self.expect_gt()?;
            return Ok(Vec::new());
        }
        let mut args = Vec::new();
        loop {
            if self.tok().is_punct("?") {
                self.i = save;
                return self.err_note(
                    "les jokers génériques `?` n'existent pas en Rust",
                    "utilisez un paramètre de type borné, `impl Trait` ou `dyn Trait`. Voir docs/IMPOSSIBLE.md#wildcards",
                );
            }
            // Argument générique constant : `Tampon<8>`.
            if self.tok().kind == TokKind::IntLit {
                let t = self.bump();
                args.push(Type {
                    annots: Vec::new(),
                    kind: TypeKind::Named { path: vec![t.text], args: Vec::new() },
                    span: Span { line: t.span.line, col: t.span.col },
                });
                if self.eat_punct(",") {
                    continue;
                }
                break;
            }
            match self.type_() {
                Ok(t) => args.push(t),
                Err(e) => {
                    self.i = save;
                    return Err(e);
                }
            }
            if self.eat_punct(",") {
                continue;
            }
            break;
        }
        if !self.at_gt() {
            self.i = save;
            return self.err("`>` attendu à la fin des arguments génériques");
        }
        self.expect_gt()?;
        Ok(args)
    }

    // ------------------------------------------------------------- statements

    fn block(&mut self) -> PResult<Block> {
        let span = self.span();
        self.expect_punct("{")?;
        let mut stmts = Vec::new();
        while !self.tok().is_punct("}") && !self.is_eof() {
            stmts.push(self.stmt()?);
        }
        self.expect_punct("}")?;
        Ok(Block { stmts, span })
    }

    fn stmt(&mut self) -> PResult<Stmt> {
        self.check_unsupported()?;
        let span = self.span();

        if self.eat_punct(";") {
            return Ok(Stmt::Empty);
        }

        // Étiquette de boucle : `outer: for (...)`.
        let label = if self.tok().kind == TokKind::Ident
            && self.at(1).is_punct(":")
            && matches!(self.at(2).text.as_str(), "for" | "while" | "do" | "loop")
        {
            let l = self.bump().text;
            self.bump();
            Some(l)
        } else {
            None
        };

        // `@Unsafe { ... }` : bloc unsafe.
        if self.tok().is_punct("@") {
            let save = self.i;
            let annots = self.annots()?;
            if self.tok().is_punct("{") && has_annot(&annots, "Unsafe") {
                return Ok(Stmt::Unsafe(self.block()?, span));
            }
            self.i = save;
        }

        if self.tok().is_punct("{") {
            return Ok(Stmt::Block(self.block()?));
        }

        match self.tok().text.as_str() {
            "if" | "unless" => {
                // `unless (c)` est exactement `if (!c)` : on nie la condition ici,
                // le reste de la chaîne (`else`, `else unless`) suit les mêmes règles.
                let negated = self.tok().text == "unless";
                self.bump();
                self.expect_punct("(")?;
                let cond = self.expr()?;
                self.expect_punct(")")?;
                let cond = if negated { negate(cond) } else { cond };
                let then = Box::new(self.stmt()?);
                let otherwise = if self.eat_kw("else") {
                    // `else unless (c)` : `self.stmt()` reconnaît `unless`, ce qui
                    // produit la même chaîne `else if` qu'un `else if` classique.
                    Some(Box::new(self.stmt()?))
                } else {
                    None
                };
                return Ok(Stmt::If { cond, then, otherwise, span });
            }
            "while" => {
                self.bump();
                self.expect_punct("(")?;
                let cond = self.expr()?;
                self.expect_punct(")")?;
                let body = Box::new(self.stmt()?);
                // `while (true)` est la boucle infinie de Rust.
                if matches!(&cond, Expr::Lit(Lit::Bool(true), _)) {
                    return Ok(Stmt::Loop { label, body, span });
                }
                return Ok(Stmt::While { label, cond, body, span });
            }
            "do" => {
                self.bump();
                let body = Box::new(self.stmt()?);
                if !self.eat_kw("while") {
                    return self.err("`while` attendu après le corps du `do`");
                }
                self.expect_punct("(")?;
                let cond = self.expr()?;
                self.expect_punct(")")?;
                self.expect_punct(";")?;
                return Ok(Stmt::DoWhile { label, body, cond, span });
            }
            "for" => return self.for_stmt(label, span),
            "return" => {
                self.bump();
                let e = if self.tok().is_punct(";") { None } else { Some(self.expr()?) };
                self.expect_punct(";")?;
                return Ok(Stmt::Return(e, span));
            }
            "break" => {
                self.bump();
                let mut lbl = None;
                let mut value = None;
                if self.tok().kind == TokKind::Ident && !self.tok().is_punct(";") {
                    // `break label;` ou `break value;`
                    if self.at(1).is_punct(";") && looks_like_label(&self.tok().text) {
                        lbl = Some(self.bump().text);
                    } else {
                        value = Some(self.expr()?);
                    }
                } else if !self.tok().is_punct(";") {
                    value = Some(self.expr()?);
                }
                self.expect_punct(";")?;
                return Ok(Stmt::Break(lbl, value, span));
            }
            "continue" => {
                self.bump();
                let lbl = if self.tok().kind == TokKind::Ident {
                    Some(self.bump().text)
                } else {
                    None
                };
                self.expect_punct(";")?;
                return Ok(Stmt::Continue(lbl, span));
            }
            "switch" => {
                let sw = self.switch_expr()?;
                self.eat_punct(";");
                return Ok(Stmt::Switch(sw));
            }
            "loop" if self.at(1).is_punct("{") => {
                self.bump();
                let body = Box::new(Stmt::Block(self.block()?));
                return Ok(Stmt::Loop { label, body, span });
            }
            "class" | "interface" | "enum" | "record" => {
                return self.err_note(
                    "déclaration de type dans un corps de méthode",
                    "déplacez-la au niveau du fichier ; Rust autorise les items imbriqués, mais Rava les remonte explicitement",
                )
            }
            _ => {}
        }

        // Déclaration locale ou expression : on tente la déclaration d'abord.
        if let Some(s) = self.attempt(Self::local_decl) {
            return Ok(s);
        }
        let e = self.expr()?;
        self.expect_punct(";")?;
        Ok(Stmt::Expr(e))
    }

    fn local_decl(&mut self) -> PResult<Stmt> {
        let span = self.span();
        let annots = self.annots()?;
        let modifiers = self.modifiers();
        let ty = self.type_()?;

        // Déstructuration : `var Point(var x, var y) = p;`
        if matches!(ty.kind, TypeKind::Infer) && self.tok().kind == TokKind::Ident {
            let save = self.i;
            let candidate = self.attempt(|p| {
                let pat = p.pattern()?;
                if !p.tok().is_punct("=") {
                    return p.err("`=` attendu");
                }
                Ok(pat)
            });
            match candidate {
                Some(pat @ (Pattern::TupleStruct { .. } | Pattern::Struct { .. })) => {
                    self.expect_punct("=")?;
                    let init = self.expr()?;
                    self.expect_punct(";")?;
                    return Ok(Stmt::LocalPattern { annots, pat, init, span });
                }
                // Simple identifiant : c'est une déclaration ordinaire, on rembobine.
                _ => self.i = save,
            }
        }

        let name = self.expect_ident()?;
        let init = if self.eat_punct("=") { Some(self.expr()?) } else { None };
        if !self.tok().is_punct(";") {
            return self.err("`;` attendu");
        }
        self.bump();
        Ok(Stmt::Local { annots, modifiers, ty, name, init, span })
    }

    fn for_stmt(&mut self, label: Option<Ident>, span: Span) -> PResult<Stmt> {
        self.bump(); // for
        self.expect_punct("(")?;

        // `for (;;)` -> boucle infinie.
        if self.tok().is_punct(";") && self.at(1).is_punct(";") && self.at(2).is_punct(")") {
            self.bump();
            self.bump();
            self.bump();
            let body = Box::new(self.stmt()?);
            return Ok(Stmt::Loop { label, body, span });
        }

        // for-each : `for (var x : it)`
        if let Some(head) = self.attempt(|p| {
            let annots = p.annots()?;
            let _ = p.eat_kw("final");
            let ty = p.type_()?;
            let name = p.expect_ident()?;
            if !p.eat_punct(":") {
                return p.err("`:` attendu");
            }
            let iter = p.expr()?;
            p.expect_punct(")")?;
            Ok((annots, ty, name, iter))
        }) {
            let (annots, ty, name, iter) = head;
            let body = Box::new(self.stmt()?);
            return Ok(Stmt::ForEach { label, annots, ty, name, iter, body, span });
        }

        let mut init = Vec::new();
        if !self.tok().is_punct(";") {
            if let Some(d) = self.attempt(Self::local_decl) {
                init.push(d);
            } else {
                loop {
                    init.push(Stmt::Expr(self.expr()?));
                    if !self.eat_punct(",") {
                        break;
                    }
                }
                self.expect_punct(";")?;
            }
        } else {
            self.bump();
        }
        let cond = if self.tok().is_punct(";") { None } else { Some(self.expr()?) };
        self.expect_punct(";")?;
        let mut update = Vec::new();
        if !self.tok().is_punct(")") {
            loop {
                update.push(self.expr()?);
                if !self.eat_punct(",") {
                    break;
                }
            }
        }
        self.expect_punct(")")?;
        let body = Box::new(self.stmt()?);
        Ok(Stmt::For { label, init, cond, update, body, span })
    }

    // ------------------------------------------------------------- switch / match

    fn switch_expr(&mut self) -> PResult<SwitchExpr> {
        let span = self.span();
        self.bump(); // switch
        self.expect_punct("(")?;
        let scrutinee = Box::new(self.expr()?);
        self.expect_punct(")")?;
        self.expect_punct("{")?;
        let mut arms = Vec::new();
        while !self.tok().is_punct("}") && !self.is_eof() {
            let aspan = self.span();
            let mut patterns = Vec::new();
            if self.eat_kw("default") {
                patterns.push(Pattern::Wildcard(aspan));
            } else {
                if !self.eat_kw("case") {
                    return self.err(format!(
                        "`case` ou `default` attendu, trouvé `{}`",
                        self.tok().text
                    ));
                }
                loop {
                    patterns.push(self.pattern()?);
                    if !self.eat_punct(",") {
                        break;
                    }
                }
            }
            let guard = if self.eat_kw("when") {
                self.in_guard += 1;
                let g = self.expr();
                self.in_guard -= 1;
                Some(g?)
            } else {
                None
            };
            if self.tok().is_punct(":") {
                return self.err_fix(
                    "la forme `case X:` du switch n'est pas supportée",
                    "utilisez la forme flèche `case X -> ...;` : elle se traduit directement en bras de `match`. Voir docs/SYNTAX.md#switch",
                    "rava.case-colon",
                );
            }
            self.expect_punct("->")?;
            let body = if self.tok().is_punct("{") {
                SwitchArmBody::Block(self.block()?)
            } else {
                let e = self.expr()?;
                self.expect_punct(";")?;
                SwitchArmBody::Expr(e)
            };
            arms.push(SwitchArm { patterns, guard, body, span: aspan });
        }
        self.expect_punct("}")?;
        Ok(SwitchExpr { scrutinee, arms, span })
    }

    fn pattern(&mut self) -> PResult<Pattern> {
        let span = self.span();
        let annots = self.annots()?;
        let t = self.tok().clone();

        match t.kind {
            TokKind::IntLit | TokKind::FloatLit | TokKind::StrLit | TokKind::CharLit => {
                let lit = self.literal()?;
                if self.tok().is_punct("...") {
                    self.bump();
                    let end = self.pattern()?;
                    return Ok(Pattern::Range {
                        start: Box::new(Pattern::Lit(lit, span)),
                        end: Box::new(end),
                        inclusive: true,
                        span,
                    });
                }
                return Ok(Pattern::Lit(lit, span));
            }
            TokKind::Punct if t.text == "-" => {
                self.bump();
                let lit = self.literal()?;
                let neg = match lit {
                    Lit::Int(s) => Lit::Int(format!("-{s}")),
                    Lit::Float(s) => Lit::Float(format!("-{s}")),
                    other => other,
                };
                return Ok(Pattern::Lit(neg, span));
            }
            _ => {}
        }

        if self.tok().is_ident("default") || self.tok().is_ident("_") {
            self.bump();
            return Ok(Pattern::Wildcard(span));
        }
        if self.tok().is_ident("true") || self.tok().is_ident("false") {
            let b = self.bump().text == "true";
            return Ok(Pattern::Lit(Lit::Bool(b), span));
        }
        if self.eat_kw("var") {
            let name = self.expect_ident()?;
            return Ok(self.wrap_ref(annots, Pattern::Binding { annots: Vec::new(), name, span }, span));
        }

        let path = self.qualified_name()?;
        // Motif de déconstruction : `Some(var v)` / `Point(var x, var y)`
        if self.tok().is_punct("(") {
            self.bump();
            let mut elems = Vec::new();
            let mut named: Vec<(Ident, Pattern)> = Vec::new();
            let mut rest = false;
            if !self.tok().is_punct(")") {
                loop {
                    if self.eat_punct("...") {
                        rest = true;
                        break;
                    }
                    // Champ nommé : `x = var a`
                    if self.tok().kind == TokKind::Ident && self.at(1).is_punct("=") {
                        let f = self.bump().text;
                        self.bump();
                        named.push((f, self.pattern()?));
                    } else {
                        elems.push(self.pattern()?);
                    }
                    if !self.eat_punct(",") {
                        break;
                    }
                }
            }
            self.expect_punct(")")?;
            let p = if named.is_empty() && !rest {
                Pattern::TupleStruct { path, elems, span }
            } else {
                Pattern::Struct { path, fields: named, rest, span }
            };
            return Ok(self.wrap_ref(annots, p, span));
        }
        // Motif de type : `case Circle c ->` (liaison typée) ou chemin nu.
        if self.tok().kind == TokKind::Ident && !self.tok().is_ident("when") {
            let name = self.bump().text;
            return Ok(self.wrap_ref(annots, Pattern::Binding { annots: Vec::new(), name, span }, span));
        }
        Ok(self.wrap_ref(annots, Pattern::Path(path, span), span))
    }

    fn wrap_ref(&self, annots: Vec<Annot>, inner: Pattern, span: Span) -> Pattern {
        if has_annot(&annots, "Ref") {
            Pattern::Ref { mutable: has_annot(&annots, "Mut"), inner: Box::new(inner), span }
        } else {
            inner
        }
    }

    fn literal(&mut self) -> PResult<Lit> {
        let t = self.bump();
        Ok(match t.kind {
            TokKind::IntLit => Lit::Int(t.text),
            TokKind::FloatLit => Lit::Float(t.text),
            TokKind::StrLit | TokKind::TextBlock => Lit::Str(t.text),
            TokKind::CharLit => Lit::Char(t.text),
            _ => return self.err(format!("littéral attendu, trouvé `{}`", t.text)),
        })
    }

    // ------------------------------------------------------------- expressions

    pub fn expr(&mut self) -> PResult<Expr> {
        self.assignment()
    }

    fn assignment(&mut self) -> PResult<Expr> {
        let span = self.span();
        let lhs = self.ternary()?;
        let t = self.tok().clone();
        if t.kind == TokKind::Punct {
            let op = match t.text.as_str() {
                "=" => Some(None),
                "+=" => Some(Some(BinOp::Add)),
                "-=" => Some(Some(BinOp::Sub)),
                "*=" => Some(Some(BinOp::Mul)),
                "/=" => Some(Some(BinOp::Div)),
                "%=" => Some(Some(BinOp::Rem)),
                "&=" => Some(Some(BinOp::BitAnd)),
                "|=" => Some(Some(BinOp::BitOr)),
                "^=" => Some(Some(BinOp::BitXor)),
                "<<=" => Some(Some(BinOp::Shl)),
                ">>=" => Some(Some(BinOp::Shr)),
                ">>>=" => Some(Some(BinOp::UShr)),
                _ => None,
            };
            if let Some(op) = op {
                self.bump();
                let value = self.assignment()?;
                return Ok(Expr::Assign {
                    op,
                    target: Box::new(lhs),
                    value: Box::new(value),
                    span,
                });
            }
        }
        Ok(lhs)
    }

    fn ternary(&mut self) -> PResult<Expr> {
        let span = self.span();
        let cond = self.binary(0)?;
        if self.eat_punct("?") {
            let then = self.assignment()?;
            self.expect_punct(":")?;
            let otherwise = self.assignment()?;
            return Ok(Expr::Ternary {
                cond: Box::new(cond),
                then: Box::new(then),
                otherwise: Box::new(otherwise),
                span,
            });
        }
        Ok(cond)
    }

    fn binary(&mut self, min_prec: u8) -> PResult<Expr> {
        let span = self.span();
        // `not` est un opérateur préfixe : il lie plus lâche que les comparaisons
        // (`not a == b` vaut `not (a == b)`) mais plus serré que `and`/`or`.
        let mut lhs = if self.tok().is_ident("not") {
            self.bump();
            let operand = self.binary(PREC_NOT_OPERAND)?;
            negate(operand)
        } else {
            self.unary()?
        };
        loop {
            // Opérateurs logiques en toutes lettres : and, or, xor, nand, nor,
            // xnor, implies. Ils sont désucrés ici, le codegen n'a rien à savoir.
            if let Some((w, prec)) = self.peek_word_op() {
                if prec < min_prec {
                    break;
                }
                self.bump();
                // `implies` est associatif à droite : a implies b implies c
                // se lit a implies (b implies c), comme en logique.
                let rhs = if w == WordOp::Implies {
                    self.binary(prec)?
                } else {
                    self.binary(prec + 1)?
                };
                lhs = build_word_op(w, lhs, rhs, span);
                continue;
            }
            // `x instanceof Foo` a la précédence des opérateurs relationnels.
            if self.tok().is_ident("instanceof") && PREC_RELATIONAL >= min_prec {
                self.bump();
                let _ = self.eat_kw("final");
                let ty = self.type_()?;
                let binding = if self.tok().kind == TokKind::Ident
                    && !MODIFIERS.contains(&self.tok().text.as_str())
                {
                    Some(self.bump().text)
                } else {
                    None
                };
                lhs = Expr::InstanceOf { expr: Box::new(lhs), ty, binding, span };
                continue;
            }
            let Some((op, prec)) = self.peek_binop() else { break };
            if prec < min_prec {
                break;
            }
            self.consume_binop(op);
            let rhs = self.binary(prec + 1)?;
            lhs = Expr::Binary { op, lhs: Box::new(lhs), rhs: Box::new(rhs), span };
        }
        Ok(lhs)
    }

    fn peek_binop(&self) -> Option<(BinOp, u8)> {
        if self.tok().kind != TokKind::Punct {
            return None;
        }
        let op = match self.tok().text.as_str() {
            "||" => (BinOp::Or, 5),
            "&&" => (BinOp::And, 6),
            "|" => (BinOp::BitOr, 7),
            "^" => (BinOp::BitXor, 8),
            "&" => (BinOp::BitAnd, 9),
            "==" => (BinOp::Eq, 10),
            "!=" => (BinOp::Ne, 10),
            "<" => (BinOp::Lt, PREC_RELATIONAL),
            "<=" => (BinOp::Le, PREC_RELATIONAL),
            ">" => (BinOp::Gt, PREC_RELATIONAL),
            ">=" => (BinOp::Ge, PREC_RELATIONAL),
            "<<" => (BinOp::Shl, 12),
            ">>" => (BinOp::Shr, 12),
            ">>>" => (BinOp::UShr, 12),
            "+" => (BinOp::Add, 13),
            "-" => (BinOp::Sub, 13),
            "*" => (BinOp::Mul, 14),
            "/" => (BinOp::Div, 14),
            "%" => (BinOp::Rem, 14),
            _ => return None,
        };
        Some(op)
    }

    /// Opérateurs logiques littéraux, du plus lâche au plus serré :
    /// `implies` < `or`/`nor` < `xor`/`xnor` < `and`/`nand`, comme en logique.
    fn peek_word_op(&self) -> Option<(WordOp, u8)> {
        if self.tok().kind != TokKind::Ident {
            return None;
        }
        Some(match self.tok().text.as_str() {
            "implies" => (WordOp::Implies, 1),
            "or" => (WordOp::Or, 2),
            "nor" => (WordOp::Nor, 2),
            "xor" => (WordOp::Xor, 3),
            "xnor" => (WordOp::Xnor, 3),
            "and" => (WordOp::And, 4),
            "nand" => (WordOp::Nand, 4),
            _ => return None,
        })
    }

    fn consume_binop(&mut self, _op: BinOp) {
        self.bump();
    }

    fn unary(&mut self) -> PResult<Expr> {
        let span = self.span();
        let t = self.tok().clone();
        if t.kind == TokKind::Punct {
            let op = match t.text.as_str() {
                "-" => Some(UnOp::Neg),
                "+" => Some(UnOp::Plus),
                "!" => Some(UnOp::Not),
                "~" => Some(UnOp::BitNot),
                "++" => Some(UnOp::PreInc),
                "--" => Some(UnOp::PreDec),
                _ => None,
            };
            if let Some(op) = op {
                self.bump();
                let e = self.unary()?;
                return Ok(Expr::Unary { op, expr: Box::new(e), span });
            }
        }
        self.postfix()
    }

    fn postfix(&mut self) -> PResult<Expr> {
        let mut e = self.primary()?;
        loop {
            let span = self.span();
            if self.tok().is_punct(".") {
                // `.await` (extension) : `.` suivi de `await` sans parenthèses.
                if self.at(1).is_ident("await") && !self.at(2).is_punct("(") {
                    self.bump();
                    self.bump();
                    e = Expr::Await(Box::new(e), span);
                    continue;
                }
                self.bump();
                // Arguments génériques explicites : `obj.<T>m(...)`
                let generics = if self.tok().is_punct("<") {
                    self.type_args()?
                } else {
                    Vec::new()
                };
                let name = self.expect_ident()?;
                // Turbofish Rust `x.parse::<i32>()` : extension hors syntaxe Java,
                // équivalente à la forme stricte `x.<i32>parse()`.
                let generics = if generics.is_empty()
                    && self.tok().is_punct("::")
                    && self.at(1).is_punct("<")
                {
                    self.bump();
                    self.type_args()?
                } else {
                    generics
                };
                if self.tok().is_punct("(") {
                    let args = self.call_args()?;
                    e = match (name.as_str(), args.len()) {
                        // Pseudo-méthodes : équivalents Java stricts des opérateurs Rust.
                        ("q", 0) => Expr::Try(Box::new(e), span),
                        ("await", 0) => Expr::Await(Box::new(e), span),
                        _ => Expr::Call {
                            recv: Some(Box::new(e)),
                            name,
                            generics,
                            args,
                            span,
                        },
                    };
                } else {
                    e = Expr::Field { recv: Box::new(e), name, span };
                }
                continue;
            }
            if self.tok().is_punct("::") {
                self.bump();
                let head_generics =
                    if self.tok().is_punct("<") { self.type_args()? } else { Vec::new() };
                // `Vec::<i32>::new()` : on laisse le `::` suivant à l'itération d'après.

                // `x.parse::<i32>()` : `qualified_name` a déjà absorbé `parse`
                // dans le chemin, le turbofish porte donc sur l'appel courant.
                if !head_generics.is_empty() && !self.tok().is_punct("::") {
                    let Expr::Name(path, _) = &e else {
                        return self.err("turbofish : nom attendu à gauche de `::`");
                    };
                    if !self.tok().is_punct("(") {
                        return self.err("`(` attendu après le turbofish");
                    }
                    let (recv, name) = split_call_path(path.clone(), span);
                    let args = self.call_args()?;
                    e = Expr::Call { recv, name, generics: head_generics, args, span };
                    continue;
                }
                // `Vec::<i32>::new()` : les arguments portent sur le type. On
                // fabrique le chemin, la suite de la boucle traite `::new(...)`.
                if !head_generics.is_empty() {
                    let Expr::Name(path, _) = &e else {
                        return self.err("chemin de type attendu à gauche de `::<`");
                    };
                    e = Expr::TypePath { path: path.clone(), args: head_generics, span };
                    continue;
                }

                let name = self.expect_ident()?;
                let generics = if self.tok().is_punct("::") && self.at(1).is_punct("<") {
                    self.bump();
                    self.type_args()?
                } else {
                    Vec::new()
                };
                let recv = match &e {
                    Expr::Name(p, _) => Expr::Name(p.clone(), span),
                    Expr::TypePath { .. } => e.clone(),
                    _ => return self.err("`::` attendu après un nom de type"),
                };
                if self.tok().is_punct("(") {
                    // `Type::assoc(args)` : appel de fonction associée.
                    let args = self.call_args()?;
                    e = Expr::Call {
                        recv: Some(Box::new(recv)),
                        name,
                        generics,
                        args,
                        span,
                    };
                } else {
                    // `Type::method` : référence de méthode.
                    let Expr::Name(path, _) = recv else {
                        return self.err("référence de méthode : nom de type attendu");
                    };
                    e = Expr::MethodRef { ty: path, name, span };
                }
                continue;
            }
            if self.tok().is_punct("[") {
                self.bump();
                let idx = self.expr()?;
                self.expect_punct("]")?;
                e = Expr::Index { recv: Box::new(e), index: Box::new(idx), span };
                continue;
            }
            if self.tok().is_punct("++") {
                self.bump();
                e = Expr::PostIncDec { op: UnOp::PostInc, expr: Box::new(e), span };
                continue;
            }
            if self.tok().is_punct("--") {
                self.bump();
                e = Expr::PostIncDec { op: UnOp::PostDec, expr: Box::new(e), span };
                continue;
            }
            // `expr?` : extension Rust, hors syntaxe Java stricte.
            if self.tok().is_punct("?") && !self.starts_ternary_branch() {
                self.bump();
                e = Expr::Try(Box::new(e), span);
                continue;
            }
            break;
        }
        Ok(e)
    }

    /// Distingue `x?` (opérateur Rust) de `x ? a : b` (ternaire Java).
    fn starts_ternary_branch(&self) -> bool {
        let n = self.at(1);
        !matches!(n.kind, TokKind::Punct)
            || matches!(n.text.as_str(), "(" | "[" | "!" | "-" | "+" | "~")
    }

    fn call_args(&mut self) -> PResult<Vec<Expr>> {
        self.expect_punct("(")?;
        let mut args = Vec::new();
        if !self.tok().is_punct(")") {
            loop {
                args.push(self.expr()?);
                if !self.eat_punct(",") {
                    break;
                }
            }
        }
        self.expect_punct(")")?;
        Ok(args)
    }

    fn primary(&mut self) -> PResult<Expr> {
        self.check_unsupported()?;
        let span = self.span();
        let t = self.tok().clone();

        match t.kind {
            TokKind::IntLit | TokKind::FloatLit | TokKind::StrLit | TokKind::CharLit
            | TokKind::TextBlock => {
                let lit = self.literal()?;
                return Ok(Expr::Lit(lit, span));
            }
            _ => {}
        }

        // Valeur unité `()` — Java n'en a pas de notation, Rust en a besoin.
        if t.is_punct("(") && self.at(1).is_punct(")") && !self.at(2).is_punct("->") {
            self.bump();
            self.bump();
            return Ok(Expr::Lit(Lit::Unit, span));
        }

        if t.is_punct("(") {
            // Lambda `(a, b) -> ...`
            if self.in_guard == 0 {
                if let Some(e) = self.attempt(Self::lambda) {
                    return Ok(e);
                }
            }
            // Cast `(Type) expr`
            if let Some(e) = self.attempt(Self::cast) {
                return Ok(e);
            }
            self.bump();
            let inner = self.expr()?;
            self.expect_punct(")")?;
            return Ok(Expr::Paren(Box::new(inner), span));
        }

        if t.kind == TokKind::Ident {
            match t.text.as_str() {
                "true" => {
                    self.bump();
                    return Ok(Expr::Lit(Lit::Bool(true), span));
                }
                "false" => {
                    self.bump();
                    return Ok(Expr::Lit(Lit::Bool(false), span));
                }
                "null" => {
                    return self.err_fix(
                        "`null` n'existe pas en Rust",
                        "utilisez Option<T> : `None`, ou `Some(x)`. Voir docs/IMPOSSIBLE.md#null",
                        "rava.null",
                    )
                }
                "this" => {
                    self.bump();
                    return Ok(Expr::This(span));
                }
                "super" => {
                    self.bump();
                    return Ok(Expr::Super(span));
                }
                "new" => return self.new_expr(span),
                "switch" => return Ok(Expr::Switch(Box::new(self.switch_expr()?))),
                _ => {}
            }
            // Lambda à paramètre unique : `x -> ...`
            if self.in_guard == 0 && self.at(1).is_punct("->") {
                let name = self.bump().text;
                self.bump();
                let body = self.lambda_body()?;
                return Ok(Expr::Lambda {
                    params: vec![LambdaParam { annots: Vec::new(), name, ty: None }],
                    body: Box::new(body),
                    is_move: false,
                    span,
                });
            }
            let path = self.qualified_name()?;
            if self.tok().is_punct("(") {
                let args = self.call_args()?;
                if let Some(e) = self.builtin_call(&path, &args, span)? {
                    return Ok(e);
                }
                let (recv, name) = split_call_path(path, span);
                return Ok(Expr::Call { recv, name, generics: Vec::new(), args, span });
            }
            return Ok(Expr::Name(path, span));
        }

        self.err(format!("expression attendue, trouvé `{}`", t.text))
    }

    /// Traduit les façades Java strictes des constructions Rust sans syntaxe Java.
    fn builtin_call(
        &mut self,
        path: &[Ident],
        args: &[Expr],
        span: Span,
    ) -> PResult<Option<Expr>> {
        let (head, tail) = match path {
            [h, t] => (h.as_str(), t.as_str()),
            [h, mid @ .., t] if !mid.is_empty() => {
                let _ = mid;
                (h.as_str(), t.as_str())
            }
            _ => return Ok(None),
        };
        let a = |i: usize| args.get(i).cloned();
        Ok(match (head, tail) {
            ("Macro", name) => Some(Expr::Macro {
                name: name.to_string(),
                args: args.to_vec(),
                span,
            }),
            ("Ref", "of") => a(0).map(|e| Expr::Borrow {
                mutable: false,
                expr: Box::new(e),
                span,
            }),
            ("Ref", "mut_") => a(0).map(|e| Expr::Borrow {
                mutable: true,
                expr: Box::new(e),
                span,
            }),
            ("Deref", "of") => a(0).map(|e| Expr::Deref(Box::new(e), span)),
            ("Range", "of") => Some(Expr::Range {
                start: a(0).map(Box::new),
                end: a(1).map(Box::new),
                inclusive: false,
                span,
            }),
            ("Range", "closed") => Some(Expr::Range {
                start: a(0).map(Box::new),
                end: a(1).map(Box::new),
                inclusive: true,
                span,
            }),
            ("Range", "from") => Some(Expr::Range {
                start: a(0).map(Box::new),
                end: None,
                inclusive: false,
                span,
            }),
            ("Range", "to") => Some(Expr::Range {
                start: None,
                end: a(0).map(Box::new),
                inclusive: false,
                span,
            }),
            // Forme Java stricte de la valeur unité.
            ("Unit", "of") if args.is_empty() => Some(Expr::Lit(Lit::Unit, span)),
            ("Rust", "expr") => match args.first() {
                Some(Expr::Lit(Lit::Str(s), _)) => Some(Expr::RawRust(s.clone(), span)),
                _ => {
                    return self.err_note(
                        "Rust.expr attend un littéral de chaîne",
                        "exemple : Rust.expr(\"unsafe { *p }\")",
                    )
                }
            },
            ("System", "println") | ("System", "print") => None,
            _ => None,
        })
    }

    fn lambda(&mut self) -> PResult<Expr> {
        let span = self.span();
        self.expect_punct("(")?;
        let mut params = Vec::new();
        if !self.tok().is_punct(")") {
            loop {
                let annots = self.annots()?;
                // Paramètre typé `(int x)` ou nu `(x)`.
                let save = self.i;
                let ty = self.attempt(Self::type_);
                let name = match self.tok().kind {
                    TokKind::Ident if ty.is_some() => self.bump().text,
                    _ => {
                        self.i = save;
                        self.expect_ident()?
                    }
                };
                let ty = if self.i > save + 1 { ty } else { None };
                params.push(LambdaParam { annots, name, ty });
                if !self.eat_punct(",") {
                    break;
                }
            }
        }
        self.expect_punct(")")?;
        if !self.eat_punct("->") {
            return self.err("`->` attendu");
        }
        let body = self.lambda_body()?;
        Ok(Expr::Lambda { params, body: Box::new(body), is_move: false, span })
    }

    fn lambda_body(&mut self) -> PResult<LambdaBody> {
        if self.tok().is_punct("{") {
            Ok(LambdaBody::Block(self.block()?))
        } else {
            Ok(LambdaBody::Expr(self.expr()?))
        }
    }

    fn cast(&mut self) -> PResult<Expr> {
        let span = self.span();
        self.expect_punct("(")?;
        let ty = self.type_()?;
        self.expect_punct(")")?;
        // Un cast doit être suivi d'une expression, pas d'un opérateur binaire.
        if matches!(self.tok().kind, TokKind::Punct)
            && !matches!(self.tok().text.as_str(), "(" | "!" | "~" | "-")
        {
            return self.err("ce n'est pas un cast");
        }
        let e = self.unary()?;
        Ok(Expr::Cast { ty, expr: Box::new(e), span })
    }

    fn new_expr(&mut self, span: Span) -> PResult<Expr> {
        self.bump(); // new
        let ty = self.type_()?;

        // `new int[]{1, 2, 3}`
        if self.tok().is_punct("{") {
            if let TypeKind::Array(inner) = &ty.kind {
                let elem = (**inner).clone();
                self.bump();
                let mut elems = Vec::new();
                if !self.tok().is_punct("}") {
                    loop {
                        elems.push(self.expr()?);
                        if !self.eat_punct(",") {
                            break;
                        }
                    }
                }
                self.expect_punct("}")?;
                return Ok(Expr::ArrayLit { ty: Some(elem), elems, span });
            }
        }

        if !self.tok().is_punct("(") {
            return self.err(format!(
                "`(` attendu après `new {}`",
                ty.last_segment().unwrap_or("?")
            ));
        }
        let args = self.call_args()?;

        // Littéral de struct : `new Point() { x = 1, y = 2 }`
        if self.tok().is_punct("{") && args.is_empty() {
            self.bump();
            let mut fields = Vec::new();
            let mut rest = None;
            while !self.tok().is_punct("}") {
                if self.eat_punct("...") {
                    rest = Some(Box::new(self.expr()?));
                    break;
                }
                let name = self.expect_ident()?;
                self.expect_punct("=")?;
                fields.push((name, self.expr()?));
                if !self.eat_punct(",") {
                    break;
                }
            }
            self.expect_punct("}")?;
            return Ok(Expr::StructLit { ty, fields, rest, span });
        }

        Ok(Expr::New { ty, args, span })
    }
}


/// Précédence des comparaisons (`<`, `<=`, `>`, `>=`, `instanceof`).
const PREC_RELATIONAL: u8 = 11;
/// Ce que consomme l'opérande de `not` : tout sauf les opérateurs littéraux.
const PREC_NOT_OPERAND: u8 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WordOp {
    And,
    Or,
    Xor,
    Nand,
    Nor,
    Xnor,
    Implies,
}

/// Désucre un opérateur logique littéral vers sa forme Rust.
///
/// | Rava | Rust |
/// |---|---|
/// | `a and b` | `a && b` |
/// | `a or b` | `a \|\| b` |
/// | `a xor b` | `a ^ b` |
/// | `a nand b` | `!(a && b)` |
/// | `a nor b` | `!(a \|\| b)` |
/// | `a xnor b` | `!(a ^ b)` |
/// | `a implies b` | `!a \|\| b` |
fn build_word_op(w: WordOp, lhs: Expr, rhs: Expr, span: Span) -> Expr {
    let bin = |op: BinOp, l: Expr, r: Expr| Expr::Binary {
        op,
        lhs: Box::new(guard_operand(op, l)),
        rhs: Box::new(guard_operand(op, r)),
        span,
    };
    match w {
        WordOp::And => bin(BinOp::And, lhs, rhs),
        WordOp::Or => bin(BinOp::Or, lhs, rhs),
        WordOp::Xor => bin(BinOp::BitXor, lhs, rhs),
        WordOp::Nand => negate(bin(BinOp::And, lhs, rhs)),
        WordOp::Nor => negate(bin(BinOp::Or, lhs, rhs)),
        WordOp::Xnor => negate(bin(BinOp::BitXor, lhs, rhs)),
        WordOp::Implies => bin(BinOp::Or, negate(lhs), rhs),
    }
}

/// La précédence des mots (`and` < `xor`) n'est pas celle des symboles Rust
/// (`&&` < `^`) : on parenthèse tout opérande binaire d'un autre opérateur pour
/// que le Rust généré garde le sens écrit en Rava.
fn guard_operand(parent: BinOp, e: Expr) -> Expr {
    match &e {
        Expr::Binary { op, .. } if *op != parent => {
            let span = e.span();
            Expr::Paren(Box::new(e), span)
        }
        _ => e,
    }
}

/// Négation d'une condition, en gardant le Rust généré lisible :
/// on inverse l'opérateur de comparaison plutôt que d'empiler des `!`.
fn negate(e: Expr) -> Expr {
    match e {
        Expr::Unary { op: UnOp::Not, expr, .. } => *expr,
        Expr::Lit(Lit::Bool(b), span) => Expr::Lit(Lit::Bool(!b), span),
        Expr::Paren(inner, _) => negate(*inner),
        Expr::Binary { op, lhs, rhs, span } if inverse_cmp(op).is_some() => Expr::Binary {
            op: inverse_cmp(op).unwrap(),
            lhs,
            rhs,
            span,
        },
        other => {
            let span = other.span();
            // `!a && b` ne veut pas dire `!(a && b)` : on parenthèse ce qui lie
            // moins fort que l'opérateur unaire.
            let needs_paren = matches!(
                other,
                Expr::Binary { .. } | Expr::Ternary { .. } | Expr::Assign { .. } | Expr::Cast { .. }
            );
            let inner = if needs_paren { Expr::Paren(Box::new(other), span) } else { other };
            Expr::Unary { op: UnOp::Not, expr: Box::new(inner), span }
        }
    }
}

fn inverse_cmp(op: BinOp) -> Option<BinOp> {
    Some(match op {
        BinOp::Eq => BinOp::Ne,
        BinOp::Ne => BinOp::Eq,
        BinOp::Lt => BinOp::Ge,
        BinOp::Le => BinOp::Gt,
        BinOp::Gt => BinOp::Le,
        BinOp::Ge => BinOp::Lt,
        _ => return None,
    })
}

fn modifier_of(s: &str) -> Option<Modifier> {
    Some(match s {
        "public" => Modifier::Public,
        "private" => Modifier::Private,
        "protected" => Modifier::Protected,
        "static" => Modifier::Static,
        "final" => Modifier::Final,
        "abstract" => Modifier::Abstract,
        "default" => Modifier::Default,
        "native" => Modifier::Native,
        "synchronized" => Modifier::Synchronized,
        "transient" => Modifier::Transient,
        "volatile" => Modifier::Volatile,
        "strictfp" => Modifier::Strictfp,
        "sealed" => Modifier::Sealed,
        _ => return None,
    })
}

/// Une étiquette de boucle Java est conventionnellement en minuscules.
fn looks_like_label(s: &str) -> bool {
    s.chars().next().is_some_and(|c| c.is_lowercase())
}

/// `Foo.bar(...)` -> receveur `Foo`, méthode `bar` ; `bar(...)` -> pas de receveur.
fn split_call_path(mut path: Vec<Ident>, span: Span) -> (Option<Box<Expr>>, Ident) {
    let name = path.pop().expect("chemin non vide");
    if path.is_empty() {
        (None, name)
    } else {
        (Some(Box::new(Expr::Name(path, span))), name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one_class(src: &str) -> Class {
        match parse(src).unwrap().items.remove(0) {
            Item::Class(c) => c,
            other => panic!("classe attendue, obtenu {other:?}"),
        }
    }

    #[test]
    fn parse_une_classe_avec_champs_et_methode() {
        let c = one_class(
            r#"
            public class Point {
                int x;
                int y;
                @Mut public void translate(int dx) { this.x += dx; }
            }"#,
        );
        assert_eq!(c.name, "Point");
        assert_eq!(c.fields.len(), 2);
        assert_eq!(c.methods.len(), 1);
        assert!(has_annot(&c.methods[0].annots, "Mut"));
    }

    #[test]
    fn parse_generiques_imbriques() {
        let c = one_class("class A { Map<String, Vec<i32>> m; }");
        let TypeKind::Named { args, .. } = &c.fields[0].ty.kind else { panic!() };
        assert_eq!(args.len(), 2);
    }

    #[test]
    fn parse_switch_avec_deconstruction() {
        let c = one_class(
            r#"class A {
                int f(Shape s) {
                    return switch (s) {
                        case Circle(var r) -> r;
                        case Square(var a) when a > 0 -> a;
                        default -> 0;
                    };
                }
            }"#,
        );
        let Some(Stmt::Return(Some(Expr::Switch(sw)), _)) =
            c.methods[0].body.as_ref().map(|b| &b.stmts[0])
        else {
            panic!("switch attendu")
        };
        assert_eq!(sw.arms.len(), 3);
        assert!(sw.arms[1].guard.is_some());
    }

    #[test]
    fn refuse_null_avec_renvoi_doc() {
        let e = parse("class A { Object f() { return null; } }").unwrap_err();
        assert!(e.message.contains("null"));
        assert!(e.note.unwrap().contains("Option"));
    }

    #[test]
    fn refuse_les_exceptions_avec_renvoi_doc() {
        let e = parse("class A { void f() { throw new E(); } }").unwrap_err();
        assert!(e.note.unwrap().contains("IMPOSSIBLE"));
    }

    #[test]
    fn unless_nie_la_condition() {
        let c = one_class("class A { void f(int x) { unless (x > 0) { g(); } } }");
        let Some(Stmt::If { cond, .. }) = c.methods[0].body.as_ref().map(|b| &b.stmts[0]) else {
            panic!("if attendu")
        };
        // `unless (x > 0)` doit devenir `x <= 0`, pas `!(x > 0)`.
        assert!(matches!(cond, Expr::Binary { op: BinOp::Le, .. }));
    }

    #[test]
    fn else_unless_chaine_comme_else_if() {
        let c = one_class(
            "class A { void f(int x) { if (x == 1) { a(); } else unless (x == 2) { b(); } else { c(); } } }",
        );
        let Some(Stmt::If { otherwise: Some(e), .. }) =
            c.methods[0].body.as_ref().map(|b| &b.stmts[0])
        else {
            panic!("if attendu")
        };
        let Stmt::If { cond, otherwise, .. } = e.as_ref() else { panic!("else if attendu") };
        assert!(matches!(cond, Expr::Binary { op: BinOp::Ne, .. }));
        assert!(otherwise.is_some());
    }

    #[test]
    fn unless_sur_condition_composee_parenthese() {
        let c = one_class("class A { void f() { unless (a && b) { g(); } } }");
        let Some(Stmt::If { cond, .. }) = c.methods[0].body.as_ref().map(|b| &b.stmts[0]) else {
            panic!("if attendu")
        };
        let Expr::Unary { op: UnOp::Not, expr, .. } = cond else { panic!("négation attendue") };
        assert!(matches!(expr.as_ref(), Expr::Paren(..)));
    }

    fn cond_of(src: &str) -> Expr {
        let c = one_class(&format!("class A {{ void f() {{ if ({src}) {{ g(); }} }} }}"));
        let Some(Stmt::If { cond, .. }) = c.methods[0].body.as_ref().map(|b| &b.stmts[0]) else {
            panic!("if attendu")
        };
        cond.clone()
    }

    fn rust_of(src: &str) -> String {
        let u = parse(&format!(
            "class A {{ void f() {{ if ({src}) {{ g(); }} }} }}"
        ))
        .unwrap();
        let out = rava_codegen_stub(&u);
        out
    }

    // Le codegen vit dans un autre crate ; on rejoue ici la seule chose qui
    // nous intéresse : la forme de l'arbre produit par le désucrage.
    fn rava_codegen_stub(u: &Unit) -> String {
        let Item::Class(c) = &u.items[0] else { panic!() };
        let Some(Stmt::If { cond, .. }) = c.methods[0].body.as_ref().map(|b| &b.stmts[0]) else {
            panic!()
        };
        render(cond)
    }

    fn render(e: &Expr) -> String {
        match e {
            Expr::Name(p, _) => p.join("."),
            Expr::Lit(Lit::Bool(b), _) => b.to_string(),
            Expr::Paren(i, _) => format!("({})", render(i)),
            Expr::Unary { op: UnOp::Not, expr, .. } => format!("!{}", render(expr)),
            Expr::Binary { op, lhs, rhs, .. } => {
                format!("{} {} {}", render(lhs), op.as_rust(), render(rhs))
            }
            other => format!("{other:?}"),
        }
    }

    #[test]
    fn operateurs_logiques_litteraux() {
        assert_eq!(rust_of("a and b"), "a && b");
        assert_eq!(rust_of("a or b"), "a || b");
        assert_eq!(rust_of("a xor b"), "a ^ b");
        assert_eq!(rust_of("a nand b"), "!(a && b)");
        assert_eq!(rust_of("a nor b"), "!(a || b)");
        assert_eq!(rust_of("a xnor b"), "!(a ^ b)");
        assert_eq!(rust_of("a implies b"), "!a || b");
    }

    #[test]
    fn precedence_logique_and_plus_serre_que_or() {
        // `and` lie plus fort que `or`, et `implies` est le plus lâche.
        assert_eq!(rust_of("a and b or c"), "(a && b) || c");
        assert_eq!(rust_of("a or b implies c"), "!(a || b) || c");
    }

    #[test]
    fn precedence_rust_preservee_par_parentheses() {
        // En Rust `^` lie plus fort que `&&` : sans parenthèses, `a xor b and c`
        // changerait de sens.
        assert_eq!(rust_of("a xor b and c"), "a ^ (b && c)");
    }

    #[test]
    fn not_lie_plus_lache_que_les_comparaisons() {
        // `not a == b` doit valoir `not (a == b)`, donc `a != b`.
        assert!(matches!(cond_of("not a == b"), Expr::Binary { op: BinOp::Ne, .. }));
        // ... mais plus serré que `and`.
        assert_eq!(rust_of("not a and b"), "!a && b");
    }

    #[test]
    fn implies_est_associatif_a_droite() {
        assert_eq!(rust_of("a implies b implies c"), "!a || !b || c");
    }

    #[test]
    fn parse_lifetime_et_ref() {
        let c = one_class(
            r#"class A {
                @Lifetime("a")
                static @Ref("a") String longest(@Ref("a") String a, @Ref("a") String b) {
                    return a;
                }
            }"#,
        );
        assert_eq!(c.methods[0].generics.lifetimes, vec!["a"]);
        assert!(has_annot(&c.methods[0].params[0].ty.annots, "Ref"));
    }
}
