//! ravac — compilateur Rava.
//!
//! Rava, c'est Rust avec la syntaxe de Java. `ravac` traduit, puis délègue à
//! `rustc` pour un fichier isolé, à `cargo` pour un projet. Les diagnostics
//! reviennent toujours ancrés sur le `.rava`.

use rava_check::{CrateType, Diagnostic, Level};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const USAGE: &str = "\
ravac — Rust avec la syntaxe de Java

USAGE:
    ravac new   <nom>                     Crée un projet Rava
    ravac emit  <fichier.rava>            Écrit le Rust généré sur la sortie standard
    ravac build [chemin]                  Traduit un projet ou un fichier
    ravac check [chemin]                  Traduit puis vérifie (cargo, ou rustc)
    ravac run   [chemin] [-- args...]     Traduit, compile et exécute

`chemin` vaut « . » par défaut. Un répertoire est traité comme un projet
(il porte un `rava.toml`, ou un `src/`), un `.rava` comme un fichier isolé.

OPTIONS:
    -o, --out-dir <dir>   Répertoire de sortie (défaut : <projet>/target/rava)
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

/// Options communes aux commandes qui prennent un chemin.
struct Opts {
    path: PathBuf,
    out_dir: Option<PathBuf>,
    /// Arguments après `--`, transmis au programme exécuté.
    passthrough: Vec<String>,
}

fn parse_opts(rest: &[String]) -> Result<Opts, String> {
    let mut path = None;
    let mut out_dir = None;
    let mut passthrough = Vec::new();
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--" => {
                passthrough.extend(it.cloned());
                break;
            }
            "-o" | "--out-dir" => {
                out_dir = Some(PathBuf::from(it.next().ok_or("`-o` attend un répertoire")?));
            }
            other if path.is_none() => path = Some(PathBuf::from(other)),
            other => return Err(format!("argument inattendu : `{other}`")),
        }
    }
    Ok(Opts { path: path.unwrap_or_else(|| PathBuf::from(".")), out_dir, passthrough })
}

fn run(args: &[String]) -> Result<ExitCode, String> {
    let cmd = args[0].as_str();
    let rest = &args[1..];

    match cmd {
        "new" => {
            let name = rest.first().ok_or("`new` attend un nom de projet")?;
            scaffold(Path::new(name))?;
            println!("projet créé : {name}\n\n  cd {name}\n  ravac run");
            Ok(ExitCode::SUCCESS)
        }

        "emit" => {
            let path = parse_opts(rest)?.path;
            print!("{}", translate_file(&path)?);
            Ok(ExitCode::SUCCESS)
        }

        "build" | "check" | "run" => {
            let opts = parse_opts(rest)?;
            if opts.path.is_file() {
                single_file(cmd, &opts)
            } else if opts.path.is_dir() {
                project(cmd, &opts)
            } else {
                Err(format!("{} : introuvable", opts.path.display()))
            }
        }

        other => Err(format!("commande inconnue `{other}`\n\n{USAGE}")),
    }
}

// ---------------------------------------------------------------- projet

fn project(cmd: &str, opts: &Opts) -> Result<ExitCode, String> {
    if !rava_build::is_project(&opts.path) {
        return Err(format!(
            "{} n'est pas un projet Rava : il lui faut un `rava.toml`, ou un `src/` contenant des .rava.\n\
             Créez-en un avec `ravac new <nom>`.",
            opts.path.display()
        ));
    }
    let out_dir = opts
        .out_dir
        .clone()
        .unwrap_or_else(|| opts.path.join("target").join("rava"));

    let report = rava_check::check_project(&opts.path, &out_dir, cmd != "build");
    let sources = SourceCache::default();
    for d in &report.diagnostics {
        eprint!("{}", render(d, &sources));
    }
    if !report.cargo_available {
        eprintln!("attention: `cargo` est introuvable — seule la traduction a été faite.");
    }
    if report.has_errors() {
        eprintln!("Projet généré : {}", out_dir.display());
        return Ok(ExitCode::FAILURE);
    }
    let Some(built) = report.built else {
        return Err("la traduction n'a rien produit".into());
    };

    match cmd {
        "build" => {
            println!(
                "{} fichier(s) traduit(s) -> {}",
                built.files.len(),
                out_dir.display()
            );
            Ok(ExitCode::SUCCESS)
        }
        "check" => {
            let warnings =
                report.diagnostics.iter().filter(|d| d.level == Level::Warning).count();
            match warnings {
                0 => println!("ok — {}", opts.path.display()),
                n => println!("ok — {} ({n} avertissement(s))", opts.path.display()),
            }
            Ok(ExitCode::SUCCESS)
        }
        _ => {
            if !built.is_binary {
                return Err(
                    "ce projet est une bibliothèque : aucune méthode `public static void main`."
                        .into(),
                );
            }
            let status = Command::new("cargo")
                .arg("run")
                .arg("--quiet")
                .arg("--manifest-path")
                .arg(out_dir.join("Cargo.toml"))
                .arg("--")
                .args(&opts.passthrough)
                .status()
                .map_err(|e| format!("cargo introuvable: {e}"))?;
            Ok(if status.success() { ExitCode::SUCCESS } else { ExitCode::FAILURE })
        }
    }
}

