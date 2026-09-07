# Projets Rava

Un fichier isolé suffit pour essayer. Dès qu'il y en a plusieurs, il faut des
paquets — et un projet.

```sh
ravac new ma-banque
cd ma-banque
ravac run
```

---

## Arborescence

```
ma-banque/
├── rava.toml
└── src/
    ├── Main.rava              (paquet racine)
    ├── geo/
    │   ├── Point.rava         package geo;
    │   └── Forme.rava         package geo;
    └── banque/
        └── Compte.rava        package banque;
```

Comme en Java, **le répertoire fait foi** : `src/geo/Point.rava` est dans le
paquet `geo`, et sa déclaration `package geo;` doit le confirmer. Un désaccord
est refusé :

```
erreur[rava.package-mismatch]: le paquet déclaré `formes` ne correspond pas au répertoire `src/geo`
  = déplacez le fichier dans `src/formes/`, ou corrigez la déclaration
```

La déclaration `package` reste facultative : sans elle, le répertoire décide
seul. Elle sert de vérification, comme en Java.

---

## Le paquet est le seul espace de noms

C'est la règle Java, et Rava la conserve : **le fichier n'est pas un espace de
noms**. Deux fichiers du même paquet se voient sans rien importer.

```java
// src/geo/Point.rava
package geo;
public record Point(f64 x, f64 y) { }
```
```java
// src/geo/Forme.rava
package geo;

public interface Forme {
    Point centre();          // pas d'import : même paquet
}
```

Entre paquets, il faut un `import` :

```java
// src/banque/Compte.rava
package banque;

import geo.Point;            // -> use crate::geo::Point;
```

Un `import` dont le premier segment n'est **pas** un paquet du projet passe
tel quel : `import std.collections.HashMap;` reste `use std::collections::HashMap;`,
et il en va de même pour toute crate déclarée en dépendance.

### Ce que cela donne en Rust

```rust
// src/geo/mod.rs
pub mod ...;                          // sous-paquets

#[path = "Point.rs"]
mod __rava_file_Point;
pub use __rava_file_Point::*;         // le fichier est aplati dans le paquet

#[path = "Forme.rs"]
mod __rava_file_Forme;
pub use __rava_file_Forme::*;
```

et chaque fichier généré s'ouvre par `use super::*;` — c'est ainsi que les
voisins du même paquet restent visibles.

---

## `rava.toml`

C'est un manifeste Cargo. `ravac` n'en lit que le nom et l'édition ; **tout le
reste est recopié tel quel** dans le `Cargo.toml` généré. L'écosystème Rust est
donc accessible sans que Rava ait à comprendre chaque option.

```toml
[package]
name = "ma-banque"
version = "0.1.0"

[dependencies]
serde = { version = "1", features = ["derive"] }
```

```java
import serde.Serialize;          // le paquet n'est pas du projet : passe tel quel

@Derive({"Serialize"})
public record Point(f64 x, f64 y) { }
```

Ce que `ravac` ajoute s'il manque : `version`, `edition`, la cible
(`[[bin]]` ou `[lib]`) et une section `[workspace]` vide — pour que le projet
généré ne soit pas happé par un espace de travail voisin. Si vous déclarez
vous-même `[[bin]]` ou `[lib]`, votre déclaration est respectée.

---

## Sortie

Tout est généré dans `target/rava/` :

```
target/rava/
├── .rava-build              (marqueur : le répertoire est régénérable)
├── Cargo.toml
└── src/
    ├── __rava_root.rs       (racine du crate)
    ├── Main.rs
    ├── geo/{mod.rs, Point.rs, Forme.rs}
    └── banque/{mod.rs, Compte.rs}
```

Le répertoire est **entièrement régénéré** à chaque construction. Le marqueur
`.rava-build` évite d'effacer par accident un répertoire qui ne viendrait pas
de `ravac`.

La racine du crate s'appelle `__rava_root.rs` plutôt que `main.rs` : une classe
nommée `Main` produit `Main.rs`, qui est le **même fichier** que `main.rs` sur
un système insensible à la casse — macOS, Windows.

Un projet est un exécutable s'il contient une méthode
`public static void main(String[] args)` ; sinon c'est une bibliothèque.

---

## Commandes

| Commande | Effet |
|---|---|
| `ravac new <nom>` | crée `rava.toml`, `.gitignore` et `src/Main.rava` |
| `ravac build [chemin]` | traduit vers `target/rava/` |
| `ravac check [chemin]` | traduit, puis `cargo build` — diagnostics ramenés sur les `.rava` |
| `ravac run [chemin] [-- args]` | traduit, compile, exécute |

`chemin` vaut `.` par défaut. Un répertoire est traité comme un projet, un
fichier `.rava` comme une unité isolée — `ravac check Point.rava` continue de
fonctionner.

`-o, --out-dir` change le répertoire de sortie.

---

## Diagnostics

Les erreurs de `cargo` et de `rustc` sont ramenées sur le fichier `.rava`
concerné, avec la ligne, la colonne et les positions liées :

```
erreur[E0502]: cannot borrow `self.items` as mutable because it is also borrowed as immutable
  --> src/geo/Boite.rava:7:9
  |
7 |         this.items.push(s);
  |         ^
  = mutable borrow occurs here
  = ligne 6 : immutable borrow occurs here
  = ligne 8 : immutable borrow later used here
```

Dans l'éditeur, le serveur de langage fait de même : il détecte que le fichier
appartient à un projet, vérifie le projet entier à l'enregistrement, et répartit
les diagnostics sur les bons fichiers. Un fichier de projet ne peut pas être
vérifié seul — ses `import` désignent d'autres paquets.

---

## Ce qui n'est pas encore là

- **Un seul crate par projet.** Pas de sous-crates ni d'espace de travail Rava ;
  pour cela, écrivez plusieurs projets et déclarez-les en dépendances de chemin
  dans `rava.toml`.
- **Pas de recompilation incrémentale côté Rava.** Tous les `.rava` sont
  retraduits à chaque construction. C'est instantané à cette échelle, et
  `cargo` fait le reste du travail incrémental.
- **`public` / `private` sur une classe** contrôlent la visibilité Rust
  (`pub` / privé) mais rien n'empêche encore un paquet d'en importer un autre :
  il n'y a pas de notion de paquet « fermé ».
