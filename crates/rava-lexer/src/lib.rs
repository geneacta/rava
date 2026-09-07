//! Lexer Rava : découpe un fichier `.rava` en tokens Java.
//!
//! Il accepte la syntaxe Java stricte, plus deux extensions optionnelles
//! héritées de Rust qui n'ont pas d'équivalent Java (voir `docs/SYNTAX.md`) :
//! le `?` postfixé et `expr.await`.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub line: u32,
    pub col: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokKind {
    Ident,
    IntLit,
    FloatLit,
    StrLit,
    /// Bloc de texte Java `"""..."""`.
    TextBlock,
    CharLit,
    Punct,
    Eof,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokKind,
    /// Texte source brut du token (littéraux : contenu déjà décodé).
    pub text: String,
    pub span: Span,
    /// Javadoc (`/** ... */`) ou `///` précédant immédiatement le token.
    pub doc: Option<String>,
}

impl Token {
    pub fn is_punct(&self, p: &str) -> bool {
        self.kind == TokKind::Punct && self.text == p
    }
    pub fn is_ident(&self, i: &str) -> bool {
        self.kind == TokKind::Ident && self.text == i
    }
}

#[derive(Debug, Clone)]
pub struct LexError {
    pub message: String,
    pub span: Span,
}

impl fmt::Display for LexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: {}", self.span.line, self.span.col, self.message)
    }
}

/// Opérateurs, du plus long au plus court : l'ordre garantit un « maximal munch ».
const PUNCTS: &[&str] = &[
    ">>>=", "<<=", ">>=", ">>>", "...", "->", "::", "++", "--", "&&", "||", "==", "!=", "<=", ">=",
    "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "<<", ">>", "{", "}", "(", ")", "[", "]", ";",
    ",", ".", "=", ">", "<", "!", "~", "?", ":", "+", "-", "*", "/", "&", "|", "^", "%", "@",
];

pub fn lex(src: &str) -> Result<Vec<Token>, LexError> {
    Lexer::new(src).run()
}

struct Lexer<'a> {
    src: &'a [u8],
    pos: usize,
    line: u32,
    col: u32,
    pending_doc: Option<String>,
}

impl<'a> Lexer<'a> {
    fn new(src: &'a str) -> Self {
        Lexer { src: src.as_bytes(), pos: 0, line: 1, col: 1, pending_doc: None }
    }

    fn span(&self) -> Span {
        Span { line: self.line, col: self.col }
    }

    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    fn peek_at(&self, off: usize) -> Option<u8> {
        self.src.get(self.pos + off).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let c = self.peek()?;
        self.pos += 1;
        if c == b'\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(c)
    }

    fn starts_with(&self, s: &str) -> bool {
        self.src[self.pos..].starts_with(s.as_bytes())
    }

    fn err<T>(&self, msg: impl Into<String>) -> Result<T, LexError> {
        Err(LexError { message: msg.into(), span: self.span() })
    }

    fn run(mut self) -> Result<Vec<Token>, LexError> {
        let mut out = Vec::new();
        loop {
            self.skip_trivia()?;
            let span = self.span();
            let Some(c) = self.peek() else {
                out.push(Token {
                    kind: TokKind::Eof,
                    text: String::new(),
                    span,
                    doc: self.pending_doc.take(),
                });
                return Ok(out);
            };
            let tok = if is_ident_start(c) {
                self.lex_ident(span)
            } else if c.is_ascii_digit() {
                self.lex_number(span)?
            } else if c == b'"' {
                self.lex_string(span)?
            } else if c == b'\'' {
                self.lex_char(span)?
            } else {
                self.lex_punct(span)?
            };
            out.push(tok);
        }
    }

