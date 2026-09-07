//! Lecture de `rava.toml` et écriture du `Cargo.toml` correspondant.
//!
//! On ne lit que ce dont on a besoin — le nom du paquet, l'édition. Tout le
//! reste, dépendances comprises, est **recopié tel quel** : le manifeste Rava
//! est un manifeste Cargo, et l'écosystème de crates reste accessible sans que
//! Rava ait à comprendre chaque option.

use std::path::Path;

/// Racine du crate généré. Le nom est volontairement improbable : il ne doit
/// entrer en collision avec aucune classe, sur aucun système de fichiers.
pub const ROOT_FILE: &str = "src/__rava_root.rs";

#[derive(Debug, Clone)]
pub struct Manifest {
    pub name: String,
    pub edition: String,
    /// Texte d'origine, réémis dans le `Cargo.toml` généré.
    pub raw: String,
}

impl Manifest {
    /// Manifeste implicite, pour un répertoire sans `rava.toml`.
    pub fn implicit(dir: &Path) -> Manifest {
        let name = dir
            .file_name()
            .map(|n| sanitize(&n.to_string_lossy()))
            .unwrap_or_else(|| "rava_projet".to_string());
        Manifest {
            raw: format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n"),
            name,
            edition: "2021".to_string(),
        }
    }

    pub fn parse(text: &str, dir: &Path) -> Manifest {
        let mut name = None;
        let mut edition = None;
        let mut section = String::new();
        for line in text.lines() {
            let l = line.trim();
            if l.starts_with('[') {
                section = l.trim_matches(['[', ']']).to_string();
                continue;
            }
            if section != "package" {
                continue;
            }
            let Some((k, v)) = l.split_once('=') else { continue };
            let v = v.trim().trim_matches('"').to_string();
            match k.trim() {
                "name" => name = Some(sanitize(&v)),
                "edition" => edition = Some(v),
                _ => {}
            }
        }
        Manifest {
            name: name.unwrap_or_else(|| Manifest::implicit(dir).name),
            edition: edition.unwrap_or_else(|| "2021".to_string()),
            raw: text.to_string(),
        }
    }

    /// `Cargo.toml` du projet généré : le manifeste Rava, complété de ce que
    /// Cargo exige et que l'on n'oblige pas à écrire.
    ///
    /// La racine du crate est nommée explicitement plutôt que laissée aux
    /// conventions `src/main.rs` / `src/lib.rs` : une classe `Main` produirait
    /// `src/Main.rs`, qui est le *même fichier* sur un système insensible à la
    /// casse — macOS, Windows.
    pub fn to_cargo_toml(&self, is_binary: bool) -> String {
        let mut out = String::new();
        let has_package = self.raw.lines().any(|l| l.trim() == "[package]");
        if !has_package {
            out.push_str("[package]\n");
        }
        let mut in_package = !has_package;
        let mut seen_version = false;
        let mut seen_edition = false;
        let mut inserted = false;

        for line in self.raw.lines() {
            let l = line.trim();
            if l.starts_with('[') {
                // Fin de `[package]` : on y ajoute ce qui manque avant de sortir.
                if in_package && !inserted {
                    if !seen_version {
                        out.push_str("version = \"0.1.0\"\n");
                    }
                    if !seen_edition {
                        out.push_str(&format!("edition = \"{}\"\n", self.edition));
                    }
                    inserted = true;
                }
                in_package = l == "[package]";
                out.push_str(line);
                out.push('\n');
                continue;
            }
            if in_package {
                let key = l.split_once('=').map(|(k, _)| k.trim()).unwrap_or("");
                seen_version |= key == "version";
                seen_edition |= key == "edition";
            }
            out.push_str(line);
            out.push('\n');
        }
        if in_package && !inserted {
            if !seen_version {
                out.push_str("version = \"0.1.0\"\n");
            }
            if !seen_edition {
                out.push_str(&format!("edition = \"{}\"\n", self.edition));
            }
        }
        let declares_target =
            self.raw.lines().any(|l| matches!(l.trim(), "[[bin]]" | "[lib]"));
        if !declares_target {
            if is_binary {
                out.push_str(&format!(
                    "\n[[bin]]\nname = \"{}\"\npath = \"{}\"\n",
                    self.name, ROOT_FILE
                ));
            } else {
                out.push_str(&format!(
                    "\n[lib]\nname = \"{}\"\npath = \"{}\"\n",
                    self.name.replace('-', "_"),
                    ROOT_FILE
                ));
            }
        }
        // Le projet généré se tient seul : il peut vivre sous n'importe quel
        // répertoire, y compris à l'intérieur d'un espace de travail Rust.
        if !self.raw.lines().any(|l| l.trim() == "[workspace]") {
            out.push_str("\n[workspace]\n");
        }
        out
    }
}

/// Nom de paquet Cargo valide.
fn sanitize(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    if out.is_empty() || out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_ce_que_cargo_exige() {
        let m = Manifest::parse("[package]\nname = \"demo\"\n", Path::new("/tmp/x"));
        let toml = m.to_cargo_toml(true);
        assert!(toml.contains("name = \"demo\""));
        assert!(toml.contains("version = \"0.1.0\""));
        assert!(toml.contains("edition = \"2021\""));
        // Le projet généré ne doit pas être happé par un espace de travail voisin.
        assert!(toml.contains("[workspace]"));
        // La racine est nommée : `src/main.rs` entrerait en collision avec une
        // classe `Main` sur un système insensible à la casse.
        assert!(toml.contains("path = \"src/__rava_root.rs\""), "{toml}");
    }

    #[test]
    fn une_cible_declaree_a_la_main_est_respectee() {
        let src = "[package]\nname = \"demo\"\n\n[lib]\nname = \"autre\"\npath = \"src/x.rs\"\n";
        let toml = Manifest::parse(src, Path::new("/tmp/x")).to_cargo_toml(false);
        assert_eq!(toml.matches("[lib]").count(), 1, "{toml}");
    }

    #[test]
    fn recopie_les_dependances_telles_quelles() {
        let src = "[package]\nname = \"demo\"\nversion = \"2.0.0\"\n\n\
                   [dependencies]\nserde = { version = \"1\", features = [\"derive\"] }\n";
        let toml = Manifest::parse(src, Path::new("/tmp/x")).to_cargo_toml(false);
        assert!(toml.contains("serde = { version = \"1\", features = [\"derive\"] }"));
        assert!(toml.contains("version = \"2.0.0\""));
        assert_eq!(toml.matches("version =").count(), 2, "{toml}");
    }

    #[test]
    fn nom_implicite_depuis_le_repertoire() {
        assert_eq!(Manifest::implicit(Path::new("/a/mon projet")).name, "mon_projet");
    }
}
