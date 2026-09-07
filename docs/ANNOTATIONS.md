# Annotations Rava

Les annotations sont le moyen d'écrire, **en Java valide**, ce que Rust exprime
avec une syntaxe que Java n'a pas : emprunts, mutabilité, durées de vie,
`unsafe`, attributs, macros de dérivation.

Elles ne sont jamais « interprétées à l'exécution » : `ravac` les consomme à la
traduction. Une annotation inconnue est ignorée.

---

## Propriété et emprunt

| Annotation | Porte sur | Effet |
|---|---|---|
| `@Ref` | type, paramètre, champ, motif | `&T` |
| `@Ref("a")` | idem | `&'a T` |
| `@Mut` | méthode | receveur `&mut self` |
| `@Mut` | paramètre / variable locale | liaison mutable (`mut x`) |
| `@Ref @Mut` | type, paramètre | `&mut T` |
| `@Owned` | méthode | receveur `self` (consomme) |
| `@Owned @Mut` | méthode | receveur `mut self` |
| `@Ptr("const")` / `@Ptr("mut")` | type | `*const T` / `*mut T` |

```java
public class Panier {
    private Vec<String> articles;

    /// `&self` : lecture seule.
    public usize taille() { return this.articles.len(); }

    /// `&mut self` : modification en place.
    @Mut public void ajouter(String a) { this.articles.push(a); }

    /// `self` : consomme le panier et rend son contenu.
    @Owned public Vec<String> vider() { return this.articles; }

    /// `&mut Vec<String>` en paramètre.
    public static void trier(@Ref @Mut Vec<String> v) { v.sort(); }
}
```

Sur une boucle `for`, `@Ref` porte sur l'itérable :

```java
for (@Ref var x : liste) { }        // for x in &liste
for (@Ref @Mut var x : liste) { }   // for x in &mut liste
for (var x : liste) { }             // for x in liste  (consomme)
```

---

## Durées de vie et généricité

| Annotation | Effet |
|---|---|
| `@Lifetime("a")` | déclare `<'a>` sur le type ou la méthode |
| `@Lifetime({"a", "b: a"})` | déclare `<'a, 'b: 'a>` |
| `@Where("T: Into<String>")` | ajoute une clause `where` |
| `@Const` | sur un paramètre générique : `<const N: usize>` |

```java
@Lifetime({"a", "b: a"})
@Where("T: Clone")
public static <T> @Ref("a") T premier(@Ref("a") T[] xs, @Ref("b") str nom) { … }
```

```java
public class Tampon<@Const usize N> {          // struct Tampon<const N: usize>
    private u8[] octets;
}
```

---

## Polymorphisme

| Annotation | Effet |
|---|---|
| `@Dyn` | `dyn Trait` |
| `@Impl` | `impl Trait` (argument ou retour) |
| `@Override` | place la méthode — ou le type associé — dans `impl Trait for T` |
| `@Override(Display.class)` | idem, en désignant le trait |

```java
public static void afficher(@Ref Vec<Box<@Dyn Shape>> formes) { … }
public static @Impl Iterator<Item = i32> pairs(usize n) { … }
```

---

## Attributs et dérivations

| Annotation | Rust généré |
|---|---|
| `@Derive({"Debug", "Clone"})` | `#[derive(Debug, Clone)]` |
| `@Derive({Debug.class, Clone.class})` | idem |
| `@Repr("C")` | `#[repr(C)]` |
| `@Inline` | `#[inline]` |
| `@Test` | `#[test]` |
| `@Attr("cfg(test)")` | `#[cfg(test)]` — passe-plat pour tout attribut |
| `@Doc("…")` | `/// …` |
| `/** … */` ou `/// …` | `/// …` (repris automatiquement) |

`@Derive` est la bonne façon d'obtenir `Clone`, `Copy`, `Debug`, `PartialEq`,
`Eq`, `Hash`, `Default`, `PartialOrd`, `Ord`. N'écrivez pas
`implements Clone` : cela produirait un `impl Clone for T` vide.

---

## Modificateurs de fonction

| Annotation | Rust généré |
|---|---|
| `@Unsafe` sur méthode | `unsafe fn` |
| `@Unsafe { … }` sur bloc | `unsafe { … }` |
| `@Async` | `async fn` |
| `@Extern("C")` | `extern "C" fn` |
| `@Named("nom_rust")` | renomme la fonction générée |

`@Named` sert surtout aux constructeurs multiples et à
[l'absence de surcharge](IMPOSSIBLE.md#surcharge) :

```java
public Tampon(usize n) { … }                 // -> Tampon::new
@Named("with_capacity")
public Tampon(usize n, usize cap) { … }      // -> Tampon::with_capacity
```

---

## Variantes d'enum

| Annotation | Effet |
|---|---|
| `@Struct` sur une variante | variante à champs nommés au lieu d'une variante tuple |

```java
public enum Forme {
    Cercle(f64 r),                       // Cercle(f64)
    @Struct Rect(f64 w, f64 h),          // Rect { w: f64, h: f64 }
}
```

---

## Échappatoire

Quand rien d'autre ne convient, `@Rust` et `Rust.expr` insèrent du Rust brut.
C'est délibérément visible : un `@Rust` dans le code signale que la syntaxe Java
n'a pas suffi.

```java
@Rust("#[cfg(target_os = \"linux\")]")
public class Specifique { }
```

```java
var v = Rust.expr("unsafe { std::ptr::read(p) }");
```

Rien n'oblige à y recourir pour les mécanismes courants : tout ce qui est listé
plus haut couvre l'essentiel de Rust. Voir [IMPOSSIBLE.md](IMPOSSIBLE.md) pour
la liste — courte — de ce qui reste hors d'atteinte.
