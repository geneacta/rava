//! Ce que le serveur sait dire d'un document : erreurs, survol, complétion,
//! plan du fichier et corrections rapides.

use crate::json::Json;
use crate::kb::{self, Kind};
use rava_ast::*;

/// Un document ouvert, avec ses lignes déjà découpées.
pub struct Doc {
    pub text: String,
    lines: Vec<String>,
}

impl Doc {
    pub fn new(text: String) -> Doc {
        let lines = text.split('\n').map(|l| l.trim_end_matches('\r').to_string()).collect();
        Doc { text, lines }
    }

    pub fn line(&self, i: u32) -> &str {
        self.lines.get(i as usize).map(String::as_str).unwrap_or("")
    }

    /// Position LSP (0-basée, en unités UTF-16) depuis une position Rava
    /// (1-basée, en caractères).
    fn to_lsp(&self, line: u32, col: u32) -> Json {
        let l = line.saturating_sub(1);
        let chars = col.saturating_sub(1) as usize;
        let utf16: usize = self.line(l).chars().take(chars).map(char::len_utf16).sum();
        Json::obj(vec![("line", Json::int(l as i64)), ("character", Json::int(utf16 as i64))])
    }

    /// Index de caractère dans la ligne depuis un décalage UTF-16 LSP.
    fn char_index(&self, line: u32, utf16: u32) -> usize {
        let mut seen = 0usize;
        for (i, c) in self.line(line).chars().enumerate() {
            if seen >= utf16 as usize {
                return i;
            }
            seen += c.len_utf16();
        }
        self.line(line).chars().count()
    }

    /// Étendue d'une erreur : le mot sous le curseur, ou un caractère à défaut.
    fn error_range(&self, line: u32, col: u32) -> Json {
        let l = line.saturating_sub(1);
        let start = col.saturating_sub(1) as usize;
        let chars: Vec<char> = self.line(l).chars().collect();
        let mut end = start;
        while end < chars.len() && (chars[end].is_alphanumeric() || chars[end] == '_') {
            end += 1;
        }
        if end == start {
            end = (start + 1).min(chars.len().max(start + 1));
        }
        Json::obj(vec![
            ("start", self.to_lsp(line, col)),
            ("end", self.to_lsp(line, end as u32 + 1)),
        ])
    }

    fn range(&self, line: u32, col: u32, len: usize) -> Json {
        Json::obj(vec![
            ("start", self.to_lsp(line, col)),
            ("end", self.to_lsp(line, col + len as u32)),
        ])
    }

    /// Mot sous le curseur, et si un `@` le précède immédiatement.
    fn word_at(&self, line: u32, utf16: u32) -> Option<(String, bool, u32)> {
        let chars: Vec<char> = self.line(line).chars().collect();
        let idx = self.char_index(line, utf16).min(chars.len());
        let is_word = |c: char| c.is_alphanumeric() || c == '_';
        // Le curseur peut être juste après le mot.
        let mut start = idx;
        while start > 0 && is_word(chars[start - 1]) {
            start -= 1;
        }
        let mut end = idx;
        while end < chars.len() && is_word(chars[end]) {
            end += 1;
        }
        if start == end {
            return None;
        }
        let word: String = chars[start..end].iter().collect();
        let is_annot = start > 0 && chars[start - 1] == '@';
        Some((word, is_annot, start as u32 + 1))
    }
}

// ------------------------------------------------------------------ erreurs

pub fn diagnostics(doc: &Doc) -> Json {
    let mut out = Vec::new();
    let mut push = |line: u32, col: u32, msg: &str, note: Option<&str>, code: Option<&str>| {
        let mut fields = vec![
            ("range", doc.error_range(line, col)),
            ("severity", Json::int(1)),
            ("source", Json::str("rava")),
            (
                "message",
                Json::str(match note {
                    Some(n) => format!("{msg}\n\n{n}"),
                    None => msg.to_string(),
                }),
            ),
        ];
        if let Some(c) = code {
            fields.push(("code", Json::str(c)));
        }
        out.push(Json::obj(fields));
    };

    match rava_parser::parse(&doc.text) {
        Err(e) => push(e.line, e.col, &e.message, e.note.as_deref(), e.code),
        Ok(unit) => {
            if let Err(e) = rava_codegen::generate(&unit) {
                push(e.line, e.col, &e.message, e.note.as_deref(), e.code);
            }
        }
    }
    Json::Arr(out)
}

// ------------------------------------------------------------------ survol

pub fn hover(doc: &Doc, line: u32, character: u32) -> Json {
    let Some((word, is_annot, _)) = doc.word_at(line, character) else {
        return Json::Null;
    };
    // `Macro.println` : le survol sur le membre décrit le membre.
    let before: String = doc.line(line).chars().take(doc.char_index(line, character)).collect();
    if let Some(facade) = before.rsplit_once('.').and_then(|(head, _)| {
        let owner: String =
            head.chars().rev().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
        let owner: String = owner.chars().rev().collect();
        kb::members_of(&owner).map(|_| owner)
    }) {
        if let Some(m) = kb::members_of(&facade).and_then(|ms| ms.iter().find(|m| m.label == word)) {
            return hover_body(&format!("{facade}.{}", m.label), m.rust, m.doc);
        }
    }

    let Some(entry) = kb::lookup(&word, is_annot) else {
        return Json::Null;
    };
    let title = if is_annot { format!("@{}", entry.label) } else { entry.label.to_string() };
    hover_body(&title, entry.rust, entry.doc)
}

