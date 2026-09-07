# Architecture

Rava traduit une syntaxe et ne fait **rien d'autre**. Pas d'inférence de types,
pas de vérificateur d'emprunt, pas d'analyse de durées de vie : ces trois-là
existent déjà dans `rustc`, et les réécrire serait exactement le moyen de
diverger de Rust.

Ce document explique comment le dépôt est organisé, et surtout **pourquoi**
certaines décisions sont ce qu'elles sont. Le « quoi » se lit dans le code ; le
« pourquoi » ne s'y lit pas.

---

## Le chemin d'un fichier

```
  .rava
    │
    ├─ rava-lexer ──── tokens Java
    │
    ├─ rava-parser ─── AST de forme Java              (rava-ast)
    │
    ├─ rava-codegen ── source Rust + table de lignes
    │
    ├─ rava-build ──── arborescence de modules + Cargo.toml   (projet)
    │
    └─ rava-check ──── rustc / cargo, diagnostics ramenés sur le .rava
                          │
                          ├─ ravac      (ligne de commande)
                          └─ rava-lsp   (éditeurs)
```

Chaque crate ne dépend que des précédents. `rava-json` est à part : un JSON
minimal, partagé par le protocole LSP et la lecture des diagnostics de `rustc`.

**Aucune dépendance externe.** Le compilateur et ses outils se lisent en entier,
`cargo install` est instantané, et il n'y a rien à auditer d'autre que ce
dépôt. Le seul coût est un parser JSON de 300 lignes, écrit une fois.

---

## rava-lexer

Un lexeur Java classique : identifiants, littéraux, opérateurs au plus long
d'abord, commentaires.

**Les colonnes comptent des caractères, pas des octets.** C'est une correction
apportée après coup, et elle compte : une ligne contenant un accent décalait le
curseur des diagnostics, et surtout les positions envoyées aux éditeurs. Le
lexeur n'incrémente donc pas la colonne sur les octets de continuation UTF-8.

**La documentation est attachée au token suivant.** `///` et `/** */` sont
mémorisés puis rattachés à la prochaine déclaration, où le parser les
transforme en annotation `@Doc`. Le codegen n'a alors qu'un seul chemin pour
la documentation, qu'elle vienne d'un commentaire ou d'une annotation.

---

## rava-parser

Descente récursive, avec **retour arrière**. La grammaire Java l'exige :
`(Type) x` et `(expr)` commencent pareil, `Type ident` peut ouvrir une
déclaration ou une expression, `(a, b) ->` ressemble à une expression
parenthésée jusqu'à la flèche.

`attempt()` sauvegarde la position, exécute, et rembobine en cas d'échec.

**Le piège du retour arrière, et sa correction.** Quand une tentative échoue,
son erreur est jetée et l'on rapporte celle du repli — presque toujours moins
utile. Sur `var x = null;`, le parser désignait `x` (« `;` attendu ») au lieu de
`null`. Le parser retient donc **l'erreur la plus avancée** rencontrée, tentatives
abandonnées comprises, et la rapporte si elle va plus loin que celle qui remonte.
À position égale, une erreur porteuse d'une note l'emporte sur un simple échec
de forme.

**Trois ambiguïtés notables :**

| Ambiguïté | Résolution |
|---|---|
| `Map<String, Vec<i32>>` | `expect_gt()` scinde le token `>>` en deux |
| `case P when x == y -> …` | un compteur `in_guard` désactive les lambdas dans une garde ; sans lui, `y ->` en ouvrirait une |
| `f()?` contre `a ? b : c` | la syntaxe Java l'emporte : `?` n'est l'opérateur Rust que là où aucune branche de ternaire ne peut commencer |

**Le désucrage a lieu ici, pas dans le codegen.** `unless (c)` devient
`if (!c)` — avec inversion de l'opérateur de comparaison plutôt qu'un `!`
empilé — et `a nand b` devient `!(a && b)`, à l'arbre. Le codegen ne connaît
donc ni `unless` ni les opérateurs littéraux : une seule représentation à
traduire.

Une conséquence à surveiller : la précédence des mots (`and` < `xor`) n'est pas
celle des symboles Rust (`&&` < `^`). Le désucrage parenthèse tout opérande
binaire d'un opérateur différent, pour que le sens écrit reste le sens compilé.

---

## rava-codegen

L'AST devient du texte Rust. C'est de la traduction de formes, sans analyse.

### La table de correspondance des lignes

C'est ce qui permet de ramener une erreur de `rustc` sur le `.rava`. Un simple
compteur de lignes ne suffit **pas** : le codegen assemble certains blocs en
déplaçant du texte — les bras de `match`, les corps de fermeture sont générés
dans le tampon principal, puis découpés (`split_off`) et réinsérés ailleurs.

Chaque ligne porte donc, pendant la génération, un marqueur `//~rava:<ligne>`
en fin de ligne. Le marqueur **voyage avec sa ligne**, quel que soit le
déplacement. Il est retiré en fin de course, et la table est construite à ce
moment-là.

Un `//~rava:` écrit par l'utilisateur dans une chaîne n'est pas confondu : seul
un suffixe entièrement numérique est reconnu comme marqueur.

### Décisions de traduction

**Les identifiants ne sont pas renommés.** `camelCase` reste `camelCase`, et le
fichier généré porte `#![allow(non_snake_case)]`. C'est délibéré : renommer
empêcherait d'implémenter un vrai trait Rust — `fmt`, `next`, `drop` — dont les
noms sont imposés.

**Le constructeur est reconstruit, pas traduit.** Les affectations
`this.champ = …;` sont collectées et deviennent le littéral de structure final ;
les autres instructions sont émises avant, dans l'ordre. Un champ non affecté et
sans valeur par défaut produit un diagnostic — plutôt qu'un `Self { }` incomplet
que `rustc` refuserait avec un message moins clair.