    fn skip_trivia(&mut self) -> Result<(), LexError> {
        loop {
            match self.peek() {
                Some(c) if c.is_ascii_whitespace() => {
                    self.bump();
                }
                Some(b'/') if self.peek_at(1) == Some(b'/') => {
                    // `///` est traité comme de la doc, comme en Rust.
                    let is_doc = self.peek_at(2) == Some(b'/');
                    self.bump();
                    self.bump();
                    if is_doc {
                        self.bump();
                    }
                    let start = self.pos;
                    while matches!(self.peek(), Some(c) if c != b'\n') {
                        self.bump();
                    }
                    let text = String::from_utf8_lossy(&self.src[start..self.pos]).into_owned();
                    if is_doc {
                        self.push_doc(text.trim());
                    }
                }
                Some(b'/') if self.peek_at(1) == Some(b'*') => {
                    let is_doc = self.peek_at(2) == Some(b'*') && self.peek_at(3) != Some(b'/');
                    let start = self.span();
                    self.bump();
                    self.bump();
                    let from = self.pos;
                    let text;
                    loop {
                        if self.peek().is_none() {
                            return Err(LexError {
                                message: "commentaire de bloc non terminé".into(),
                                span: start,
                            });
                        }
                        if self.starts_with("*/") {
                            text = String::from_utf8_lossy(&self.src[from..self.pos]).into_owned();
                            self.bump();
                            self.bump();
                            break;
                        }
                        self.bump();
                    }
                    if is_doc {
                        for raw in text.trim_start_matches('*').lines() {
                            let l = raw.trim().trim_start_matches('*').trim();
                            self.push_doc(l);
                        }
                    }
                }
                _ => return Ok(()),
            }
        }
    }

    fn push_doc(&mut self, line: &str) {
        let slot = self.pending_doc.get_or_insert_with(String::new);
        if !slot.is_empty() {
            slot.push('\n');
        }
        slot.push_str(line);
    }

    fn lex_ident(&mut self, span: Span) -> Token {
        let start = self.pos;
        while matches!(self.peek(), Some(c) if is_ident_continue(c)) {
            self.bump();
        }
        let text = String::from_utf8_lossy(&self.src[start..self.pos]).into_owned();
        Token { kind: TokKind::Ident, text, span, doc: self.pending_doc.take() }
    }

