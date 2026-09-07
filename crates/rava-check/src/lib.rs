//! Vérification complète d'un source Rava.
//!
//! Traduit le `.rava`, le confie à `rustc`, puis **ramène les diagnostics sur
//! le fichier écrit**. C'est ce qui rend la promesse tenable : la sémantique
//! est celle de Rust, donc les erreurs sont celles de Rust — mais elles
//! doivent se lire à l'endroit où l'on a tapé.

use rava_json::Json;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Error,
    Warning,
}

impl Level {
    pub fn label(self) -> &'static str {
        match self {
            Level::Error => "erreur",
            Level::Warning => "attention",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub level: Level,
    /// `E0502` pour rustc, `rava.null` pour les refus de Rava.
    pub code: Option<String>,
    pub message: String,
    /// Position dans le `.rava`.
    pub line: u32,
    pub col: u32,
    /// Position d'origine dans le Rust généré, quand le diagnostic en vient.
    pub rust_pos: Option<(u32, u32)>,
    /// Notes et aides attachées.
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrateType {
    /// Vérification seule : accepte un fichier sans `main`.
    Lib,
    /// Production d'un exécutable.
    Bin,
}

pub struct Report {
    pub diagnostics: Vec<Diagnostic>,
    /// Rust produit, si la traduction a abouti.
    pub rust: Option<String>,
    /// Fichier `.rs` écrit sur disque, si `rustc` a été sollicité.
    pub rust_path: Option<PathBuf>,
    /// Exécutable produit, pour `CrateType::Bin` sans erreur.
    pub binary: Option<PathBuf>,
    /// Faux quand `rustc` est introuvable : seuls les diagnostics Rava sont là.
    pub rustc_available: bool,
}

impl Report {
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(|d| d.level == Level::Error)
    }
}

/// Vérification rapide : syntaxe et traduction, sans appeler `rustc`.
///
/// C'est ce qu'un éditeur peut se permettre à chaque frappe.
pub fn check_syntax(source: &str) -> Vec<Diagnostic> {
    match rava_parser::parse(source) {
        Err(e) => vec![Diagnostic {
            level: Level::Error,
            code: e.code.map(str::to_string),
            message: e.message,
            line: e.line,
            col: e.col,
            rust_pos: None,
            notes: e.note.into_iter().collect(),
        }],
        Ok(unit) => match rava_codegen::generate(&unit) {
            Ok(_) => Vec::new(),
            Err(e) => vec![Diagnostic {
                level: Level::Error,
                code: e.code.map(str::to_string),
                message: e.message,
                line: e.line,
                col: e.col,
                rust_pos: None,
                notes: e.note.into_iter().collect(),
            }],
        },
    }
}

/// Traduit puis vérifie. `name` sert à nommer le fichier `.rs` intermédiaire.
pub fn check(source: &str, name: &str, crate_type: CrateType) -> Report {
    let mut report = Report {
        diagnostics: Vec::new(),
        rust: None,
        rust_path: None,
        binary: None,
        rustc_available: true,
    };

    let unit = match rava_parser::parse(source) {
        Ok(u) => u,
        Err(e) => {
            report.diagnostics.push(Diagnostic {
                level: Level::Error,
                code: e.code.map(str::to_string),
                message: e.message,
                line: e.line,
                col: e.col,
                rust_pos: None,
                notes: e.note.into_iter().collect(),
            });
            return report;
        }
    };

    let out = match rava_codegen::generate_with_map(&unit) {
        Ok(o) => o,
        Err(e) => {
            report.diagnostics.push(Diagnostic {
                level: Level::Error,
                code: e.code.map(str::to_string),
                message: e.message,
                line: e.line,
                col: e.col,
                rust_pos: None,
                notes: e.note.into_iter().collect(),
            });
            return report;
        }
    };

    let crate_name = sanitize(name);
    // Le répertoire dépend du contenu : deux vérifications concurrentes du même
    // nom mais de sources différentes ne se marchent pas dessus.
    let dir = std::env::temp_dir().join(format!(
        "rava-{crate_name}-{}-{:x}",
        std::process::id(),
        digest(source)
    ));
    if std::fs::create_dir_all(&dir).is_err() {
        report.rust = Some(out.rust);
        return report;
    }
    let rs = dir.join(format!("{crate_name}.rs"));
    if std::fs::write(&rs, &out.rust).is_err() {
        report.rust = Some(out.rust);
        return report;
    }
    let bin = dir.join(&crate_name);

    let mut cmd = Command::new("rustc");
    cmd.arg("--edition=2021")
        .arg("--error-format=json")
        .arg("--crate-name")
        .arg(&crate_name)
        .arg("-o")
        .arg(&bin)
        .arg(&rs);
    match crate_type {
        CrateType::Lib => {
            cmd.arg("--crate-type=lib").arg("--emit=metadata");
        }
        CrateType::Bin => {
            cmd.arg("--crate-type=bin");
        }
    }

    match cmd.output() {
        Err(_) => report.rustc_available = false,
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            let rava_lines: Vec<&str> = source.lines().collect();
            let rust_lines: Vec<&str> = out.rust.lines().collect();
            for line in stderr.lines() {
                let Some(v) = rava_json::parse(line) else { continue };
                if let Some(d) = convert(&v, &out.map, &rava_lines, &rust_lines) {
                    report.diagnostics.push(d);
                }
            }
            if o.status.success() && crate_type == CrateType::Bin {
                report.binary = Some(bin);
            }
        }
    }

