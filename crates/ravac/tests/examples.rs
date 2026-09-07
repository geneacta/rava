//! Test de bout en bout : chaque exemple `.rava` doit produire du Rust que
//! `rustc` accepte. C'est la garantie centrale de Rava — la sémantique reste
//! celle de Rust, donc c'est rustc qui a le dernier mot.

use std::path::{Path, PathBuf};
use std::process::Command;

fn examples_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

fn translate(path: &Path) -> String {
    let src = std::fs::read_to_string(path).expect("lecture du source");
    let unit = rava_parser::parse(&src)
        .unwrap_or_else(|e| panic!("{}: erreur de syntaxe: {e}", path.display()));
    rava_codegen::generate(&unit)
        .unwrap_or_else(|e| panic!("{}: erreur de génération: {e}", path.display()))
}

#[test]
fn tous_les_exemples_compilent_avec_rustc() {
    let dir = examples_dir();
    let out = std::env::temp_dir().join("rava-tests-examples");
    std::fs::create_dir_all(&out).unwrap();

    let mut checked = 0;
    for entry in std::fs::read_dir(&dir).expect("répertoire examples/") {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "rava") {
            continue;
        }
        let rust = translate(&path);
        let rs = out.join(path.file_stem().unwrap()).with_extension("rs");
        std::fs::write(&rs, &rust).unwrap();

        let status = Command::new("rustc")
            .arg("--edition=2021")
            .arg("--emit=metadata")
            .arg("--crate-type=lib")
            .arg("-o")
            .arg(out.join("meta"))
            .arg(&rs)
            .output()
            .expect("rustc");
        assert!(
            status.status.success(),
            "{} ne compile pas :\n{}\n--- Rust généré ---\n{rust}",
            path.display(),
            String::from_utf8_lossy(&status.stderr)
        );
        checked += 1;
    }
    assert!(checked > 0, "aucun exemple trouvé dans {}", dir.display());
}