    fn lex_number(&mut self, span: Span) -> Result<Token, LexError> {
        let start = self.pos;
        let mut is_float = false;
        if self.peek() == Some(b'0')
            && matches!(self.peek_at(1), Some(b'x') | Some(b'X') | Some(b'b') | Some(b'B'))
        {
            self.bump();
            self.bump();
            while matches!(self.peek(), Some(c) if c.is_ascii_alphanumeric() || c == b'_') {
                self.bump();
            }
        } else {
            while matches!(self.peek(), Some(c) if c.is_ascii_digit() || c == b'_') {
                self.bump();
            }
            // Un `.` suivi d'un chiffre est décimal ; sinon c'est un accès champ.
            if self.peek() == Some(b'.') && matches!(self.peek_at(1), Some(c) if c.is_ascii_digit())
            {
                is_float = true;
                self.bump();
                while matches!(self.peek(), Some(c) if c.is_ascii_digit() || c == b'_') {
                    self.bump();
                }
            }
            if matches!(self.peek(), Some(b'e') | Some(b'E')) {
                is_float = true;
                self.bump();
                if matches!(self.peek(), Some(b'+') | Some(b'-')) {
                    self.bump();
                }
                while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                    self.bump();
                }
            }
            // Suffixes Java : L, l, f, F, d, D.
            if matches!(self.peek(), Some(b'L') | Some(b'l') | Some(b'f') | Some(b'F') | Some(b'd') | Some(b'D'))
            {
                if matches!(self.peek(), Some(b'f') | Some(b'F') | Some(b'd') | Some(b'D')) {
                    is_float = true;
                }
                self.bump();
            }
        }
        let text = String::from_utf8_lossy(&self.src[start..self.pos]).into_owned();
        let kind = if is_float { TokKind::FloatLit } else { TokKind::IntLit };
        Ok(Token { kind, text, span, doc: self.pending_doc.take() })
    }

    fn lex_string(&mut self, span: Span) -> Result<Token, LexError> {
        if self.starts_with("\"\"\"") {
            self.bump();
            self.bump();
            self.bump();
            // Le contenu commence après le saut de ligne qui suit l'ouverture.
            while matches!(self.peek(), Some(c) if c != b'\n') {
                self.bump();
            }
            self.bump();
            let start = self.pos;
            loop {
                if self.peek().is_none() {
                    return self.err("bloc de texte non terminé");
                }
                if self.starts_with("\"\"\"") {
                    break;
                }
                self.bump();
            }
            let raw = String::from_utf8_lossy(&self.src[start..self.pos]).into_owned();
            self.bump();
            self.bump();
            self.bump();
            return Ok(Token {
                kind: TokKind::TextBlock,
                text: dedent(&raw),
                span,
                doc: self.pending_doc.take(),
            });
        }
        self.bump();
        let mut text = String::new();
        loop {
            match self.peek() {
                None | Some(b'\n') => return self.err("chaîne non terminée"),
                Some(b'"') => {
                    self.bump();
                    break;
                }
                Some(b'\\') => {
                    self.bump();
                    text.push('\\');
                    if let Some(c) = self.bump() {
                        text.push(c as char);
                    }
                }
                Some(_) => {
                    let start = self.pos;
                    self.bump();
                    // Recolle les octets d'un caractère UTF-8 multi-octets.
                    while matches!(self.peek(), Some(c) if c & 0xC0 == 0x80) {
                        self.bump();
                    }
                    text.push_str(&String::from_utf8_lossy(&self.src[start..self.pos]));
                }
            }
        }
        Ok(Token { kind: TokKind::StrLit, text, span, doc: self.pending_doc.take() })
    }

    fn lex_char(&mut self, span: Span) -> Result<Token, LexError> {
        self.bump();
        let mut text = String::new();
        loop {
            match self.peek() {
                None | Some(b'\n') => return self.err("littéral caractère non terminé"),
                Some(b'\'') => {
                    self.bump();
                    break;
                }
                Some(b'\\') => {
                    self.bump();
                    text.push('\\');
                    if let Some(c) = self.bump() {
                        text.push(c as char);
                    }
                }
                Some(_) => {
                    let start = self.pos;
                    self.bump();
                    while matches!(self.peek(), Some(c) if c & 0xC0 == 0x80) {
                        self.bump();
                    }
                    text.push_str(&String::from_utf8_lossy(&self.src[start..self.pos]));
                }
            }
        }
        Ok(Token { kind: TokKind::CharLit, text, span, doc: self.pending_doc.take() })
    }

    fn lex_punct(&mut self, span: Span) -> Result<Token, LexError> {
        for p in PUNCTS {
            if self.starts_with(p) {
                for _ in 0..p.len() {
                    self.bump();
                }
                return Ok(Token {
                    kind: TokKind::Punct,
                    text: (*p).to_string(),
                    span,
                    doc: self.pending_doc.take(),
                });
            }
        }
        let c = self.peek().unwrap() as char;
        self.err(format!("caractère inattendu `{c}`"))
    }
}

fn is_ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_' || c == b'$' || c >= 0x80
}

fn is_ident_continue(c: u8) -> bool {
    is_ident_start(c) || c.is_ascii_digit()
}

/// Retire l'indentation commune d'un bloc de texte Java.
fn dedent(s: &str) -> String {
    let indent = s
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);
    s.lines()
        .map(|l| if l.len() >= indent { &l[indent..] } else { l.trim_start() })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexe_une_methode() {
        let toks = lex("public void f() { int x = 1; }").unwrap();
        let texts: Vec<_> = toks.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(
            texts,
            ["public", "void", "f", "(", ")", "{", "int", "x", "=", "1", ";", "}", ""]
        );
    }

    #[test]
    fn distingue_champ_et_flottant() {
        let toks = lex("1.5 x.y").unwrap();
        assert_eq!(toks[0].kind, TokKind::FloatLit);
        assert_eq!(toks[0].text, "1.5");
        assert!(toks[2].is_punct("."));
    }

    #[test]
    fn maximal_munch_sur_les_operateurs() {
        let toks = lex(">>>= >>> >> >").unwrap();
        let texts: Vec<_> = toks.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(texts, [">>>=", ">>>", ">>", ">", ""]);
    }

    #[test]
    fn attache_la_javadoc() {
        let toks = lex("/** Doc ligne. */\nclass A {}").unwrap();
        assert_eq!(toks[0].text, "class");
        assert_eq!(toks[0].doc.as_deref(), Some("Doc ligne."));
    }
}
