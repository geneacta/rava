# Rava

**Rust, écrit avec la syntaxe de Java.**

Rava ne réinvente pas un langage : c'est **Rust tel quel**. Les types, la
propriété, l'emprunt, les durées de vie, les traits, `Option`/`Result`, `Send`
et `Sync` — tout est celui de Rust, sans adaptation. Seule la **syntaxe** change :
on écrit des classes, des interfaces, des `switch` et des `@Annotations`.

```java
package exemples;

@Derive({"Debug", "Clone"})
public class Compte {
    private String titulaire;
    private i64 solde;

    public Compte(String titulaire) {
        this.titulaire = titulaire;
        this.solde = 0;
    }

    @Mut
    public Result<Unit, String> retirer(i64 montant) {
        unless (montant <= this.solde) {
            return Err(Macro.format("solde insuffisant : {} < {}", this.solde, montant));
        }
        this.solde -= montant;
        return Ok(());
    }
}
```

```rust
#[derive(Debug, Clone)]
pub struct Compte { titulaire: String, solde: i64 }

impl Compte {
    pub fn new(titulaire: String) -> Self { Self { titulaire, solde: 0 } }

    pub fn retirer(&mut self, montant: i64) -> Result<(), String> {
        if montant > self.solde {
            return Err(format!("solde insuffisant : {} < {}", self.solde, montant));
        }
        self.solde -= montant;
        return Ok(());
    }
}
```

## Comment ça marche

`ravac` traduit la syntaxe, puis **délègue tout le reste à `rustc`**. Il n'y a
ni vérificateur d'emprunt maison, ni inférence maison, ni ramasse-miettes
caché : les erreurs de propriété et de durée de vie sont celles de Rust, dans
les termes de Rust.

```
fichier.rava ──[ravac]──> fichier.rs ──[rustc]──> binaire
             syntaxe                  sémantique
        ▲                                  │
        └────── diagnostics ramenés ───────┘
```

Les erreurs de `rustc` — emprunt, durées de vie, typage — sont reportées sur le
`.rava`, ligne et colonne comprises. On lit du Rust, on le corrige là où on l'a
écrit.

C'est la raison pour laquelle la documentation la plus importante de ce dépôt
est [**ce qui est impossible**](docs/IMPOSSIBLE.md) : tout ce que Rust ne sait
pas faire, Rava ne le fait pas non plus, et le dit clairement.

## Démarrer

```sh
cargo build --release

./target/release/ravac emit  examples/Demo.rava    # affiche le Rust généré
./target/release/ravac check examples/Demo.rava    # vérifie avec rustc
./target/release/ravac run   examples/Demo.rava    # compile et exécute
./target/release/ravac build src/ -o target/rava   # traduit une arborescence
```

## Ce qui est ajouté à Java

Trois choses que Java n'a pas et que Rava reprend, parce qu'elles rendent le
code plus lisible :

```java
unless (x > 0) { … }                   // if !(x > 0)
if (a) { … } else unless (b) { … }     // else if !(b)

age >= 18 implies consentement         // !(age >= 18) || consentement
a nand b     a nor b     a xnor b      // portes logiques
a and b      a or b      a xor b       not a
```

Et tout ce que Rust a sans que Java l'ait — emprunts, durées de vie, `unsafe`,
macros, filtrage par motif — s'écrit avec des annotations qui restent du Java
valide : `@Ref`, `@Mut`, `@Lifetime("a")`, `@Unsafe`, `@Derive`, `Macro.*`.

## Ce qui est masqué

Plusieurs constructions Java sans équivalent direct sont traduites vers **ce
qu'un programmeur Rust écrirait à la main** — même sémantique, même coût :

| Rava | Rust généré |
|---|---|
| `x++` en expression | `{ let t = x; x += 1; t }` |
| `a >>> b` | décalage via le type non signé de même largeur |
| `finalize()` | `impl Drop` — déterministe, lui |
| `super.m(args)` | `Trait::m(self, args)` |
| `Array<T, N>` / `Arr.of(…)` | `[T; N]` / `[a, b, c]` |

Les deux réserves — relecture de la place pour `x++`, et `super.m()` interdit
sur une méthode redéfinie — sont documentées dans
[IMPOSSIBLE.md](docs/IMPOSSIBLE.md#masques).

## Dans votre éditeur

```sh
cargo install --path crates/rava-lsp
```

`rava-lsp` est un serveur de langage : erreurs en direct, survol documenté,
complétion, plan du fichier, corrections rapides — dans VS Code, IntelliJ,
Neovim, Helix, Zed, Sublime et Emacs. Il partage le lexer, le parser et le
générateur de `ravac` : ce que l'éditeur signale est exactement ce que le
compilateur refusera.

À l'enregistrement, il va plus loin : il appelle `rustc` sur le Rust généré et
**ramène ses erreurs sur le `.rava`** — emprunt, durées de vie, typage, avec les
positions liées. Le codegen produit une table de correspondance des lignes, et
la colonne est retrouvée par le nom de l'identifiant.

Pour la coloration seule, sans rien installer : un `.rava` est du Java
syntaxiquement valide, il suffit d'associer l'extension au langage Java.

Le détail par éditeur — et l'extension VS Code — est dans
[editors/README.md](editors/README.md).

## Documentation

| | |
|---|---|
| [**IMPOSSIBLE.md**](docs/IMPOSSIBLE.md) | Ce que Rava ne fera pas, et quoi écrire à la place |
| [**SYNTAX.md**](docs/SYNTAX.md) | La table de correspondance Java ↔ Rust, complète |
| [**ANNOTATIONS.md**](docs/ANNOTATIONS.md) | Référence des annotations |

Site : parcours **« je viens de Java »** et **« je viens de Rust »** sur
<https://geneacta.github.io/rava/>.

## Organisation du dépôt

```
crates/rava-lexer/     tokens Java
crates/rava-parser/    grammaire Java -> AST
crates/rava-codegen/   AST -> source Rust
crates/ravac/          binaire en ligne de commande
crates/rava-check/     traduction + rustc, diagnostics ramenés sur le .rava
crates/rava-json/      JSON minimal (protocole LSP, diagnostics rustc)
crates/rava-lsp/       serveur de langage (LSP) pour les éditeurs
editors/               extension VS Code, grammaire TextMate, réglages par éditeur
examples/              programmes .rava, vérifiés par rustc dans les tests
docs/                  documentation et site GitHub Pages
```

```sh
cargo test        # unitaires + compilation de tous les exemples par rustc
```

## Statut

Prototype fonctionnel. Le langage couvre classes, records, interfaces (traits
avec types et constantes associés, méthodes par défaut), enums à charge utile,
génériques bornés, durées de vie, emprunts, filtrage par motif avec gardes et
déconstruction, fermetures, `unsafe`, macros et attributs.

Licence Apache 2.0.