fn hover_body(title: &str, rust: &str, doc: &str) -> Json {
    let value = format!("**{title}**\n\n```rust\n{rust}\n```\n\n{doc}");
    Json::obj(vec![(
        "contents",
        Json::obj(vec![("kind", Json::str("markdown")), ("value", Json::str(value))]),
    )])
}

// ------------------------------------------------------------------ complétion

pub fn completion(doc: &Doc, line: u32, character: u32) -> Json {
    let idx = doc.char_index(line, character);
    let before: String = doc.line(line).chars().take(idx).collect();

    // Après `@` : uniquement les annotations.
    let trailing: String =
        before.chars().rev().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
    let prefix_char = before.chars().nth(before.chars().count() - trailing.chars().count() - 1);
    if prefix_char == Some('@') {
        return items(kb::ANNOTATIONS);
    }

    // Après `Facade.` : les membres de la façade.
    if prefix_char == Some('.') {
        let head: String = before
            .chars()
            .take(before.chars().count() - trailing.chars().count() - 1)
            .collect();
        let owner: String =
            head.chars().rev().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
        let owner: String = owner.chars().rev().collect();
        if let Some(ms) = kb::members_of(&owner) {
            return items(ms);
        }
        if owner == "System" {
            return Json::Arr(vec![
                item(&kb::Entry {
                    label: "out",
                    kind: Kind::Member,
                    rust: "println! / print!",
                    doc: "Sortie standard : `System.out.println(fmt, args)`.",
                    snippet: Some("out.println(\"${1:{}}\", $0)"),
                }),
                item(&kb::Entry {
                    label: "err",
                    kind: Kind::Member,
                    rust: "eprintln! / eprint!",
                    doc: "Sortie d'erreur : `System.err.println(fmt, args)`.",
                    snippet: Some("err.println(\"${1:{}}\", $0)"),
                }),
            ]);
        }
        return Json::Arr(Vec::new());
    }

    let mut all: Vec<Json> = Vec::new();
    all.extend(kb::KEYWORDS.iter().map(item));
    all.extend(kb::FACADES.iter().map(item));
    all.extend(kb::TYPES.iter().map(item));
    all.extend(SNIPPETS.iter().map(|(label, detail, body)| {
        Json::obj(vec![
            ("label", Json::str(*label)),
            ("kind", Json::int(15)),
            ("detail", Json::str(*detail)),
            ("insertText", Json::str(*body)),
            ("insertTextFormat", Json::int(2)),
        ])
    }));
    Json::Arr(all)
}

fn items(entries: &[kb::Entry]) -> Json {
    Json::Arr(entries.iter().map(item).collect())
}

/// `CompletionItemKind` du protocole : c'est lui qui choisit l'icône.
fn completion_kind(k: Kind) -> i64 {
    match k {
        Kind::Annotation => 10, // Property
        Kind::Keyword => 14,
        Kind::Facade => 7,      // Class
        Kind::Member => 2,      // Method
        Kind::Type => 22,       // Struct
        Kind::Refused => 1,     // Text — jamais proposé, seulement survolé
    }
}

fn item(e: &kb::Entry) -> Json {
    let mut fields = vec![
        ("label", Json::str(e.label)),
        ("kind", Json::int(completion_kind(e.kind))),
        ("detail", Json::str(format!("→ {}", e.rust))),
        (
            "documentation",
            Json::obj(vec![
                ("kind", Json::str("markdown")),
                ("value", Json::str(e.doc)),
            ]),
        ),
    ];
    if let Some(s) = e.snippet {
        fields.push(("insertText", Json::str(s)));
        fields.push(("insertTextFormat", Json::int(2)));
    }
    Json::obj(fields)
}

/// Squelettes courants, proposés comme fragments.
const SNIPPETS: &[(&str, &str, &str)] = &[
    ("class", "class + constructeur", "public class ${1:Nom} {\n\tprivate ${2:i32} ${3:champ};\n\n\tpublic ${1:Nom}(${2:i32} ${3:champ}) {\n\t\tthis.${3:champ} = ${3:champ};\n\t}\n}"),
    ("main", "point d'entrée", "public static void main(String[] args) {\n\t$0\n}"),
    ("switch-result", "filtrage d'un Result", "switch (${1:expr}) {\n\tcase Ok(var v) -> $0;\n\tcase Err(var e) -> ;\n}"),
    ("switch-option", "filtrage d'une Option", "switch (${1:expr}) {\n\tcase Some(var v) -> $0;\n\tcase None -> ;\n}"),
    ("foreach-ref", "boucle sur un emprunt", "for (@Ref var ${1:x} : ${2:liste}) {\n\t$0\n}"),
    ("impl", "méthode d'interface", "@Override\npublic ${1:void} ${2:nom}() {\n\t$0\n}"),
];

