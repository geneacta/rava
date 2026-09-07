# Documentation de Rava

**Rava, c'est Rust écrit avec la syntaxe de Java.** Les types, la propriété,
l'emprunt, les durées de vie et les traits sont exactement ceux de Rust ; seule
la forme change.

Site : <https://geneacta.github.io/rava/> — avec un parcours « je viens de
Java » et un parcours « je viens de Rust ».

---

## Pour écrire du Rava

| | |
|---|---|
| [**IMPOSSIBLE.md**](IMPOSSIBLE.md) | Ce que Rava ne fera pas, ce qui est masqué, et quoi écrire à la place. **Le document central.** |
| [**SYNTAX.md**](SYNTAX.md) | La table de correspondance Java ↔ Rust, complète |
| [**ANNOTATIONS.md**](ANNOTATIONS.md) | Emprunts, durées de vie, attributs : la référence des annotations |
| [**PROJETS.md**](PROJETS.md) | Paquets, `rava.toml`, arborescence générée, dépendances Cargo |
| [**GRAMMAIRE.md**](GRAMMAIRE.md) | La grammaire complète, adossée à un test par production |

## Pour outiller

| | |
|---|---|
| [**../editors/README.md**](../editors/README.md) | Serveur de langage et coloration : VS Code, IntelliJ, Neovim, Helix, Zed, Sublime, Emacs |

## Pour travailler sur le compilateur

| | |
|---|---|
| [**ARCHITECTURE.md**](ARCHITECTURE.md) | Comment le compilateur est bâti, et pourquoi ainsi |
| [**../CONTRIBUTING.md**](../CONTRIBUTING.md) | Construire, tester, ajouter une construction, refuser une construction |
| [**../CHANGELOG.md**](../CHANGELOG.md) | L'histoire du projet |

---

## Par où commencer

**Vous venez de Java.** Lisez [IMPOSSIBLE.md](IMPOSSIBLE.md) en premier : la
liste de ce qui disparaît — `null`, les exceptions, l'héritage — dit l'essentiel
de ce qui change. Puis installez l'outillage et lisez le Rust généré par
`ravac emit` sur votre propre code : c'est le meilleur cours de Rust disponible.

**Vous venez de Rust.** [SYNTAX.md](SYNTAX.md) est un dictionnaire de formes ;
la sémantique, vous la connaissez déjà. Le seul point à retenir est que les
identifiants ne sont pas renommés en `snake_case`.

**Vous voulez modifier le compilateur.** [ARCHITECTURE.md](ARCHITECTURE.md),
puis [CONTRIBUTING.md](../CONTRIBUTING.md).
