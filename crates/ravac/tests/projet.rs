//! Test de bout en bout du modèle de projet : paquets, visibilité, imports
//! entre paquets, et diagnostics ramenés sur le bon fichier.

use std::path::{Path, PathBuf};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn out(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("rava-test-projet-{name}"));
    let _ = std::fs::remove_dir_all(&d);
    d
}

#[test]
fn le_projet_dexemple_compile() {
    let root = repo().join("examples/projet");
    let report = rava_check::check_project(&root, &out("exemple"), true);
    if !report.cargo_available {
        return;
    }
    assert!(
        !report.has_errors(),
        "{:#?}",
        report.diagnostics.iter().map(|d| (&d.file, d.line, &d.message)).collect::<Vec<_>>()
    );
    let built = report.built.expect("projet généré");
    assert!(built.is_binary, "le projet déclare un `main`");
    assert_eq!(built.files.len(), 4);
}

#[test]
fn un_import_entre_paquets_devient_un_chemin_de_crate() {
    let root = repo().join("examples/projet");
    let project = rava_build::discover(&root).unwrap();
    let built = rava_build::emit(&project, &out("imports")).unwrap();

    let compte = built
        .files
        .iter()
        .find(|f| f.rava_path.ends_with("Compte.rava"))
        .expect("Compte.rava");
    assert!(compte.rust_source.contains("use crate::geo::Point;"), "{}", compte.rust_source);
    // Même paquet = visible sans import, comme en Java.
    assert!(compte.rust_source.contains("use super::*;"));

    // La racine déclare les paquets, chaque paquet aplatit ses fichiers.
    let root_rs =
        std::fs::read_to_string(out("imports").join("src/__rava_root.rs")).unwrap_or_default();
    let _ = root_rs;
}

#[test]
fn un_paquet_qui_ne_correspond_pas_au_repertoire_est_refuse() {
    let dir = out("mismatch");
    std::fs::create_dir_all(dir.join("src/geo")).unwrap();
    std::fs::write(dir.join("rava.toml"), "[package]\nname = \"x\"\n").unwrap();
    std::fs::write(
        dir.join("src/geo/Point.rava"),
        "package formes;\npublic class Point { }\n",
    )
    .unwrap();

    let project = rava_build::discover(&dir).unwrap();
    let errs = rava_build::emit(&project, &out("mismatch-out")).unwrap_err();
    assert_eq!(errs[0].code.as_deref(), Some("rava.package-mismatch"));
    assert!(errs[0].message.contains("formes"), "{}", errs[0].message);
}

#[test]
fn une_erreur_de_rustc_designe_le_bon_fichier_du_projet() {
    let dir = out("emprunt");
    std::fs::create_dir_all(dir.join("src/geo")).unwrap();
    std::fs::write(dir.join("rava.toml"), "[package]\nname = \"emprunt-test\"\n").unwrap();
    std::fs::write(
        dir.join("src/Main.rava"),
        "public class Main {\n  public static void main(String[] a) { }\n}\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/geo/Boite.rava"),
        "package geo;\n\
         public class Boite {\n\
         \x20 private Vec<String> items;\n\
         \x20 public Boite() { this.items = Macro.vec(); }\n\
         \x20 @Mut public void ajouter(String s) {\n\
         \x20   var premier = Ref.of(this.items[0]);\n\
         \x20   this.items.push(s);\n\
         \x20   System.out.println(\"{}\", premier);\n\
         \x20 }\n\
         }\n",
    )
    .unwrap();

    let report = rava_check::check_project(&dir, &out("emprunt-out"), true);
    if !report.cargo_available {
        return;
    }
    let e = report
        .diagnostics
        .iter()
        .find(|d| d.level == rava_check::Level::Error)
        .expect("une erreur d'emprunt était attendue");
    assert_eq!(e.code.as_deref(), Some("E0502"));
    assert!(e.file.ends_with("geo/Boite.rava"), "{:?}", e.file);
    assert_eq!(e.line, 7, "{e:?}");
}