// ---------------------------------------------------------------- fichier isolé

fn single_file(cmd: &str, opts: &Opts) -> Result<ExitCode, String> {
    let path = &opts.path;
    if cmd == "build" {
        let rust = translate_file(path)?;
        let dst = match &opts.out_dir {
            Some(d) => {
                std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
                d.join(path.file_stem().unwrap()).with_extension("rs")
            }
            None => path.with_extension("rs"),
        };
        std::fs::write(&dst, rust).map_err(|e| e.to_string())?;
        println!("{} -> {}", path.display(), dst.display());
        return Ok(ExitCode::SUCCESS);
    }

    let src = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let crate_type = if cmd == "run" { CrateType::Bin } else { CrateType::Lib };
    let report = rava_check::check(&src, &path.to_string_lossy(), crate_type);

    let sources = SourceCache::default();
    for d in &report.diagnostics {
        eprint!("{}", render(d, &sources));
    }
    if !report.rustc_available {
        eprintln!("attention: `rustc` est introuvable — seule la syntaxe Rava a été vérifiée.");
    }
    if report.has_errors() {
        if let Some(p) = &report.rust_path {
            eprintln!("Rust généré : {}", p.display());
        }
        return Ok(ExitCode::FAILURE);
    }

    if cmd == "check" {
        let warnings = report.diagnostics.iter().filter(|d| d.level == Level::Warning).count();
        match warnings {
            0 => println!("ok — {}", path.display()),
            n => println!("ok — {} ({n} avertissement(s))", path.display()),
        }
        return Ok(ExitCode::SUCCESS);
    }

    let bin = report.binary.ok_or("aucun exécutable produit")?;
    let status = Command::new(&bin)
        .args(&opts.passthrough)
        .status()
        .map_err(|e| e.to_string())?;
    Ok(if status.success() { ExitCode::SUCCESS } else { ExitCode::FAILURE })
}

fn translate_file(path: &Path) -> Result<String, String> {
    let src = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let sources = SourceCache::default();
    let diags = rava_check::check_syntax(&src, path);
    if let Some(d) = diags.first() {
        return Err(render(d, &sources).trim_end().to_string());
    }
    let unit = rava_parser::parse(&src).map_err(|e| e.message)?;
    rava_codegen::generate(&unit).map_err(|e| e.message)
}

// ---------------------------------------------------------------- rendu

/// Les sources ne sont lues qu'une fois, quel que soit le nombre de
/// diagnostics qui les concernent.
#[derive(Default)]
struct SourceCache {
    cache: std::cell::RefCell<std::collections::HashMap<PathBuf, Option<String>>>,
}

impl SourceCache {
    fn line(&self, path: &Path, line: u32) -> Option<String> {
        let mut cache = self.cache.borrow_mut();
        let text = cache
            .entry(path.to_path_buf())
            .or_insert_with(|| std::fs::read_to_string(path).ok());
        text.as_ref()?.lines().nth(line.saturating_sub(1) as usize).map(str::to_string)
    }
}

/// Diagnostic à la manière de rustc, toujours ancré sur le source `.rava` —
/// y compris quand il vient de `rustc` ou de `cargo`.
fn render(d: &Diagnostic, sources: &SourceCache) -> String {
    let head = match &d.code {
        Some(c) => format!("{}[{c}]", d.level.label()),
        None => d.level.label().to_string(),
    };
    let mut out = format!("{head}: {}\n", d.message);
    if !d.file.as_os_str().is_empty() {
        out.push_str(&format!("  --> {}:{}:{}\n", d.file.display(), d.line, d.col));
        if let Some(text) = sources.line(&d.file, d.line) {
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

// ---------------------------------------------------------------- squelette

fn scaffold(dir: &Path) -> Result<(), String> {
    if dir.exists() {
        return Err(format!("{} existe déjà", dir.display()));
    }
    let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    std::fs::create_dir_all(dir.join("src")).map_err(|e| e.to_string())?;

    let write = |rel: &str, content: String| -> Result<(), String> {
        let p = dir.join(rel);
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        std::fs::write(p, content).map_err(|e| e.to_string())
    };

    write(
        "rava.toml",
        format!(
            "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n\n\
             [dependencies]\n\
             # Recopiées telles quelles dans le Cargo.toml généré :\n\
             # tout l'écosystème Rust reste accessible.\n"
        ),
    )?;
    write(
        ".gitignore",
        "target/\n".to_string(),
    )?;
    write(
        "src/Main.rava",
        "public class Main {\n\n\
         \x20   public static void main(String[] args) {\n\
         \x20       System.out.println(\"Bonjour, {} !\", \"Rava\");\n\
         \x20   }\n\
         }\n"
            .to_string(),
    )?;
    Ok(())
}
