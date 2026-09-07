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

| Construction Java | Statut | À écrire à la place |
|---|---|---|
| [`null`](#null) | ❌ impossible | `Option<T>` |
| [Exceptions (`try`/`catch`/`throw`/`throws`)](#exceptions) | ❌ impossible | `Result<T, E>` + `.q()` |
| [Héritage de classe (`extends`, `super`)](#heritage) | ❌ impossible | composition + interfaces (traits) |
| [`instanceof` / downcast](#instanceof) | ❌ impossible | `switch` sur enum, ou `dyn Any` |
| [Surcharge de méthode](#surcharge) | ❌ impossible | noms distincts, ou `@Named` |
| [Jokers génériques `<?>`](#wildcards) | ❌ impossible | paramètre borné, `@Impl`, `@Dyn` |
| [Varargs `int...`](#varargs) | ❌ impossible | slice `@Ref int[]` |
| [`>>>`](#ushr) | ❌ impossible | type non signé + `>>` |
| [`x++` en position d'expression](#incrementation) | ❌ impossible | instruction séparée, ou `x += 1` |
| [Classes anonymes / internes non statiques](#anonymes) | ❌ impossible | lambda, ou type nommé |
| [Ramasse-miettes, cycles d'objets](#gc) | ❌ impossible | `Rc`/`Arc` + `Weak` |
| [Réflexion, chargement dynamique](#reflexion) | ❌ impossible | traits, `enum`, macros |
| [`static` mutable partagé](#statics) | ⚠️ contraint | `const`, `static` immuable, `OnceLock` |
| [Généricité effacée / covariance de tableau](#variance) | ❌ impossible | monomorphisation Rust |
| [`synchronized`, `finalize`](#concurrence) | ❌ impossible | `Mutex`, `Drop` |

---

<a id="null"></a>
## `null` n'existe pas

Rust n'a pas de valeur nulle. Il n'y a donc rien à traduire, et `ravac` refuse
le littéral.

```java
String s = null;          // ❌ erreur de compilation Rava
```

```java
Option<String> s = None();                 // ✅
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

Pour appeler la version « parente » d'une méthode de trait, nommez-la
explicitement au lieu d'utiliser `super` :

```java
return Animal.crier(this);        // -> Animal::crier(self)
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
## Pas de `>>>`

Java a besoin de `>>>` parce que tous ses entiers sont signés. Rust a des types
non signés, où `>>` est déjà un décalage logique.

```java
int h = x >>> 3;                           // ❌
u32 h = x >> 3;                            // ✅
```

---

<a id="incrementation"></a>
## `x++` uniquement en position d'instruction

Rust n'a pas d'opérateur d'incrémentation. Rava accepte `x++;` et `++x;`
**comme instruction** (traduit en `x += 1;`), mais pas en tant que valeur.

```java
a[i++] = 0;                                // ❌ pas d'équivalent
int y = x++;                               // ❌

i++;                                       // ✅ instruction
for (@Mut var i = 0; i < n; i++) { }       // ✅ pas d'itération d'une boucle
```

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

Le modificateur `synchronized` est accepté par le parser (c'est un modificateur
Java) mais **sans effet** : il n'y a pas de moniteur par objet en Rust.
Utilisez `Mutex<T>` / `RwLock<T>`.

`finalize()` n'existe pas ; implémentez `Drop` :

```java
public class Fichier implements Drop {
    @Override @Mut public void drop() { System.out.println("fermeture"); }
}
```

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
tuples (`Tuple<A, B>`, `Tuple.of(a, b)`) · intervalles (`Range.of`) ·
fermetures capturantes (`Move.of`) · et, en dernier recours, du Rust brut
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
  = note: utilisez Option<T> : Option.none() / Option.some(x). Voir docs/IMPOSSIBLE.md#null
```

Tout ce qui n'est pas listé ici est laissé à `rustc`, dans le vocabulaire de
Rust. C'est délibéré : Rava change la syntaxe, jamais les règles.
