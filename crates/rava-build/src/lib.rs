//! Projets Rava : plusieurs fichiers, des paquets, un `Cargo.toml`.
//!
//! Le modèle est celui de Java, projeté sur celui de Rust :
//!
//! - un **paquet** est le seul espace de noms ; le fichier n'en est pas un ;
//! - le répertoire fait foi, comme en Java : `src/geo/Point.rava` est dans le
//!   paquet `geo`, et sa déclaration `package geo;` doit le confirmer ;
//! - tous les fichiers d'un paquet se retrouvent dans le **même module** Rust,
//!   donc se voient sans `import` — exactement la règle Java.
//!
//! Rava ne compile pas : il écrit un projet Cargo et laisse `cargo` faire.

pub mod manifest;

use manifest::Manifest;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

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

/// Un diagnostic, toujours exprimé en coordonnées `.rava`.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub level: Level,
    /// `E0502` pour rustc, `rava.null` pour les refus de Rava.
    pub code: Option<String>,
    pub message: String,
    /// Fichier concerné ; vide pour un diagnostic sans origine.
    pub file: PathBuf,
    pub line: u32,
    pub col: u32,
    /// Position d'origine dans le Rust généré, quand le diagnostic en vient.
    pub rust_pos: Option<(u32, u32)>,
    pub notes: Vec<String>,
}