    report.rust = Some(out.rust);
    report.rust_path = Some(rs);
    report
}

/// Convertit un diagnostic JSON de `rustc` en diagnostic ancré sur le `.rava`.
fn convert(
    v: &Json,
    map: &[u32],
    rava_lines: &[&str],
    rust_lines: &[&str],
) -> Option<Diagnostic> {
    let level = match v.get("level")?.as_str()? {
        "error" => Level::Error,
        "warning" => Level::Warning,
        // `failure-note`, `note` isolée : ce sont des post-scriptums de rustc.
        _ => return None,
    };
    let message = v.get("message")?.as_str()?.to_string();
    // rustc clôt sa sortie par un décompte, qui n'apporte rien ici.
    if message.starts_with("aborting due to") || message.starts_with("For more information") {
        return None;
    }
    let code = v.path(&["code", "code"]).and_then(Json::as_str).map(str::to_string);

    let primary = v
        .get("spans")
        .and_then(Json::as_arr)
        .and_then(|spans| {
            spans
                .iter()
                .find(|s| s.get("is_primary") == Some(&Json::Bool(true)))
                .or_else(|| spans.first())
        })
        .cloned();

    let (line, col, rust_pos) = match &primary {
        Some(s) => {
            let rl = s.get("line_start").and_then(Json::as_u32).unwrap_or(1);
            let rc = s.get("column_start").and_then(Json::as_u32).unwrap_or(1);
            let origin = map.get(rl.saturating_sub(1) as usize).copied().unwrap_or(0);
            if origin == 0 {
                // Ligne sans origine : le prélude, ou un masque inséré. Une
                // erreur mérite quand même d'être vue ; un avertissement, non.
                if level == Level::Warning {
                    return None;
                }
                (1, 1, Some((rl, rc)))
            } else {
                (origin, align_column(origin, rc, rl, rava_lines, rust_lines), Some((rl, rc)))
            }
        }
        None => (1, 1, None),
    };

    let mut notes = Vec::new();
    if let Some(label) = primary.as_ref().and_then(|s| s.get("label")).and_then(Json::as_str) {
        notes.push(label.to_string());
    }
    // Une erreur d'emprunt se comprend par ses positions liées : l'emprunt
    // initial, l'usage suivant. On les ramène aussi sur le `.rava`.
    if let Some(spans) = v.get("spans").and_then(Json::as_arr) {
        for sp in spans {
            if sp.get("is_primary") == Some(&Json::Bool(true)) {
                continue;
            }
            let Some(label) = sp.get("label").and_then(Json::as_str) else { continue };
            if let Some(l) = origin_line(sp, map) {
                notes.push(format!("ligne {l} : {label}"));
            }
        }
    }
    if let Some(children) = v.get("children").and_then(Json::as_arr) {
        for c in children {
            let (Some(lvl), Some(msg)) =
                (c.get("level").and_then(Json::as_str), c.get("message").and_then(Json::as_str))
            else {
                continue;
            };
            if msg.is_empty() {
                continue;
            }
            let where_ = c
                .get("spans")
                .and_then(Json::as_arr)
                .and_then(|s| s.first())
                .and_then(|s| origin_line(s, map))
                .map(|l| format!(" (ligne {l})"))
                .unwrap_or_default();
            notes.push(format!("{lvl}{where_} : {msg}"));
        }
    }

    Some(Diagnostic { level, code, message, line, col, rust_pos, notes })
}

/// Ligne `.rava` d'un span rustc, si elle est connue.
fn origin_line(span: &Json, map: &[u32]) -> Option<u32> {
    let rl = span.get("line_start").and_then(Json::as_u32)?;
    match map.get(rl.saturating_sub(1) as usize).copied() {
        Some(0) | None => None,
        Some(l) => Some(l),
    }
}