// ------------------------------------------------------------------ plan

/// `None` si le fichier ne se lit pas : l'appelant garde alors le plan
/// précédent, plutôt que de le faire disparaître à chaque frappe.
pub fn document_symbols(doc: &Doc) -> Option<Json> {
    let unit = rava_parser::parse(&doc.text).ok()?;
    Some(Json::Arr(unit.items.iter().map(|i| symbol_of_item(doc, i)).collect()))
}

fn symbol_of_item(doc: &Doc, item: &Item) -> Json {
    match item {
        Item::Class(c) => {
            let mut kids: Vec<Json> = Vec::new();
            kids.extend(c.fields.iter().map(|f| sym(doc, &f.name, 8, f.span)));
            kids.extend(c.consts.iter().map(|k| sym(doc, &k.name, 14, k.span)));
            kids.extend(c.methods.iter().map(|m| method_sym(doc, m)));
            kids.extend(c.nested.iter().map(|n| symbol_of_item(doc, n)));
            container(doc, &c.name, 5, c.span, kids)
        }
        Item::Record(r) => {
            let mut kids: Vec<Json> =
                r.components.iter().map(|p| sym(doc, &p.name, 8, p.span)).collect();
            kids.extend(r.methods.iter().map(|m| method_sym(doc, m)));
            container(doc, &r.name, 23, r.span, kids)
        }
        Item::Interface(i) => {
            let mut kids: Vec<Json> =
                i.assoc_types.iter().map(|a| sym(doc, &a.name, 26, a.span)).collect();
            kids.extend(i.consts.iter().map(|k| sym(doc, &k.name, 14, k.span)));
            kids.extend(i.methods.iter().map(|m| method_sym(doc, m)));
            container(doc, &i.name, 11, i.span, kids)
        }
        Item::Enum(e) => {
            let mut kids: Vec<Json> =
                e.variants.iter().map(|v| sym(doc, &v.name, 22, v.span)).collect();
            kids.extend(e.methods.iter().map(|m| method_sym(doc, m)));
            container(doc, &e.name, 10, e.span, kids)
        }
    }
}

fn method_sym(doc: &Doc, m: &Method) -> Json {
    let name = if m.is_ctor { format!("{}(…)", m.name) } else { format!("{}(…)", m.name) };
    sym(doc, &name, if m.is_ctor { 9 } else { 6 }, m.span)
}

fn sym(doc: &Doc, name: &str, kind: i64, span: Span) -> Json {
    let r = doc.range(span.line, span.col, name.chars().count());
    Json::obj(vec![
        ("name", Json::str(name)),
        ("kind", Json::int(kind)),
        ("range", r.clone()),
        ("selectionRange", r),
    ])
}

fn container(doc: &Doc, name: &str, kind: i64, span: Span, children: Vec<Json>) -> Json {
    let r = doc.range(span.line, span.col, name.chars().count());
    Json::obj(vec![
        ("name", Json::str(name)),
        ("kind", Json::int(kind)),
        ("range", r.clone()),
        ("selectionRange", r),
        ("children", Json::Arr(children)),
    ])
}

// ------------------------------------------------------------------ corrections

/// Corrections rapides, accrochées au code du diagnostic.
pub fn code_actions(doc: &Doc, uri: &str, diags: &[Json]) -> Json {
    let mut out = Vec::new();
    for d in diags {
        let Some(code) = d.get("code").and_then(Json::as_str) else { continue };
        let Some(range) = d.get("range") else { continue };
        let line = range.path(&["start", "line"]).and_then(Json::as_u32).unwrap_or(0);

        let fix = match code {
            "rava.null" => Some(("Remplacer `null` par `None`", range.clone(), "None".to_string())),
            "rava.case-colon" => {
                Some(("Passer à la forme flèche `case X ->`", range.clone(), " ->".to_string()))
            }
            "rava.synchronized" => {
                let text = doc.line(line);
                text.find("synchronized").map(|byte| {
                    let col = text[..byte].chars().count() as u32 + 1;
                    let len = "synchronized ".len();
                    let len = if text[byte..].starts_with("synchronized ") { len } else { len - 1 };
                    (
                        "Retirer `synchronized` (protégez la donnée avec un Mutex)",
                        doc.range(line + 1, col, len),
                        String::new(),
                    )
                })
            }
            _ => None,
        };

        let Some((title, range, new_text)) = fix else { continue };
        out.push(Json::obj(vec![
            ("title", Json::str(title)),
            ("kind", Json::str("quickfix")),
            ("diagnostics", Json::Arr(vec![d.clone()])),
            ("isPreferred", Json::Bool(true)),
            (
                "edit",
                Json::obj(vec![(
                    "changes",
                    Json::Obj(
                        [(
                            uri.to_string(),
                            Json::Arr(vec![Json::obj(vec![
                                ("range", range),
                                ("newText", Json::str(new_text)),
                            ])]),
                        )]
                        .into_iter()
                        .collect(),
                    ),
                )]),
            ),
        ]));
    }
    Json::Arr(out)
}