impl Diagnostic {
    pub fn error(file: &Path, line: u32, col: u32, message: impl Into<String>) -> Diagnostic {
        Diagnostic {
            level: Level::Error,
            code: None,
            message: message.into(),
            file: file.to_path_buf(),
            line,
            col,
            rust_pos: None,
            notes: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------- découverte

#[derive(Debug, Clone)]
pub struct SourceFile {
    /// Chemin du `.rava`.
    pub path: PathBuf,
    /// Paquet, dérivé du répertoire relatif à `src/`.
    pub package: Vec<String>,
    /// Nom du module-fichier : `Point` pour `Point.rava`.
    pub stem: String,
    pub source: String,
}

#[derive(Debug, Clone)]
pub struct Project {
    pub root: PathBuf,
    pub src_dir: PathBuf,
    pub manifest: Manifest,
    pub files: Vec<SourceFile>,
}

/// Un répertoire est un projet Rava s'il porte un `rava.toml`, ou un `src/`
/// contenant au moins un `.rava`.
pub fn is_project(dir: &Path) -> bool {
    dir.join("rava.toml").is_file()
        || (dir.join("src").is_dir() && !collect(&dir.join("src")).is_empty())
}

pub fn discover(root: &Path) -> Result<Project, Diagnostic> {
    let manifest_path = root.join("rava.toml");
    let manifest = match std::fs::read_to_string(&manifest_path) {
        Ok(t) => Manifest::parse(&t, root),
        Err(_) => Manifest::implicit(root),
    };

    let src_dir = if root.join("src").is_dir() { root.join("src") } else { root.to_path_buf() };
    let paths = collect(&src_dir);
    if paths.is_empty() {
        return Err(Diagnostic::error(
            root,
            1,
            1,
            format!("aucun fichier .rava trouvé dans {}", src_dir.display()),
        ));
    }

    let mut files = Vec::new();
    for path in paths {
        let source = std::fs::read_to_string(&path)
            .map_err(|e| Diagnostic::error(&path, 1, 1, format!("lecture impossible : {e}")))?;
        let rel = path.strip_prefix(&src_dir).unwrap_or(&path);
        let package: Vec<String> = rel
            .parent()
            .map(|p| p.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect())
            .unwrap_or_default();
        let stem = path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        files.push(SourceFile { path, package, stem, source });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(Project { root: root.to_path_buf(), src_dir, manifest, files })
}

fn collect(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else { return out };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(collect(&p));
        } else if p.extension().is_some_and(|x| x == "rava") {
            out.push(p);
        }
    }
    out.sort();
    out
}

// ---------------------------------------------------------------- génération

/// Un fichier `.rs` produit, et de quoi remonter à sa source.
#[derive(Debug, Clone)]
pub struct BuiltFile {
    pub rust_path: PathBuf,
    /// Chemin tel que `cargo` le nommera dans ses diagnostics.
    pub cargo_name: String,
    pub rava_path: PathBuf,
    pub rava_source: String,
    pub rust_source: String,
    /// Ligne `.rava` d'origine de chaque ligne générée.
    pub map: Vec<u32>,
}

#[derive(Debug, Clone)]
pub struct Built {
    pub out_dir: PathBuf,
    pub crate_name: String,
    pub files: Vec<BuiltFile>,
    /// Vrai si le projet produit un exécutable (un `main` a été trouvé).
    pub is_binary: bool,
}

/// Traduit tout le projet dans `out_dir` : arborescence de modules,
/// fichiers de paquet et `Cargo.toml`.
pub fn emit(project: &Project, out_dir: &Path) -> Result<Built, Vec<Diagnostic>> {
    let mut errors = Vec::new();
    let mut units = Vec::new();

    // Les premiers segments des paquets : ce qui distingue `import geo.Point`
    // d'un `import std.collections.HashMap`.
    let mut crate_roots: Vec<String> = project
        .files
        .iter()
        .filter_map(|f| f.package.first().cloned())
        .collect();
    crate_roots.sort();
    crate_roots.dedup();

    for f in &project.files {
        match rava_parser::parse(&f.source) {
            Err(e) => errors.push(Diagnostic {
                level: Level::Error,
                code: e.code.map(str::to_string),
                message: e.message,
                file: f.path.clone(),
                line: e.line,
                col: e.col,
                rust_pos: None,
                notes: e.note.into_iter().collect(),
            }),
            Ok(unit) => {
                // Comme en Java, le répertoire fait foi.
                if let Some(declared) = &unit.package {
                    if *declared != f.package {
                        errors.push(Diagnostic {
                            level: Level::Error,
                            code: Some("rava.package-mismatch".into()),
                            message: format!(
                                "le paquet déclaré `{}` ne correspond pas au répertoire `{}`",
                                declared.join("."),
                                if f.package.is_empty() {
                                    "src".to_string()
                                } else {
                                    format!("src/{}", f.package.join("/"))
                                }
                            ),
                            file: f.path.clone(),
                            line: 1,
                            col: 1,
                            rust_pos: None,
                            notes: vec![format!(
                                "déplacez le fichier dans `src/{}/`, ou corrigez la déclaration",
                                declared.join("/")
                            )],
                        });
                        continue;
                    }
                }
                units.push((f, unit));
            }
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }

    let ctx_for = |f: &SourceFile| rava_codegen::Context {
        crate_roots: crate_roots.clone(),
        in_package: !f.package.is_empty(),
    };

    let mut files = Vec::new();
    let mut is_binary = false;
    // Contenu de chaque module : sous-paquets et fichiers.
    let mut modules: BTreeMap<Vec<String>, ModuleContent> = BTreeMap::new();
    modules.entry(Vec::new()).or_default();

    for (f, unit) in &units {
        let out = match rava_codegen::generate_with_map_in(unit, &ctx_for(f)) {
            Ok(o) => o,
            Err(e) => {
                errors.push(Diagnostic {
                    level: Level::Error,
                    code: e.code.map(str::to_string),
                    message: e.message,
                    file: f.path.clone(),
                    line: e.line,
                    col: e.col,
                    rust_pos: None,
                    notes: e.note.into_iter().collect(),
                });
                continue;
            }
        };
        is_binary |= declares_main(unit);

        let rel_dir: PathBuf = f.package.iter().collect();
        let rust_path = out_dir.join("src").join(&rel_dir).join(format!("{}.rs", f.stem));
        let cargo_name = {
            let mut p = PathBuf::from("src");
            p.push(&rel_dir);
            p.push(format!("{}.rs", f.stem));
            p.to_string_lossy().into_owned()
        };

        modules.entry(f.package.clone()).or_default().files.push(f.stem.clone());
        // Chaque niveau doit déclarer le suivant.
        for i in 0..f.package.len() {
            let parent = f.package[..i].to_vec();
            let child = f.package[i].clone();
            let entry = modules.entry(parent).or_default();
            if !entry.submodules.contains(&child) {
                entry.submodules.push(child);
            }
            modules.entry(f.package[..=i].to_vec()).or_default();
        }

        files.push(BuiltFile {
            rust_path,
            cargo_name,
            rava_path: f.path.clone(),
            rava_source: f.source.clone(),
            rust_source: out.rust,
            map: out.map,
        });
    }
    if !errors.is_empty() {
        return Err(errors);
    }

    // Le répertoire de sortie est entièrement régénéré : un fichier laissé par
    // une génération précédente troublerait Cargo (un `lib.rs` orphelin à côté
    // d'un `main.rs`, par exemple). Le marqueur évite d'effacer un répertoire
    // qui ne nous appartiendrait pas.
    let marker = out_dir.join(".rava-build");
    if out_dir.exists()
        && !marker.exists()
        && out_dir.read_dir().is_ok_and(|mut d| d.next().is_some())
    {
        return Err(vec![Diagnostic::error(
            out_dir,
            1,
            1,
            format!("{} existe déjà et n'a pas été produit par ravac", out_dir.display()),
        )]);
    }
    let _ = std::fs::remove_dir_all(out_dir.join("src"));

    // Écriture.
    let write = |path: &Path, content: &str| -> Result<(), Diagnostic> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| {
                Diagnostic::error(path, 1, 1, format!("création de {} : {e}", dir.display()))
            })?;
        }
        std::fs::write(path, content)
            .map_err(|e| Diagnostic::error(path, 1, 1, format!("écriture : {e}")))
    };

