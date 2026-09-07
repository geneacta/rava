# Contribuer à Rava

## Construire et vérifier

```sh
cargo build             # les six crates
cargo test              # unitaires, grammaire, exemples compilés par rustc
cargo clippy --all-targets
```

`cargo test` compile **tous les exemples du dépôt avec `rustc`**. C'est le
garde-fou principal : il empêche le compilateur de produire du Rust invalide
sans qu'on s'en aperçoive.

Installer les outils localement :

```sh
cargo install --path crates/ravac
cargo install --path crates/rava-lsp
```

L'organisation du dépôt et les décisions de conception sont dans
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) — à lire avant de toucher au
compilateur.

---

## Ajouter une construction au langage

Une nouvelle forme se propage à travers six endroits. Les oublier ne casse rien
tout de suite : c'est plus tard que la documentation ment ou que l'éditeur se
tait.

1. **`crates/rava-ast`** — le nœud, si la forme n'entre dans aucun existant.
   Préférez réutiliser : `unless` et les opérateurs littéraux n'ont ajouté aucun
   nœud, ils sont désucrés à l'analyse.
2. **`crates/rava-parser`** — la lecture. Si la forme est ambiguë avec une autre,
   `attempt()` et un rembobinage explicite ; jamais une devinette silencieuse.
3. **`crates/rava-codegen`** — la traduction. Si le Rust produit dépend d'un
   contexte, passez-le par `Context`, pas par une variable globale.
4. **`docs/GRAMMAIRE.md`** — la production.
5. **`crates/rava-parser/tests/grammaire.rs`** — le test de cette production.
   C'est lui qui empêche le document de dériver.
6. **`crates/rava-lsp/src/kb.rs`** — l'entrée du dictionnaire : ce que la forme
   devient en Rust, et son texte de survol. Sans cela, l'éditeur reste muet
   dessus.

Et selon la nature de l'ajout : `docs/SYNTAX.md` (table de correspondance),
`docs/ANNOTATIONS.md` (nouvelle annotation), `editors/vscode/syntaxes/rava.tmLanguage.json`
(coloration), `docs/index.html` (les deux parcours du site).

---

## Refuser une construction

Un refus est une fonctionnalité, pas un échec. Il doit :

- **expliquer**, pas constater. « `null` n'existe pas en Rust » puis, en note,
  quoi écrire à la place ;
- **renvoyer** à la section de [docs/IMPOSSIBLE.md](docs/IMPOSSIBLE.md) qui
  détaille le pourquoi ;
- **porter un code** (`err_fix` plutôt que `err_note`) si la correction est
  mécanique — c'est ce qui donne la correction rapide dans l'éditeur ;
- **ne jamais être silencieux.** `synchronized` était accepté sans effet : on
  croyait protéger quelque chose. C'est pire qu'un refus.

Ajoutez le refus à la table de [docs/IMPOSSIBLE.md](docs/IMPOSSIBLE.md) et une
entrée `Kind::Refused` dans `kb.rs`, pour que le survol l'explique aussi.

---

## Masquer plutôt que refuser

Certaines formes Java n'ont pas d'équivalent **direct** en Rust, mais un
équivalent **exact** : ce qu'un programmeur Rust écrirait à la main. `x++` en
expression, `>>>`, `finalize()`. Un masque est légitime si — et seulement si :

- la sémantique est **identique**, pas approchée ;
- le coût est le même ;
- toute divergence résiduelle est **écrite** dans
  [IMPOSSIBLE.md](docs/IMPOSSIBLE.md#masques). `x++` relit la place deux fois :
  c'est documenté, pas caché.

Si la divergence ne peut pas être documentée en une phrase claire, ce n'est pas
un masque : c'est un piège. Refusez.

---

## Ce que Rava ne fera pas

Le principe tient en une ligne : **Rava change la syntaxe, jamais les règles.**

Il n'y aura donc ni vérificateur d'emprunt, ni inférence de types, ni
résolution de traits côté Rava. La tentation revient régulièrement — un message
plus précis, une complétion plus fine — et la réponse est toujours la même : ce
serait une deuxième implémentation de Rust, qui finirait par ne plus dire la
même chose que la première.

Toute proposition qui rendrait un programme Rava valide alors que le Rust
correspondant ne l'est pas, ou l'inverse, est hors sujet.

---

## Style

Le code du compilateur est en Rust, la documentation et les messages en
français.

- **Les commentaires disent pourquoi.** Le code dit déjà ce qu'il fait. Un
  commentaire qui paraphrase la ligne suivante est du bruit ; un commentaire
  qui explique l'ambiguïté qu'on vient de trancher vaut dix lignes de code.
- **Les noms de test décrivent la propriété**, pas la fonction appelée :
  `lindentation_ne_change_rien`, pas `test_generate`.
- **Aucune dépendance externe.** C'est une contrainte assumée : le dépôt se lit
  en entier. Si une dépendance semble indispensable, ouvrez d'abord une
  discussion — jusqu'ici, un parser JSON de 300 lignes a suffi.
- `cargo test` et `cargo build --release` doivent passer **sans avertissement**.

---

## Signaler un problème

Le plus utile est un `.rava` minimal, ce que vous attendiez, et ce que `ravac`
a produit :

```sh
ravac emit  probleme.rava     # le Rust généré
ravac check probleme.rava     # les diagnostics
```

Si `ravac` produit du Rust que `rustc` refuse, c'est un bug de Rava. Si `rustc`
refuse pour une raison de Rust — emprunt, durée de vie, typage — ce n'en est
pas un : c'est le langage qui parle, et Rava se contente de vous le montrer à
l'endroit où vous avez écrit.