/// Reporte la colonne : on cherche dans la ligne `.rava` le mot que `rustc`
/// désigne dans le Rust. Les identifiants traversent la traduction tels quels,
/// donc cela tombe juste la plupart du temps.
fn align_column(
    rava_line: u32,
    rust_col: u32,
    rust_line: u32,
    rava_lines: &[&str],
    rust_lines: &[&str],
) -> u32 {
    let Some(target) = rava_lines.get(rava_line.saturating_sub(1) as usize) else {
        return 1;
    };
    let first_word = |s: &str| -> u32 {
        s.chars().take_while(|c| c.is_whitespace()).count() as u32 + 1
    };
    let Some(rust) = rust_lines.get(rust_line.saturating_sub(1) as usize) else {
        return first_word(target);
    };

    let chars: Vec<char> = rust.chars().collect();
    let start = rust_col.saturating_sub(1) as usize;
    if start >= chars.len() {
        return first_word(target);
    }
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    if !is_word(chars[start]) {
        return first_word(target);
    }
    let mut end = start;
    while end < chars.len() && is_word(chars[end]) {
        end += 1;
    }
    let word: String = chars[start..end].iter().collect();

    match find_word(target, &word) {
        Some(col) => col,
        None => first_word(target),
    }
}

/// Position (1-basée, en caractères) du mot entier `word` dans `line`.
fn find_word(line: &str, word: &str) -> Option<u32> {
    let chars: Vec<char> = line.chars().collect();
    let w: Vec<char> = word.chars().collect();
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    for i in 0..chars.len().saturating_sub(w.len().saturating_sub(1)) {
        if chars[i..].starts_with(&w[..])
            && (i == 0 || !is_word(chars[i - 1]))
            && (i + w.len() >= chars.len() || !is_word(chars[i + w.len()]))
        {
            return Some(i as u32 + 1);
        }
    }
    None
}

/// Empreinte FNV-1a, juste assez pour distinguer deux sources.
fn digest(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Nom de crate valide : lettres, chiffres et `_`, ne commençant pas par un chiffre.
fn sanitize(name: &str) -> String {
    let stem = Path::new(name).file_stem().map(|s| s.to_string_lossy().into_owned());
    let base = stem.unwrap_or_else(|| name.to_string());
    let mut out: String =
        base.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    if out.is_empty() || out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn une_erreur_demprunt_atterrit_sur_la_bonne_ligne() {
        let src = "\
class A {
    public static void main(String[] args) {
        @Mut var v = Macro.vec(1, 2, 3);
        var premier = Ref.of(v[0]);
        v.push(4);
        System.out.println(\"{}\", premier);
    }
}";
        let r = check(src, "A", CrateType::Lib);
        if !r.rustc_available {
            return;
        }
        let e = r
            .diagnostics
            .iter()
            .find(|d| d.level == Level::Error)
            .expect("une erreur d'emprunt était attendue");
        assert_eq!(e.code.as_deref(), Some("E0502"));
        // `v.push(4);` est à la ligne 5 du .rava.
        assert_eq!(e.line, 5, "{e:?}");
        // Les positions liées sont ramenées, elles aussi.
        assert!(
            e.notes.iter().any(|n| n.starts_with("ligne 4 :")),
            "l'emprunt initial (ligne 4) doit être signalé : {:?}",
            e.notes
        );
        assert!(
            e.notes.iter().any(|n| n.starts_with("ligne 6 :")),
            "l'usage suivant (ligne 6) doit être signalé : {:?}",
            e.notes
        );
    }

    #[test]
    fn une_erreur_de_type_pointe_le_bon_identifiant() {
        let src = "\
class A {
    public static i32 f() {
        var s = \"x\".to_string();
        return s;
    }
}";
        let r = check(src, "A", CrateType::Lib);
        if !r.rustc_available {
            return;
        }
        let e = r.diagnostics.iter().find(|d| d.level == Level::Error).expect("erreur de type");
        assert_eq!(e.line, 4, "{e:?}");
        // La colonne doit désigner `s`, pas le début de la ligne.
        assert_eq!(e.col, 16, "{e:?}");
    }

    #[test]
    fn un_fichier_correct_ne_produit_aucune_erreur() {
        let src = "class A { public i32 f() { return 1; } }";
        let r = check(src, "A", CrateType::Lib);
        assert!(!r.has_errors(), "{:?}", r.diagnostics);
    }

    #[test]
    fn une_erreur_rava_court_circuite_rustc() {
        let r = check("class A { i32 f() { return null; } }", "A", CrateType::Lib);
        assert_eq!(r.diagnostics.len(), 1);
        assert_eq!(r.diagnostics[0].code.as_deref(), Some("rava.null"));
        assert!(r.rust.is_none(), "on ne génère pas de Rust si le .rava ne se lit pas");
    }

    #[test]
    fn nom_de_crate_assaini() {
        assert_eq!(sanitize("mon-fichier.rava"), "mon_fichier");
        assert_eq!(sanitize("2cool.rava"), "_2cool");
    }
}