    let mut io = Vec::new();
    for bf in &files {
        if let Err(e) = write(&bf.rust_path, &bf.rust_source) {
            io.push(e);
        }
    }
    for (package, content) in &modules {
        let path = if package.is_empty() {
            out_dir.join(manifest::ROOT_FILE)
        } else {
            let rel: PathBuf = package.iter().collect();
            out_dir.join("src").join(rel).join("mod.rs")
        };
        if let Err(e) = write(&path, &content.render(package.is_empty())) {
            io.push(e);
        }
    }
    if let Err(e) =
        write(&out_dir.join("Cargo.toml"), &project.manifest.to_cargo_toml(is_binary))
    {
        io.push(e);
    }
    if let Err(e) = write(&marker, "Répertoire généré par ravac. Ne rien y écrire à la main.\n") {
        io.push(e);
    }
    if !io.is_empty() {
        return Err(io);
    }

    Ok(Built {
        out_dir: out_dir.to_path_buf(),
        crate_name: project.manifest.name.clone(),
        files,
        is_binary,
    })
}

/// Le projet produit-il un exécutable ? On le lit dans l'arbre, pas dans le
/// texte généré.
fn declares_main(unit: &rava_ast::Unit) -> bool {
    unit.items.iter().any(|i| match i {
        rava_ast::Item::Class(c) => c.methods.iter().any(|m| {
            m.name == "main"
                && m.modifiers.contains(&rava_ast::Modifier::Static)
                && matches!(m.ret.kind, rava_ast::TypeKind::Void)
        }),
        _ => false,
    })
}

#[derive(Debug, Default, Clone)]
struct ModuleContent {
    submodules: Vec<String>,
    files: Vec<String>,
}

impl ModuleContent {
    /// Le fichier de module : il déclare les sous-paquets, puis aplatit les
    /// fichiers du paquet — en Java, le fichier n'est pas un espace de noms.
    fn render(&self, is_root: bool) -> String {
        let mut out = String::new();
        out.push_str("// Généré par ravac : ce module est un paquet Rava.\n");
        if is_root {
            out.push_str("#![allow(non_snake_case, non_camel_case_types, unused_parens, dead_code)]\n");
        }
        // Un paquet expose tout ce qu'il contient, y compris ce que personne
        // n'utilise encore : c'est la règle Java, pas un oubli.
        out.push_str("#![allow(unused_imports)]\n");
        out.push('\n');
        let mut subs = self.submodules.clone();
        subs.sort();
        for s in &subs {
            out.push_str(&format!("pub mod {s};\n"));
        }
        if !subs.is_empty() {
            out.push('\n');
        }
        let mut files = self.files.clone();
        files.sort();
        for f in &files {
            out.push_str(&format!("#[path = \"{f}.rs\"]\nmod __rava_file_{f};\n"));
            out.push_str(&format!("pub use __rava_file_{f}::*;\n"));
        }
        out
    }
}
