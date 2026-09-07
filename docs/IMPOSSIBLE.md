# Ce qui est impossible en Rava

Rava, c'est **Rust, écrit avec la syntaxe de Java**. La sémantique n'est pas
adaptée, pas approchée, pas réinventée : `ravac` traduit la syntaxe et laisse
`rustc` faire le typage, l'emprunt et les durées de vie.

Conséquence directe : **tout ce que Rust ne sait pas faire, Rava ne le fait pas
non plus.** Ce document liste ces trous, et ce qu'on écrit à la place.

Le principe inverse est vrai aussi, et il est important : **tout ce que Rust
sait faire reste accessible en Rava**, y compris ce que Java n'a pas. Ces
mécanismes sont documentés dans [SYNTAX.md](SYNTAX.md) et
[ANNOTATIONS.md](ANNOTATIONS.md) — ils ne sont pas impossibles, juste écrits
autrement.

---

## Sommaire

| Construction Java | Statut | Traduction ou remplacement |
|---|---|---|
| [`null`](#null) | ❌ impossible | `Option<T>` |
| [Exceptions (`try`/`catch`/`throw`/`throws`)](#exceptions) | ❌ impossible | `Result<T, E>` + `.q()` |
| [Héritage de classe (`extends`)](#heritage) | ❌ impossible | composition + interfaces (traits) |
| [`super.m()` sur une méthode redéfinie](#heritage) | ❌ impossible | extraire la partie commune |
| [`instanceof` / downcast](#instanceof) | ❌ impossible | `switch` sur enum, ou `dyn Any` |
| [Surcharge de méthode](#surcharge) | ❌ impossible | noms distincts, ou `@Named` |
| [Jokers génériques `<?>`](#wildcards) | ❌ impossible | paramètre borné, `@Impl`, `@Dyn` |
| [Varargs `int...`](#varargs) | ❌ impossible | slice `@Ref int[]` |
| [Classes anonymes / internes non statiques](#anonymes) | ❌ impossible | lambda, ou type nommé |
| [Ramasse-miettes, cycles d'objets](#gc) | ❌ impossible | `Rc`/`Arc` + `Weak` |
| [Réflexion, chargement dynamique](#reflexion) | ❌ impossible | traits, `enum`, macros |
| [`synchronized`](#concurrence) | ❌ impossible | `Mutex<T>`, `RwLock<T>` |
| [Généricité effacée / covariance de tableau](#variance) | ❌ impossible | monomorphisation Rust |
| [`static` mutable partagé](#statics) | ⚠️ contraint | `const`, `OnceLock`, `Mutex` |
| [`>>>`](#masques) | ✅ **masqué** | décalage via le non signé de même largeur |
| [`x++` en expression](#masques) | ✅ **masqué** | bloc-expression |
| [`finalize()`](#masques) | ✅ **masqué** | `impl Drop` — déterministe, lui |
| [`super.m()` non redéfinie](#masques) | ✅ **masqué** | `Trait::m(self, …)` |

---

<a id="masques"></a>
## D'abord : ce qui est *masqué*

Certaines constructions Java n'ont pas d'équivalent **direct** en Rust, mais ont
un équivalent **exact** : ce qu'un programmeur Rust écrirait à la main. Rava les
traduit vers cette forme. Ce ne sont pas des approximations — la sémantique est
identique, et le coût aussi.

| Rava | Rust généré |
|---|---|
| `x++` / `x--` en expression | `{ let t = x; x += 1; t }` |
| `++x` / `--x` en expression | `{ x += 1; x }` |
| `a >>> b` | `__rava::UShr::ushr(a, b as u32)` |
| `finalize()` | `impl Drop for T { fn drop(&mut self) }` |
| `super.m(args)` | `Trait::m(self, args)` |
| `Array<T, N>` | `[T; N]` |
| `Arr.of(a, b, c)` / `Arr.fill(v, n)` | `[a, b, c]` / `[v; n]` |
| `Vec::<T>::new()` | `Vec::<T>::new()` |

Deux précisions, parce qu'un masque qui ment est pire qu'une erreur :

**`>>>`.** Java n'a que des entiers signés, d'où l'opérateur. Rust a les deux
familles, mais on ne connaît pas toujours le type à la traduction : Rava insère
donc dans le fichier généré un trait `UShr` implémenté pour chaque largeur, qui
passe par le type non signé correspondant. Le masque n'est émis que si `>>>`
sert réellement, et il se compile en une seule instruction machine.

**`finalize()`.** La méthode Java est appelée à un moment indéterminé, ou
jamais. `Drop` s'exécute à la sortie de portée, toujours. Le masque vous donne
donc *mieux* que ce que vous écriviez — mais ne comptez pas sur le même
calendrier.

**`x++` en expression** relit la place deux fois (`{ let t = x; x += 1; t }`).
Si l'expression de place a un effet de bord — `a[f()]++` — cet effet a lieu deux
fois. Java ne le fait qu'une fois. Écrivez l'incrémentation à part dans ce cas.

**`super.m()`** ne fonctionne que sur une méthode que la classe **ne redéfinit
pas**. Sur une méthode redéfinie, `Trait::m(self)` repasse par la table de
dispatch, donc par votre propre implémentation : c'est une récursion infinie.
Rust n'offre aucun moyen d'atteindre un corps par défaut que l'on remplace, et
Rava refuse le cas explicitement plutôt que de produire une boucle.

---

<a id="null"></a>
## `null` n'existe pas

Rust n'a pas de valeur nulle. Il n'y a donc rien à traduire, et `ravac` refuse
le littéral.

```java
String s = null;          // ❌ erreur de compilation Rava
```

```java
Option<String> s = None;                 // ✅
Option<String> s = Some("bonjour".to_string());

switch (s) {
    case Some(var v) -> System.out.println("{}", v);
    case None -> System.out.println("rien");
}
```

L'absence de `null` est l'une des raisons d'être de Rust. La rétablir
reviendrait à changer la sémantique — ce que Rava ne fait jamais.

---

<a id="exceptions"></a>
## Pas d'exceptions : ni `try`, ni `catch`, ni `throw`, ni `throws`

Rust ne propage pas d'exception. Les quatre mots-clés sont refusés par le
parser, avec un renvoi vers cette page.

```java
public int parse(String s) throws NumberFormatException {   // ❌
    try { ... } catch (Exception e) { ... }                 // ❌
}
```

```java
public static Result<i32, String> parse(@Ref str s) {       // ✅
    var n = s.parse::<i32>().map_err(e -> Macro.format("{:?}", e)).q();
    return Ok(n);
}
```

`.q()` est l'opérateur `?` de Rust en syntaxe Java stricte ; `expr?` fonctionne
aussi si vous acceptez de sortir de la syntaxe Java (voir
[SYNTAX.md](SYNTAX.md#extensions)).

Le nettoyage en `finally` se fait par `Drop` : implémentez l'interface `Drop`
sur le type concerné.

Un `panic!` (`Macro.panic("...")`) existe, mais ce n'est pas une exception : il
n'est pas rattrapable au fil du code, il termine le fil d'exécution.

---

<a id="heritage"></a>
## Pas d'héritage de classe : ni `extends` sur une classe, ni `super`

Rust n'a pas de sous-typage nominal entre structures. `class A extends B` est
refusé, et `super` aussi.

```java
public class Chien extends Animal { }        // ❌
```

Deux remplacements, selon l'intention :

**Partager du comportement** → une interface (un trait) avec des méthodes par
défaut :

```java
public interface Animal {
    String nom();
    default String crier() { return Macro.format("{} fait du bruit", this.nom()); }
}

public class Chien implements Animal {
    private String nom;
    public Chien(String nom) { this.nom = nom; }
    @Override public String nom() { return this.nom.clone(); }
    @Override public String crier() { return Macro.format("{} aboie", this.nom); }
}
```

**Partager de l'état** → la composition :

```java
public class Chien {
    private Animal base;      // le « parent » devient un champ
    private bool dresse;
}
```

`super.m(args)` est traduit en `Trait::m(self, args)` — mais **seulement pour
une méthode que la classe ne redéfinit pas**. Sur la méthode que vous êtes en
train de redéfinir, cet appel repasserait par votre propre implémentation :
Rust n'a aucun moyen d'atteindre un corps par défaut remplacé, et Rava refuse
le cas plutôt que de produire une récursion infinie.

```java
public interface Animal {
    default String politesse() { return "salut"; }
    default String crier() { return Macro.format("{} !", this.politesse()); }
}

public class Chien implements Animal {
    @Override public String crier() {
        return Macro.format("{} wouf", super.politesse());   // ✅ non redéfinie
        // return super.crier();                             // ❌ récursion
    }
}
```

Un `class ... implements I` reste parfaitement valide : c'est un `impl I for
Struct`, pas de l'héritage.

---

<a id="instanceof"></a>
## Pas d'`instanceof`, pas de transtypage descendant

Il n'y a pas de hiérarchie de types à interroger à l'exécution.

```java
if (s instanceof Circle c) { ... }         // ❌
```

Si l'ensemble des cas est connu, c'est une enum, et le `switch` la
déconstruit :

```java
public enum AnyShape { Circle(Circle inner), Rect(Rect inner) }

switch (shape) {
    case AnyShape.Circle(var c) -> c.rayon();
    case AnyShape.Rect(var r)   -> r.largeur();
}
```

Si l'ensemble est ouvert, passez par `dyn Any` et `downcast_ref`, exactement
comme en Rust — c'est verbeux des deux côtés, et c'est voulu.

---

<a id="surcharge"></a>
## Pas de surcharge de méthode

Rust n'a pas de résolution par signature : deux méthodes ne peuvent pas
partager un nom dans un même `impl`.

```java
public void add(int x) { }
public void add(String x) { }              // ❌ conflit à la génération
```

Trois issues :

```java
public void addInt(int x) { }
public void addStr(String x) { }

// ou bien un nom Rust distinct, en gardant le nom Java :
@Named("add_str") public void addStr(String x) { }

// ou bien la généricité, quand les cas partagent un comportement :
public <T extends Into<String>> void add(T x) { }
```

Même règle pour les constructeurs : le premier devient `new`, les suivants
**doivent** porter `@Named` :

```java
public Buffer(usize n) { ... }                       // -> Buffer::new(n)
@Named("with_capacity")
public Buffer(usize n, usize cap) { ... }            // -> Buffer::with_capacity(n, cap)
```

---

<a id="wildcards"></a>
## Pas de jokers génériques `<?>`

`List<? extends Number>` n'a pas de sens en Rust : il n'y a ni effacement de
type ni variance de ce genre.

```java
void f(List<? extends Shape> l) { }        // ❌
```

```java
<T extends Shape> void f(@Ref T[] l) { }       // ✅ générique borné (monomorphisé)
void f(@Ref Vec<Box<@Dyn Shape>> l) { }        // ✅ polymorphisme dynamique
void f(@Impl Iterator<Item = i32> it) { }      // ✅ `impl Trait` en position d'argument
```

---

<a id="varargs"></a>
## Pas de varargs

```java
public void log(String... parts) { }       // ❌
```

```java
public void log(@Ref String[] parts) { }   // ✅ -> fn log(&self, parts: &[String])
```

Les macros variadiques existent en revanche : `Macro.println("{} {}", a, b)`
devient `println!("{} {}", a, b)`.

---

<a id="ushr"></a>
## `>>>` : masqué, pas impossible

Java a besoin de `>>>` parce que tous ses entiers sont signés. Rust a des types
non signés, où `>>` est déjà un décalage logique.

Rava accepte quand même `>>>` : voir [les masques](#masques). La forme
idiomatique reste préférable quand vous maîtrisez le type :

```java
u32 h = x >> 3;                            // ✅ direct, rien à masquer
i32 h = x >>> 3;                           // ✅ masqué, équivalent exact
```

---

<a id="incrementation"></a>
## `x++` : masqué, avec une réserve

Rust n'a pas d'opérateur d'incrémentation. Rava traduit `x++;` en `x += 1;` en
position d'instruction, et en bloc-expression ailleurs :

```java
i++;                                       // -> i += 1;
var avant = i++;                           // -> { let t = i; i += 1; t }
var apres = ++i;                           // -> { i += 1; i }
tampon[i++] = 10;                          // ✅
```

La réserve : la place est relue deux fois. `a[f()]++` appellerait `f()` deux
fois là où Java ne l'appelle qu'une. Sortez l'indice dans une variable si
l'expression a un effet de bord.

Le `for` classique reste correct même avec `continue` : `ravac` place le pas
d'itération en tête de boucle pour que `continue` l'exécute, comme en Java.

---

<a id="anonymes"></a>
## Pas de classe anonyme ni de classe interne non statique

```java
Runnable r = new Runnable() { public void run() { } };   // ❌
```

Une lambda couvre le cas courant (Rust : une fermeture) :

```java
Fn0<Unit> r = () -> { System.out.println("hop"); };
```

Sinon, déclarez un type nommé. Les types imbriqués **statiques** sont acceptés
(`class A { static class B { } }`) : ils deviennent des items Rust au même
niveau. Un type interne non statique supposerait une référence implicite à
l'instance englobante — Rust ne l'a pas.

---

<a id="gc"></a>
## Pas de ramasse-miettes : les graphes cycliques ne se font pas tout seuls

Un graphe d'objets qui se référencent mutuellement est trivial en Java et
impossible tel quel en Rust : la propriété est un arbre.

```java
public class Noeud { Noeud parent; Vec<Noeud> enfants; }   // ❌ à la compilation Rust
```

```java
public class Noeud {                                        // ✅
    Weak<RefCell<Noeud>> parent;
    Vec<Rc<RefCell<Noeud>>> enfants;
}
```

`ravac` ne vous arrêtera pas ici — c'est `rustc` qui le fera, avec le message
de Rust. C'est le comportement recherché.

---

<a id="reflexion"></a>
## Pas de réflexion, pas de chargement de classe

Ni `Class<?>`, ni `getDeclaredMethods()`, ni `Class.forName(...)`. Rust
monomorphise et efface tout à la compilation.

Ce qu'on met à la place : des traits (dispatch statique ou `dyn`), des `enum`
pour les ensembles fermés, et les macros pour la génération de code
(`@Derive`, `Macro.*`, `@Attr`).

---

<a id="statics"></a>
## `static` mutable : contraint, pas interdit

Un champ `static final` devient une `const` Rust, et c'est le cas courant :

```java
public static final i32 MAX = 100;         // -> const MAX: i32 = 100;
```

Un `static` **mutable** partagé n'est pas accessible sans synchronisation, comme
en Rust :

```java
@Attr("static COMPTEUR: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0)")
public class Compteur { }
```

ou `OnceLock` / `Mutex` pour un état initialisé paresseusement. Un `static mut`
nu exige `@Unsafe`, exactement comme en Rust.

---

<a id="variance"></a>
## Pas d'effacement de type, pas de covariance de tableau

`Object[] o = new String[1];` est légal en Java et échoue à l'exécution. En
Rust — donc en Rava — cela ne compile pas du tout. `Vec<String>` et
`Vec<Object>` sont des types sans relation.

De même, `List<String>` et `List<Integer>` sont deux types **distincts** à
l'exécution : la généricité est monomorphisée, pas effacée.

---

<a id="concurrence"></a>
## Pas de `synchronized`, pas de `finalize()`

`synchronized` est **refusé**, pas ignoré : il n'y a pas de moniteur par objet
en Rust, et laisser passer le mot-clé sans effet serait un piège. Protégez la
donnée, pas la méthode : `Mutex<T>`, `RwLock<T>`.

`finalize()` est en revanche [masqué](#masques) vers `Drop` :

```java
public class Fichier {
    protected void finalize() { System.out.println("fermeture"); }
}
```
```rust
impl Drop for Fichier {
    fn drop(&mut self) { println!("fermeture"); }
}
```

Vous pouvez aussi écrire l'implémentation directement :
`class Fichier implements Drop { @Override @Mut public void drop() { … } }`.

En contrepartie, Rust apporte `Send` / `Sync` : les erreurs de partage entre
fils deviennent des erreurs de compilation. Rava en hérite intégralement.

---

## Et ce qui n'est *pas* impossible

Ces éléments n'ont pas de syntaxe Java, mais restent tous exprimables en Rava —
voir [SYNTAX.md](SYNTAX.md) et [ANNOTATIONS.md](ANNOTATIONS.md) :

durées de vie (`@Lifetime`, `@Ref("a")`) · emprunts et emprunts mutables
(`@Ref`, `@Mut`) · déplacement et propriété · types non signés (`u8`…`u128`,
`usize`) · `impl Trait` et `dyn Trait` (`@Impl`, `@Dyn`) · types associés
(`type Item;`) · génériques constants (`<@Const usize N>`) · filtrage par motif
avec gardes et déconstruction (`switch` / `case ... when`) · `unsafe`
(`@Unsafe`) · pointeurs bruts (`@Ptr`) · `async` / `.await` (`@Async`,
`.await()`) · macros (`Macro.*`) · attributs (`@Attr`, `@Derive`, `@Repr`) ·
tuples (`Tuple<A, B>`, `Tuple.of(a, b)`) · tableaux de taille fixe
(`Array<T, N>`, `Arr.of`, `Arr.fill`) · arguments de type portés par le type
(`Vec::<T>::new()`) · intervalles (`Range.of`) · fermetures capturantes
(`Move.of`) · `Drop` (`finalize()`) · et, en dernier recours, du Rust brut
(`@Rust`, `Rust.expr`).

---

## Comment Rava vous le dit

Chaque cas ci-dessus produit un diagnostic ancré sur le `.rava`, avec un renvoi
vers la section correspondante :

```
`null` n'existe pas en Rust
  --> src/Compte.rava:14:20
   |
14 |         String nom = null;
   |                      ^
  = note: utilisez Option<T> : `None` / `Some(x)`. Voir docs/IMPOSSIBLE.md#null
```

Tout ce qui n'est pas listé ici est laissé à `rustc`, dans le vocabulaire de
Rust. C'est délibéré : Rava change la syntaxe, jamais les règles.
