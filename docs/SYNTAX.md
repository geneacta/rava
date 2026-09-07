# Syntaxe Rava

Un fichier `.rava` est du **Java syntaxiquement valide** — un IDE Java le
colorise, un parser Java l'accepte — mais il décrit du **Rust**.

Ce document est la table de correspondance complète. Ce qui n'y figure pas et
n'est pas dans [IMPOSSIBLE.md](IMPOSSIBLE.md) passe tel quel à `rustc`.

---

## 1. Fichier, paquet, imports

| Rava | Rust |
|---|---|
| `package a.b.c;` | (informatif — l'arborescence des fichiers fait foi) |
| `import std.collections.HashMap;` | `use std::collections::HashMap;` |
| `import std.fmt.*;` | `use std::fmt::*;` |

Un `.rava` produit un `.rs` de même nom.

---

## 2. Types

Les noms Rust s'écrivent **tels quels** : `i32`, `u64`, `usize`, `f64`, `str`,
`String`, `Vec`, `Option`, `Result`, `Box`, `Rc`, `Arc`, `HashMap`… Rava
n'invente pas de vocabulaire.

Les primitives Java sont des **alias** de leur équivalent Rust :

| Rava | Rust | | Rava | Rust |
|---|---|---|---|---|
| `int` | `i32` | | `boolean` | `bool` |
| `long` | `i64` | | `float` | `f32` |
| `short` | `i16` | | `double` | `f64` |
| `byte` | `i8` | | `char` | `char` |
| `void` | `()` | | `var` | inféré |

Les types **non signés** n'ont pas de nom Java : on écrit directement `u8`,
`u16`, `u32`, `u64`, `u128`, `usize`, `i128`, `isize`.

### Formes composées

| Rava | Rust | Note |
|---|---|---|
| `T[]` | `Vec<T>` | tableau possédé |
| `@Ref T[]` | `&[T]` | tranche empruntée |
| `@Ref T` | `&T` | emprunt partagé |
| `@Ref @Mut T` | `&mut T` | emprunt exclusif |
| `@Ref("a") T` | `&'a T` | emprunt à durée de vie nommée |
| `@Ptr("const") T` | `*const T` | pointeur brut (exige `@Unsafe` à l'usage) |
| `@Ptr("mut") T` | `*mut T` | |
| `Box<@Dyn Shape>` | `Box<dyn Shape>` | polymorphisme dynamique |
| `@Impl Iterator<Item = i32>` | `impl Iterator<Item = i32>` | |
| `Tuple<A, B>` | `(A, B)` | |
| `Unit` | `()` | |
| `Fn2<A, B, R>` | `Fn(A, B) -> R` | idem `FnMut2`, `FnOnce2`, pour 0 à N arguments |

---

<a id="generiques"></a>
## 3. Généricité et durées de vie

| Rava | Rust |
|---|---|
| `<T>` | `<T>` |
| `<T extends Display>` | `<T: Display>` |
| `<T extends Display & Clone>` | `<T: Display + Clone>` |
| `@Where("T: Into<String>")` | `where T: Into<String>` |
| `@Lifetime("a")` | `<'a>` |
| `@Lifetime({"a", "b: a"})` | `<'a, 'b: 'a>` |
| `<@Const usize N>` | `<const N: usize>` |
| `Tampon<8>` / `f.<8>capacite()` | `Tampon<8>` / `f::<8>()` (argument constant) |

```java
@Lifetime("a")
public static @Ref("a") str longest(@Ref("a") str a, @Ref("a") str b) {
    unless (a.len() > b.len()) { return b; }
    return a;
}
```
```rust
pub fn longest<'a>(a: &'a str, b: &'a str) -> &'a str {
    if a.len() <= b.len() { return b; }
    return a;
}
```

Les arguments de type d'un **type** se placent sur la déclaration ; ceux d'une
**méthode** s'écrivent à la Java :

```java
HashMap<String, i32> m = HashMap.new();   // HashMap::new(), typé par la déclaration
var n = s.<i32>parse();                   // s.parse::<i32>()
```

---

## 4. Déclarations de type

| Rava | Rust |
|---|---|
| `class C { … }` | `struct C { … }` + `impl C` |
| `record P(int x, int y)` | `struct P { pub x: i32, pub y: i32 }` + `new` + accesseurs |
| `interface I { … }` | `trait I { … }` |
| `interface I extends A, B` | `trait I: A + B` |
| `enum E { … }` | `enum E { … }` |
| `class C implements I` | `impl I for C` |
| `static class N { … }` (imbriquée) | item Rust au même niveau |

### Visibilité

| Rava | Rust |
|---|---|
| `public` | `pub` |
| `protected` | `pub(crate)` |
| `private` / rien | privé |

### Classe

```java
@Derive({"Debug", "Clone"})
public class Compte {
    private String titulaire;
    private i64 solde;

    public Compte(String titulaire) {
        this.titulaire = titulaire;
        this.solde = 0;
    }

    @Mut public void crediter(i64 montant) { this.solde += montant; }
    public i64 solde() { return this.solde; }
}
```
```rust
#[derive(Debug, Clone)]
pub struct Compte { titulaire: String, solde: i64 }

impl Compte {
    pub fn new(titulaire: String) -> Self { Self { titulaire, solde: 0 } }
    pub fn crediter(&mut self, montant: i64) { self.solde += montant; }
    pub fn solde(&self) -> i64 { return self.solde; }
}
```

Le constructeur devient `new`. Ses affectations `this.champ = …;` deviennent le
littéral de structure final ; tout champ non affecté doit avoir une valeur par
défaut à la déclaration, sinon `ravac` le signale.

### Receveur de méthode

| Rava | Rust |
|---|---|
| (par défaut) | `&self` |
| `@Mut` | `&mut self` |
| `@Owned` | `self` |
| `@Owned @Mut` | `mut self` |
| `static` | pas de receveur |
| `@Ref("a")` | `&'a self` |

### Interface

```java
public interface Iterateur {
    type Item;                                  // type associé
    i32 MAX = 100;                              // constante associée
    Option<Item> next(@Mut ...);                // méthode requise
    default bool estVide() { return false; }    // méthode par défaut
}
```
```rust
pub trait Iterateur {
    type Item;
    const MAX: i32 = 100;
    fn next(&mut self) -> Option<Self::Item>;
    fn estVide(&self) -> bool { return false; }
}
```

`@Override` place la méthode dans le bon `impl Trait for`. Avec plusieurs
interfaces, précisez laquelle : `@Override(Display.class)`.

La classe qui implémente fournit les types associés dans son propre corps :

```java
public class Compteur implements Iterateur {
    type Item = i32;                            // -> type Item = i32;
    @Override public Option<i32> next() { … }
}
```

### Enum

```java
@Derive({"Debug"})
public enum Message {
    Quitter,
    Bouger(i32 x, i32 y),
    @Struct Ecrire(String texte),
    Code = 3;
}
```
```rust
#[derive(Debug)]
pub enum Message {
    Quitter,
    Bouger(i32, i32),
    Ecrire { texte: String },
    Code = 3,
}
```

Une variante avec charge utile devient une variante **tuple** ; les noms des
champs servent de documentation. `@Struct` produit une variante **nommée**.

---

## 5. Instructions

| Rava | Rust |
|---|---|
| `var x = e;` | `let x = e;` |
| `int x = e;` | `let x: i32 = e;` |
| `@Mut var x = e;` | `let mut x = e;` |
| `var Point(var a, var b) = p;` | `let Point(a, b) = p;` (déstructuration) |
| `if (c) {} else {}` | `if c {} else {}` |
| `unless (c) {}` | `if !c {}` |
| `else unless (c) {}` | `else if !c {}` |
| `while (c) {}` | `while c {}` |
| `while (true) {}` / `for (;;) {}` | `loop {}` |
| `do {} while (c);` | `loop { …; if !c { break; } }` |
| `for (var x : it) {}` | `for x in it {}` |
| `for (@Ref var x : it) {}` | `for x in &it {}` |
| `for (@Ref @Mut var x : it) {}` | `for x in &mut it {}` |
| `for (init; c; pas) {}` | boucle désucrée, `continue` exécute le pas |
| `outer: for (…) { break outer; }` | `'outer: for … { break 'outer; }` |
| `switch (e) { … }` | `match e { … }` |
| `@Unsafe { … }` | `unsafe { … }` |
| `return e;` | `return e;` |

`unless` se lit « sauf si ». `ravac` inverse l'opérateur de comparaison plutôt
que d'empiler des `!` : `unless (x > 0)` donne `if x <= 0`.

---

<a id="switch"></a>
## 6. `switch` → `match`

Seule la **forme flèche** est acceptée (`case X -> …;`). La forme `case X:`
avec chute est refusée : elle n'a pas d'équivalent dans un `match`.

| Motif Rava | Motif Rust |
|---|---|
| `case 1 ->` | `1 =>` |
| `case 1, 2, 3 ->` | `1 \| 2 \| 3 =>` |
| `case 1 ... 5 ->` | `1..=5 =>` |
| `case Color.RED ->` | `Color::RED =>` |
| `case var x ->` | `x =>` |
| `case Some(var v) ->` | `Some(v) =>` |
| `case Point(x = var a, y = var b) ->` | `Point { x: a, y: b } =>` |
| `case Point(x = var a, ...) ->` | `Point { x: a, .. } =>` |
| `case @Ref var x ->` | `ref x =>` |
| `case X when c ->` | `X if c =>` |
| `default ->` | `_ =>` |

`switch` s'utilise en instruction comme en expression :

```java
var libelle = switch (msg) {
    case Message.Quitter -> "fin";
    case Message.Bouger(var x, var y) when x == y -> "diagonale";
    case Message.Bouger(var x, var y) -> "déplacement";
    default -> "autre";
};
```

---

## 7. Opérateurs

### Logiques littéraux

Sept mots réservés, plus `not`. Ils s'écrivent en toutes lettres et fonctionnent
partout où une expression est admise.

| Rava | Rust | Table de vérité |
|---|---|---|
| `a and b` | `a && b` | conjonction (court-circuit) |
| `a or b` | `a \|\| b` | disjonction (court-circuit) |
| `a xor b` | `a ^ b` | ou exclusif |
| `a nand b` | `!(a && b)` | non-et |
| `a nor b` | `!(a \|\| b)` | non-ou |
| `a xnor b` | `!(a ^ b)` | équivalence |
| `a implies b` | `!a \|\| b` | implication |
| `not a` | `!a` | négation |

**Précédence**, du plus lâche au plus serré :

```
implies  <  or, nor  <  xor, xnor  <  and, nand  <  not  <  comparaisons  <  arithmétique
```

`implies` est **associatif à droite** (`a implies b implies c` vaut
`a implies (b implies c)`), les autres sont associatifs à gauche.

`not` lie plus lâche que les comparaisons : `not a == b` vaut `not (a == b)`,
donc `a != b`. Rava inverse l'opérateur plutôt que d'ajouter un `!`.

La précédence des mots n'est pas celle des symboles Rust (`^` lie plus fort que
`&&` en Rust, l'inverse en Rava) : `ravac` parenthèse ce qu'il faut pour que le
sens écrit soit le sens compilé.

```java
if (age >= 18 implies consentement and not banni) { … }
```
```rust
if !(age >= 18) || (consentement && !banni) { … }
```

### Symboliques

`+ - * / % == != < <= > >= && || ! & | ^ << >>` : identiques à Java et à Rust.
`>>>` n'existe pas (voir [IMPOSSIBLE.md](IMPOSSIBLE.md#ushr)).

| Rava | Rust |
|---|---|
| `c ? a : b` | `if c { a } else { b }` |
| `(long) x` | `x as i64` |
| `x++;` (instruction) | `x += 1;` |

---

## 8. Expressions

| Rava | Rust |
|---|---|
| `this` | `self` |
| `new Point(1, 2)` | `Point::new(1, 2)` |
| `new Point() { x = 1, y = 2 }` | `Point { x: 1, y: 2 }` |
| `new Point() { x = 1, ... base }` | `Point { x: 1, ..base }` |
| `new int[]{1, 2, 3}` | `vec![1, 2, 3]` |
| `Color.RED` | `Color::RED` |
| `p.x` | `p.x` |
| `Foo.bar(x)` | `Foo::bar(x)` |
| `obj.bar(x)` | `obj.bar(x)` |
| `Foo::bar` | `Foo::bar` |
| `(a, b) -> a + b` | `\|a, b\| a + b` |
| `Move.of(x -> x + n)` | `move \|x\| x + n` |
| `Ref.of(x)` / `Ref.mut_(x)` | `&x` / `&mut x` |
| `Deref.of(p)` | `*p` |
| `Tuple.of(a, b)` | `(a, b)` |
| `Range.of(a, b)` | `a..b` |
| `Range.closed(a, b)` | `a..=b` |
| `Range.from(a)` / `Range.to(b)` | `a..` / `..b` |
| `Macro.println("{}", x)` | `println!("{}", x)` |
| `Macro.vec(1, 2)` / `Macro.format(…)` / `Macro.panic(…)` | `vec!` / `format!` / `panic!` |
| `System.out.println(fmt, args)` | `println!(fmt, args)` |
| `System.err.println(fmt, args)` | `eprintln!(fmt, args)` |
| `e.q()` | `e?` |
| `e.await()` | `e.await` |
| `Rust.expr("…")` | Rust brut, inséré tel quel |

Toute macro Rust est accessible via `Macro.<nom>(args)`. C'est le point
d'entrée générique : ce qui n'a pas de syntaxe Java passe par là.

---

<a id="extensions"></a>
## 9. Extensions hors Java

Rava accepte trois formes venues de Rust, qui **sortent** de la syntaxe Java
stricte. Elles sont facultatives : chacune a un équivalent Java strict.

| Extension | Équivalent Java strict |
|---|---|
| `e?` | `e.q()` |
| `e.await` | `e.await()` |
| `x.parse::<i32>()` | `x.<i32>parse()` |

Un fichier qui n'utilise que la colonne de droite reste ouvrable dans un IDE
Java.

---

## 10. Point d'entrée

```java
public static void main(String[] args) { … }
```

devient une fonction `fn main()` libre. Si `args` est déclaré, `ravac` ouvre le
corps par `let args: Vec<String> = std::env::args().collect();`.

---

## 11. Mots réservés Rava

En plus des mots-clés Java, Rava réserve :

```
unless    and    or    xor    nand    nor    xnor    implies    not
when      type   loop
```

`when` (garde de `case`), `type` (type associé) et `loop` ne sont réservés qu'en
position de mot-clé. Les huit opérateurs logiques et `unless` sont réservés
partout : ne les utilisez pas comme noms de variables.

Les mots-clés Java sans équivalent Rust (`try`, `catch`, `finally`, `throw`,
`throws`, `instanceof`, `super`, `null`) sont refusés avec un renvoi vers
[IMPOSSIBLE.md](IMPOSSIBLE.md).