**`@Override` répartit les méthodes.** Le bloc `impl` inhérent reçoit tout ce qui
n'est pas annoté ; chaque `impl Trait for` reçoit ses méthodes. Avec plusieurs
interfaces, `@Override(Display.class)` tranche. `finalize()` est routé vers
`impl Drop`.

**Les masques** — `x++` en expression, `>>>`, `super.m()`, `finalize()`,
`Array<T, N>` — sont des traductions vers ce qu'un programmeur Rust écrirait à
la main. Le masque de `>>>` est un trait `UShr` inséré dans le fichier, et
**seulement s'il sert**. Voir [IMPOSSIBLE.md](IMPOSSIBLE.md#masques) pour les
deux réserves documentées.

---

## rava-build

Le modèle de paquets, et l'écriture du projet Cargo.

**Le paquet est le seul espace de noms.** C'est la règle Java, et elle dicte la
projection : tous les fichiers d'un paquet atterrissent dans le **même module**
Rust, via `#[path] mod __rava_file_X;` suivi de `pub use __rava_file_X::*;`.
Chaque fichier généré s'ouvre par `use super::*;`, ce qui rend les voisins du
paquet visibles sans import — exactement Java.

**Le répertoire fait foi**, la déclaration `package` confirme. Un désaccord est
refusé plutôt que silencieusement arbitré.

**La racine du crate s'appelle `__rava_root.rs`**, déclarée explicitement dans
le `Cargo.toml`. Elle ne peut pas s'appeler `main.rs` : une classe `Main`
produit `Main.rs`, qui est le *même fichier* sur macOS et Windows. Le premier
essai s'écrasait lui-même.

**`rava.toml` est un manifeste Cargo.** Seuls le nom et l'édition sont lus ;
tout le reste est recopié tel quel. C'est ce qui rend l'écosystème Rust
accessible sans que Rava ait à comprendre `features`, `patch`, `profile` ou
quoi que ce soit d'autre. `ravac` n'ajoute que ce qui manque, et un
`[workspace]` vide pour que le projet généré ne soit pas happé par un espace de
travail voisin.

Le répertoire de sortie est **entièrement régénéré**, avec un marqueur
`.rava-build` qui empêche d'effacer par accident un répertoire étranger.

---

## rava-check

Le pont vers `rustc` et `cargo`, dans les deux sens.

Les diagnostics sont lus en JSON (`--error-format=json`,
`--message-format=json`) puis reconvertis :

- **la ligne** vient de la table de correspondance ;
- **la colonne** n'a pas de table : on cherche dans la ligne `.rava` le mot que
  `rustc` désigne dans le Rust. Les identifiants traversent la traduction tels
  quels, donc cela tombe juste ; à défaut, on pointe le premier caractère non
  blanc, plutôt que de mentir ;
- **les positions liées** — l'emprunt initial, l'usage suivant — sont ramenées
  aussi. Sur une erreur d'emprunt, c'est l'essentiel du message.

Un avertissement dont l'origine est inconnue (le prélude, un masque inséré) est
**écarté** plutôt que rapporté sur la ligne 1. Une erreur, non : mieux vaut une
position approximative qu'une erreur invisible.

---

## ravac et rava-lsp

Deux façades sur les mêmes crates. Un seul rendu de diagnostic, une seule
définition de ce qui est refusé.

`ravac` déduit le mode du chemin : un `.rava` est une unité isolée, un
répertoire est un projet. Pas de drapeau à retenir.

`rava-lsp` répartit l'effort selon le moment :

| Moment | Ce qui tourne |
|---|---|
| chaque frappe | lexeur, parser, codegen — en mémoire, instantané |
| ouverture, enregistrement | plus `rustc`, ou `cargo` si le fichier appartient à un projet |

C'est la répartition de rust-analyzer, pour la même raison. Un fichier de projet
ne peut pas être vérifié seul : ses `import` désignent d'autres paquets, et on
signalerait des imports non résolus qui n'existent pas. Le serveur remonte donc
au `rava.toml`, vérifie le projet entier et **publie par fichier**, y compris
vide — c'est ainsi que les marqueurs d'une correction précédente disparaissent.

Le plan du fichier (l'esquisse) est **conservé** quand le source ne se lit plus.
Pendant la frappe, un fichier passe par des états invalides, et l'esquisse ne
doit pas clignoter.

---

## Tests

| Où | Ce qui est vérifié |
|---|---|
| `rava-lexer` | tokens, colonnes en caractères, documentation attachée |
| `rava-parser` | formes de l'arbre, refus documentés, précédence des opérateurs |
| `rava-parser/tests/grammaire.rs` | **une production de [GRAMMAIRE.md](GRAMMAIRE.md) par test** |
| `rava-codegen` | masques, table de lignes, indépendance à l'indentation |
| `rava-check` | erreurs d'emprunt et de type ramenées à la bonne ligne, la bonne colonne |
| `rava-lsp` | diagnostics, survol, complétion, plan, corrections |
| `ravac/tests/examples.rs` | **chaque exemple du dépôt est compilé par `rustc`** |
| `ravac/tests/projet.rs` | paquets, imports entre paquets, diagnostics multi-fichiers |

Les deux derniers sont les plus importants : ils empêchent la documentation et
les exemples de dériver du compilateur.

---

## Ce qui n'existe pas, et n'existera pas

Pas de vérificateur d'emprunt, pas d'inférence, pas de résolution de types côté
Rava. Chaque fois que la tentation revient — pour un meilleur message, pour une
complétion plus fine — c'est la même réponse : ce serait une deuxième
implémentation de Rust, qui finirait par ne plus dire la même chose que la
première.
