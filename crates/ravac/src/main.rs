//! ravac — compilateur Rava.
//!
//! Rava, c'est Rust avec la syntaxe de Java. `ravac` traduit un `.rava` en Rust,
//! puis délègue à `rustc` : le typage, l'emprunt et les durées de vie restent
//! exactement ceux de Rust.

use rava_check::{CrateType, Diagnostic, Level};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const USAGE: &str = "\
ravac — Rust avec la syntaxe de Java

USAGE:
    ravac emit  <fichier.rava>            Écrit le Rust généré sur la sortie standard
    ravac build <fichier.rava|dossier>    Génère les .rs (défaut : à côté du source)
    ravac check <fichier.rava>            Génère puis vérifie avec rustc
    ravac run   <fichier.rava> [args...]  Génère, compile et exécute

OPTIONS:
    -o, --out-dir <dir>   Répertoire de sortie pour `build`
    -h, --help            Affiche cette aide
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args[0] == "-h" || args[0] == "--help" {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    match run(&args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("erreur: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<ExitCode, String> {
    let cmd = args[0].as_str();
    let rest = &args[1..];

    match cmd {
        "emit" => {
            let path = need_path(rest)?;
            let rust = translate_file(&path)?;
            print!("{rust}");
            Ok(ExitCode::SUCCESS)
        }
        "build" => {
            let mut out_dir: Option<PathBuf> = None;
            let mut inputs = Vec::new();
            let mut it = rest.iter();
            while let Some(a) = it.next() {
                match a.as_str() {
                    "-o" | "--out-dir" => {
                        out_dir = Some(PathBuf::from(
                            it.next().ok_or("`-o` attend un répertoire")?,
                        ));
                    }
                    other => inputs.push(PathBuf::from(other)),
                }
            }
            if inputs.is_empty() {
                return Err("aucun fichier d'entrée".into());
            }
            let mut n = 0;
            for input in inputs {
                for src in collect_rava(&input)? {
                    let rust = translate_file(&src)?;
                    let dst = match &out_dir {
                        Some(d) => {
                            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
                            d.join(src.file_stem().unwrap()).with_extension("rs")
                        }
                        None => src.with_extension("rs"),
                    };
                    std::fs::write(&dst, rust).map_err(|e| e.to_string())?;
                    println!("{} -> {}", src.display(), dst.display());
                    n += 1;
                }
            }
            println!("{n} fichier(s) généré(s)");
            Ok(ExitCode::SUCCESS)
        }
        "check" | "run" => {
            let path = need_path(rest)?;
            let src =
                std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let crate_type =
                if cmd == "run" { CrateType::Bin } else { CrateType::Lib };
            let report = rava_check::check(&src, &path.to_string_lossy(), crate_type);

            for d in &report.diagnostics {
                eprint!("{}", render(&path, &src, d));
            }
            if !report.rustc_available {
                eprintln!(
                    "attention: `rustc` est introuvable — seule la syntaxe Rava a été vérifiée."
                );
            }
            if let Some(p) = &report.rust_path {
                if report.has_errors() {
                    eprintln!("Rust généré : {}", p.display());
                }
            }
            if report.has_errors() {
                return Ok(ExitCode::FAILURE);
            }

            if cmd == "check" {
                let warnings =
                    report.diagnostics.iter().filter(|d| d.level == Level::Warning).count();
                match warnings {
                    0 => println!("ok — {}", path.display()),
                    n => println!("ok — {} ({n} avertissement(s))", path.display()),
                }
                return Ok(ExitCode::SUCCESS);
            }

            let Some(bin) = report.binary else {
                return Err("aucun exécutable produit".into());
            };
            let status = Command::new(&bin)
                .args(rest.iter().skip(1))
                .status()
                .map_err(|e| e.to_string())?;
            Ok(if status.success() { ExitCode::SUCCESS } else { ExitCode::FAILURE })
        }
        other => Err(format!("commande inconnue `{other}`\n\n{USAGE}")),
    }
}

fn need_path(rest: &[String]) -> Result<PathBuf, String> {
    rest.first().map(PathBuf::from).ok_or_else(|| "fichier .rava attendu".to_string())
}

fn collect_rava(p: &Path) -> Result<Vec<PathBuf>, String> {
    if p.is_file() {
        return Ok(vec![p.to_path_buf()]);
    }
    let mut out = Vec::new();
    let entries = std::fs::read_dir(p).map_err(|e| format!("{}: {e}", p.display()))?;
    for e in entries {
        let e = e.map_err(|e| e.to_string())?;
        let path = e.path();
        if path.is_dir() {
            out.extend(collect_rava(&path)?);
        } else if path.extension().is_some_and(|x| x == "rava") {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}

fn translate_file(path: &Path) -> Result<String, String> {
    let src = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let unit = rava_parser::parse(&src).map_err(|e| {
        render(
            path,
            &src,
            &Diagnostic {
                level: Level::Error,
                code: e.code.map(str::to_string),
                message: e.message,
                line: e.line,
                col: e.col,
                rust_pos: None,
                notes: e.note.into_iter().collect(),
            },
        )
    })?;
    rava_codegen::generate(&unit).map_err(|e| {
        render(
            path,
            &src,
            &Diagnostic {
                level: Level::Error,
                code: e.code.map(str::to_string),
                message: e.message,
                line: e.line,
                col: e.col,
                rust_pos: None,
                notes: e.note.into_iter().collect(),
            },
        )
    })
}

/// Diagnostic à la manière de rustc, toujours ancré sur le source `.rava` —
/// y compris quand il vient de `rustc` lui-même.
fn render(path: &Path, src: &str, d: &Diagnostic) -> String {
    let head = match &d.code {
        Some(c) => format!("{}[{c}]", d.level.label()),
        None => d.level.label().to_string(),
    };
    let mut out = format!("{head}: {}\n  --> {}:{}:{}\n", d.message, path.display(), d.line, d.col);

    if let Some(text) = src.lines().nth(d.line.saturating_sub(1) as usize) {
        let gutter = d.line.to_string().len();
        out.push_str(&format!("{:w$} |\n", "", w = gutter));
        out.push_str(&format!("{} | {text}\n", d.line));
        out.push_str(&format!(
            "{:w$} | {:c$}^\n",
            "",
            "",
            w = gutter,
            c = d.col.saturating_sub(1) as usize
        ));
    }
    for n in &d.notes {
        out.push_str(&format!("  = {n}\n"));
    }
    if let Some((l, c)) = d.rust_pos {
        out.push_str(&format!("  = rustc, dans le Rust généré, ligne {l} colonne {c}\n"));
    }
    out.push('\n');
    out
}
