# Journal des modifications

Rava n'a pas encore été publié. Cette version rassemble tout ce qui a été
construit depuis le dépôt vide.

## 0.1.0 — non publiée

### Le langage

Rust tel quel, avec la syntaxe de Java. `ravac` traduit la forme et délègue le
reste à `rustc` : ni vérificateur d'emprunt, ni inférence, ni ramasse-miettes
maison. La sémantique est celle de Rust, sans adaptation.

**Couvert** — classes (`struct` + `impl`), records, interfaces (traits, méthodes
par défaut, types et constantes associés), enums à charge utile, génériques
bornés et constants, durées de vie, emprunts, filtrage par motif avec gardes,
alternatives, intervalles et déconstruction, fermetures, `unsafe`, pointeurs
bruts, `async`, macros et attributs.

**Ajouté à Java** — `unless` et `else unless`, avec inversion de l'opérateur de
comparaison plutôt qu'un `!` empilé ; et huit opérateurs logiques en toutes
lettres : `and`, `or`, `xor`, `nand`, `nor`, `xnor`, `implies`, `not`, de
précédence logique classique, `implies` associatif à droite.

**Refusé, avec un diagnostic qui explique** — `null`, les exceptions,
l'héritage de classe, `instanceof`, la surcharge, les jokers génériques, les
varargs, `synchronized`. Chaque refus renvoie à la section correspondante de
`docs/IMPOSSIBLE.md`.

**Masqué** — traduit vers ce qu'un programmeur Rust écrirait à la main :
`x++` en expression, `>>>`, `finalize()` (vers `Drop`), `super.m()`,
`Array<T, N>`, `Arr.of` / `Arr.fill`, `Vec::<T>::new()`. Les deux réserves —
la place relue deux fois par `x++`, et `super.m()` interdit sur une méthode
redéfinie — sont documentées plutôt que cachées.

### Projets

Paquets à la manière de Java : le répertoire fait foi, et le paquet est le seul
espace de noms — deux fichiers du même paquet se voient sans import. `rava.toml`
*est* un manifeste Cargo, dont seuls le nom et l'édition sont lus : tout le
reste, dépendances comprises, est recopié tel quel, ce qui laisse l'écosystème
Rust accessible.

`ravac new`, puis `build` / `check` / `run` sur un projet ou un fichier isolé,
le mode étant déduit du chemin.

### Diagnostics

Les erreurs de `rustc` et de `cargo` — emprunt, durées de vie, typage — sont
**ramenées sur le `.rava`** : ligne, colonne et positions liées. Le codegen
produit une table de correspondance des lignes ; la colonne est retrouvée par le
nom de l'identifiant, les identifiants traversant la traduction tels quels.

### Outillage éditeur

`rava-lsp`, serveur de langage sans dépendance : diagnostics, survol documenté,
complétion, plan du fichier, corrections rapides. Syntaxe et traduction à chaque
frappe, `rustc` ou `cargo` à l'enregistrement. Dans un projet, le serveur
vérifie l'ensemble et publie par fichier.

Grammaire TextMate et extension VS Code ; réglages fournis pour IntelliJ (via
LSP4IJ), Neovim, Helix, Zed, Sublime et Emacs.

### Documentation

`IMPOSSIBLE.md`, `SYNTAX.md`, `ANNOTATIONS.md`, `PROJETS.md`, `GRAMMAIRE.md`,
`ARCHITECTURE.md`, `CONTRIBUTING.md`, et un site GitHub Pages avec un parcours
« je viens de Java » et un parcours « je viens de Rust ».

La grammaire est adossée à un test par production : elle ne peut pas dériver de
l'implémentation sans que la suite de tests le signale.

### Corrections notables

- **Colonnes en octets.** Le lexeur comptait les octets : une ligne accentuée
  décalait le curseur des diagnostics et les positions envoyées aux éditeurs.
  Les colonnes comptent désormais des caractères.
- **Retour arrière et diagnostics.** Le parser jetait l'erreur de la tentative
  abandonnée et rapportait celle du repli. Sur `var x = null;`, il désignait
  `x` au lieu de `null`. L'erreur la plus avancée est maintenant retenue.
- **`src/main.rs` et les systèmes insensibles à la casse.** Une classe `Main`
  produit `Main.rs`, qui est le *même fichier* que `main.rs` sur macOS et
  Windows : la racine du crate s'écrasait elle-même. Elle est désormais nommée
  explicitement.
- **`synchronized` accepté sans effet.** On croyait protéger quelque chose ; le
  mot-clé est maintenant refusé.
- **`Ok(())` illisible.** Java n'a pas de notation pour la valeur unité. `()`
  est accepté, avec `Unit.of()` en forme Java stricte.
- **`super.m()` récursif.** Le premier masque compilait et débordait la pile :
  `Trait::m(self)` repasse par la table de dispatch. Le cas est détecté et
  refusé.
- **Le plan du fichier clignotait** dans l'éditeur à chaque frappe invalide ; le
  dernier plan valide est conservé.
- **Multi-déclarateurs.** `var a = 1, b = 2;` et
  `for (var i = 0, j = n; …)` ne se lisaient pas.
