//! Dictionnaire des constructions Rava : ce que chacune devient en Rust.
//!
//! Sert à la fois le survol (hover) et la complétion, pour que l'éditeur dise
//! toujours la même chose que la documentation.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Annotation,
    Keyword,
    Facade,
    Member,
    Type,
    Refused,
}

#[derive(Debug, Clone, Copy)]
pub struct Entry {
    pub label: &'static str,
    pub kind: Kind,
    /// Ce que la construction produit en Rust, affiché en tête du survol.
    pub rust: &'static str,
    pub doc: &'static str,
    /// Texte inséré par la complétion, au format « snippet » LSP.
    pub snippet: Option<&'static str>,
}

const fn e(
    label: &'static str,
    kind: Kind,
    rust: &'static str,
    doc: &'static str,
    snippet: Option<&'static str>,
) -> Entry {
    Entry { label, kind, rust, doc, snippet }
}

pub const ANNOTATIONS: &[Entry] = &[
    e("Ref", Kind::Annotation, "&T  /  &'a T", "Emprunt partagé. `@Ref(\"a\")` nomme la durée de vie. Sur une méthode, `@Ref(\"a\")` donne `&'a self`.", Some("Ref")),
    e("Mut", Kind::Annotation, "&mut self  /  mut x  /  &mut T", "Sur une méthode : receveur `&mut self`. Sur un paramètre ou une variable : liaison mutable. Combiné à `@Ref` : `&mut T`.", Some("Mut")),
    e("Owned", Kind::Annotation, "self", "Receveur pris par valeur : la méthode consomme l'objet. `@Owned @Mut` donne `mut self`.", Some("Owned")),
    e("Ptr", Kind::Annotation, "*const T  /  *mut T", "Pointeur brut. `@Ptr(\"mut\")` pour `*mut T`. Le déréférencement exige `@Unsafe`.", Some("Ptr(\"${1:const}\")")),
    e("Lifetime", Kind::Annotation, "<'a, 'b: 'a>", "Déclare les durées de vie d'un type ou d'une méthode. `@Lifetime({\"a\", \"b: a\"})` pour plusieurs.", Some("Lifetime(\"${1:a}\")")),
    e("Where", Kind::Annotation, "where T: Into<String>", "Ajoute une clause `where` brute à la déclaration.", Some("Where(\"${1:T: Clone}\")")),
    e("Const", Kind::Annotation, "const N: usize", "Sur un paramètre générique : générique constant. S'écrit `<@Const usize N>`.", Some("Const")),
    e("Dyn", Kind::Annotation, "dyn Trait", "Polymorphisme dynamique. S'utilise dans `Box<@Dyn Shape>`.", Some("Dyn")),
    e("Impl", Kind::Annotation, "impl Trait", "`impl Trait` en position d'argument ou de retour.", Some("Impl")),
    e("Override", Kind::Annotation, "impl Trait for T { ... }", "Place la méthode — ou le type associé — dans le bloc `impl Trait for`. Avec plusieurs interfaces : `@Override(Display.class)`.", Some("Override")),
    e("Derive", Kind::Annotation, "#[derive(...)]", "Dérivations automatiques : Debug, Clone, Copy, PartialEq, Eq, Hash, Default, PartialOrd, Ord.", Some("Derive({\"${1:Debug}\"})")),
    e("Repr", Kind::Annotation, "#[repr(C)]", "Représentation mémoire du type.", Some("Repr(\"${1:C}\")")),
    e("Attr", Kind::Annotation, "#[...]", "Passe-plat vers n'importe quel attribut Rust : `@Attr(\"cfg(test)\")`.", Some("Attr(\"${1:cfg(test)}\")")),
    e("Doc", Kind::Annotation, "/// ...", "Commentaire de documentation. La javadoc `/** */` et `///` sont repris automatiquement.", Some("Doc(\"${1:...}\")")),
    e("Inline", Kind::Annotation, "#[inline]", "Suggère l'inlining au compilateur.", Some("Inline")),
    e("Test", Kind::Annotation, "#[test]", "Marque une fonction de test.", Some("Test")),
    e("Unsafe", Kind::Annotation, "unsafe fn  /  unsafe { }", "Sur une méthode : `unsafe fn`. Devant un bloc : `unsafe { ... }`.", Some("Unsafe")),
    e("Async", Kind::Annotation, "async fn", "Fonction asynchrone. L'attente s'écrit `expr.await()`.", Some("Async")),
    e("Extern", Kind::Annotation, "extern \"C\" fn", "Convention d'appel étrangère.", Some("Extern(\"${1:C}\")")),
    e("Named", Kind::Annotation, "fn nom_rust", "Renomme la fonction générée. Indispensable pour un deuxième constructeur, Rust n'ayant pas de surcharge.", Some("Named(\"${1:with_capacity}\")")),
    e("Struct", Kind::Annotation, "Variante { champ: T }", "Sur une variante d'enum : produit une variante à champs nommés au lieu d'une variante tuple.", Some("Struct")),
    e("Rust", Kind::Annotation, "code Rust brut", "Insère du Rust tel quel au niveau de l'item. Échappatoire de dernier recours.", Some("Rust(\"${1:#[cfg(unix)]}\")")),
];

pub const KEYWORDS: &[Entry] = &[
    e("unless", Kind::Keyword, "if !cond", "« Sauf si ». `unless (x > 0)` produit `if x <= 0` : l'opérateur est inversé, pas préfixé d'un `!`.", Some("unless (${1:cond}) {\n\t$0\n}")),
    e("and", Kind::Keyword, "&&", "Conjonction à court-circuit. Lie plus fort que `or`, moins fort que `not`.", None),
    e("or", Kind::Keyword, "||", "Disjonction à court-circuit.", None),
    e("xor", Kind::Keyword, "^", "Ou exclusif.", None),
    e("nand", Kind::Keyword, "!(a && b)", "Non-et. Même précédence que `and`.", None),
    e("nor", Kind::Keyword, "!(a || b)", "Non-ou. Même précédence que `or`.", None),
    e("xnor", Kind::Keyword, "!(a ^ b)", "Équivalence. Même précédence que `xor`.", None),
    e("implies", Kind::Keyword, "!a || b", "Implication. L'opérateur le plus lâche, associatif à droite.", None),
    e("not", Kind::Keyword, "!a", "Négation. Lie plus lâche que les comparaisons : `not a == b` vaut `a != b`.", None),
    e("when", Kind::Keyword, "if garde", "Garde d'un bras de `switch` : `case X when cond ->`.", None),
    e("switch", Kind::Keyword, "match", "Filtrage par motif. Seule la forme flèche `case X -> ...;` est acceptée.", Some("switch (${1:e}) {\n\tcase ${2:P} -> $0;\n\tdefault -> ;\n}")),
    e("record", Kind::Keyword, "struct + new + accesseurs", "Structure à champs publics, avec constructeur et accesseurs générés.", Some("record ${1:Nom}(${2:i32 x}) {\n\t$0\n}")),
    e("interface", Kind::Keyword, "trait", "Trait Rust. Peut porter des types associés (`type Item;`), des constantes et des méthodes par défaut.", Some("interface ${1:Nom} {\n\t$0\n}")),
    e("implements", Kind::Keyword, "impl Trait for T", "Implémente une interface. Ce n'est pas de l'héritage.", None),
    e("var", Kind::Keyword, "let", "Liaison à type inféré. `@Mut var` pour `let mut`.", None),
    e("type", Kind::Keyword, "type Item;", "Type associé. Déclaré dans l'interface, défini dans la classe : `type Item = i32;`.", Some("type ${1:Item} = ${2:i32};")),
];

pub const FACADES: &[Entry] = &[
    e("Macro", Kind::Facade, "nom!(args)", "Invoque n'importe quelle macro Rust : `Macro.format(\"{}\", x)` devient `format!(\"{}\", x)`.", None),
    e("Ref", Kind::Facade, "&x  /  &mut x", "`Ref.of(x)` emprunte, `Ref.mut_(x)` emprunte en écriture.", None),
    e("Deref", Kind::Facade, "*p", "`Deref.of(p)` déréférence.", None),
    e("Range", Kind::Facade, "a..b", "`Range.of(a, b)`, `Range.closed(a, b)`, `Range.from(a)`, `Range.to(b)`.", None),
    e("Tuple", Kind::Facade, "(a, b)", "`Tuple.of(a, b)` construit un tuple ; `Tuple<A, B>` est son type.", None),
    e("Arr", Kind::Facade, "[a, b, c]  /  [v; n]", "`Arr.of(a, b, c)` et `Arr.fill(v, n)` construisent un tableau de taille fixe ; `Array<T, N>` est son type.", None),
    e("Move", Kind::Facade, "move |x| ...", "`Move.of(x -> ...)` produit une fermeture capturante.", None),
    e("Rust", Kind::Facade, "code Rust brut", "`Rust.expr(\"...\")` insère une expression Rust telle quelle.", None),
    e("System", Kind::Facade, "println!  /  eprintln!", "`System.out.println(fmt, args)` et `System.err.println(...)`. Le format est celui de Rust.", None),
];

/// Membres proposés après `Facade.`
pub const MEMBERS: &[(&str, &[Entry])] = &[
    ("Macro", &[
        e("println", Kind::Member, "println!(...)", "Écrit sur la sortie standard, avec un saut de ligne.", Some("println(\"${1:{}}\", $0)")),
        e("format", Kind::Member, "format!(...)", "Construit une `String`.", Some("format(\"${1:{}}\", $0)")),
        e("vec", Kind::Member, "vec![...]", "Construit un `Vec`.", Some("vec($0)")),
        e("panic", Kind::Member, "panic!(...)", "Interrompt le fil d'exécution. Ce n'est pas une exception : ce n'est pas rattrapable au fil du code.", Some("panic(\"${1:message}\")")),
        e("assert", Kind::Member, "assert!(...)", "Vérifie une condition.", Some("assert($0)")),
        e("assert_eq", Kind::Member, "assert_eq!(...)", "Vérifie une égalité.", Some("assert_eq($1, $2)")),
        e("write", Kind::Member, "write!(...)", "Écrit dans un `Formatter` ou un flux.", Some("write($0)")),
        e("todo", Kind::Member, "todo!()", "Marque une partie non écrite.", Some("todo()")),
    ]),
    ("Ref", &[
        e("of", Kind::Member, "&x", "Emprunt partagé.", Some("of($0)")),
        e("mut_", Kind::Member, "&mut x", "Emprunt exclusif.", Some("mut_($0)")),
    ]),
    ("Deref", &[e("of", Kind::Member, "*p", "Déréférence.", Some("of($0)"))]),
    ("Range", &[
        e("of", Kind::Member, "a..b", "Intervalle ouvert à droite.", Some("of($1, $2)")),
        e("closed", Kind::Member, "a..=b", "Intervalle fermé.", Some("closed($1, $2)")),
        e("from", Kind::Member, "a..", "Intervalle sans borne haute.", Some("from($0)")),
        e("to", Kind::Member, "..b", "Intervalle sans borne basse.", Some("to($0)")),
    ]),
    ("Tuple", &[e("of", Kind::Member, "(a, b)", "Construit un tuple.", Some("of($1, $2)"))]),
    ("Arr", &[
        e("of", Kind::Member, "[a, b, c]", "Tableau de taille fixe, éléments énumérés.", Some("of($0)")),
        e("fill", Kind::Member, "[v; n]", "Tableau de taille fixe, `n` copies de `v`.", Some("fill($1, $2)")),
    ]),
    ("Move", &[e("of", Kind::Member, "move |x| ...", "Fermeture capturante.", Some("of($0)"))]),
    ("Rust", &[e("expr", Kind::Member, "code brut", "Insère une expression Rust telle quelle.", Some("expr(\"$0\")"))]),
];

pub const TYPES: &[Entry] = &[
    e("int", Kind::Type, "i32", "Alias Java de `i32`.", None),
    e("long", Kind::Type, "i64", "Alias Java de `i64`.", None),
    e("short", Kind::Type, "i16", "Alias Java de `i16`.", None),
    e("byte", Kind::Type, "i8", "Alias Java de `i8`.", None),
    e("float", Kind::Type, "f32", "Alias Java de `f32`.", None),
    e("double", Kind::Type, "f64", "Alias Java de `f64`.", None),
    e("boolean", Kind::Type, "bool", "Alias Java de `bool`.", None),
    e("String", Kind::Type, "String", "Chaîne possédée. Pour une tranche empruntée : `@Ref str`.", None),
    e("str", Kind::Type, "str", "Tranche de chaîne. S'emploie presque toujours en `@Ref str`.", None),
    e("usize", Kind::Type, "usize", "Entier non signé de la taille d'un pointeur : indices, longueurs.", None),
    e("isize", Kind::Type, "isize", "Entier signé de la taille d'un pointeur.", None),
    e("u8", Kind::Type, "u8", "Entier non signé 8 bits.", None),
    e("u16", Kind::Type, "u16", "Entier non signé 16 bits.", None),
    e("u32", Kind::Type, "u32", "Entier non signé 32 bits.", None),
    e("u64", Kind::Type, "u64", "Entier non signé 64 bits.", None),
    e("u128", Kind::Type, "u128", "Entier non signé 128 bits.", None),
    e("i8", Kind::Type, "i8", "Entier signé 8 bits.", None),
    e("i16", Kind::Type, "i16", "Entier signé 16 bits.", None),
    e("i32", Kind::Type, "i32", "Entier signé 32 bits.", None),
    e("i64", Kind::Type, "i64", "Entier signé 64 bits.", None),
    e("i128", Kind::Type, "i128", "Entier signé 128 bits.", None),
    e("f32", Kind::Type, "f32", "Flottant 32 bits.", None),
    e("f64", Kind::Type, "f64", "Flottant 64 bits.", None),
    e("Unit", Kind::Type, "()", "Type unité. En position de retour, écrivez plutôt `void`.", None),
    e("Tuple", Kind::Type, "(A, B)", "Type tuple : `Tuple<A, B>`.", None),
    e("Array", Kind::Type, "[T; N]", "Tableau de taille fixe : `Array<i32, 3>`.", None),
    e("Option", Kind::Type, "Option<T>", "Absence possible. Remplace `null`.", None),
    e("Result", Kind::Type, "Result<T, E>", "Succès ou échec. Remplace les exceptions ; se propage avec `.q()`.", None),
    e("Box", Kind::Type, "Box<T>", "Allocation sur le tas, propriété unique.", None),
    e("Rc", Kind::Type, "Rc<T>", "Compteur de références, mono-fil.", None),
    e("Arc", Kind::Type, "Arc<T>", "Compteur de références atomique, multi-fil.", None),
    e("Vec", Kind::Type, "Vec<T>", "Tableau dynamique. S'écrit aussi `T[]`.", None),
];

/// Constructions Java refusées : le survol explique pourquoi et quoi écrire.
pub const REFUSED: &[Entry] = &[
    e("null", Kind::Refused, "— rien —", "Rust n'a pas de valeur nulle. Utilisez `Option<T>` : `None`, ou `Some(x)`.\n\nVoir `docs/IMPOSSIBLE.md#null`.", None),
    e("try", Kind::Refused, "— rien —", "Rust n'a pas d'exceptions. Retournez un `Result<T, E>` et propagez avec `.q()`.\n\nVoir `docs/IMPOSSIBLE.md#exceptions`.", None),
    e("catch", Kind::Refused, "— rien —", "Rust n'a pas d'exceptions. Filtrez le `Result` avec un `switch`.\n\nVoir `docs/IMPOSSIBLE.md#exceptions`.", None),
    e("finally", Kind::Refused, "— rien —", "Pas d'exceptions, donc pas de `finally`. Le nettoyage se fait par `Drop` — écrivez `finalize()`, Rava le traduit.\n\nVoir `docs/IMPOSSIBLE.md#exceptions`.", None),
    e("throw", Kind::Refused, "— rien —", "Retournez `Err(...)` au lieu de lever.\n\nVoir `docs/IMPOSSIBLE.md#exceptions`.", None),
    e("throws", Kind::Refused, "— rien —", "L'échec se déclare dans le type de retour : `Result<T, E>`.\n\nVoir `docs/IMPOSSIBLE.md#exceptions`.", None),
    e("instanceof", Kind::Refused, "— rien —", "Rust n'a pas de sous-typage nominal. Filtrez une `enum` avec `switch`, ou passez par `dyn Any`.\n\nVoir `docs/IMPOSSIBLE.md#instanceof`.", None),
    e("extends", Kind::Refused, "trait : oui — classe : non", "Sur une `interface`, `extends` donne `trait A: B`. Sur une `class`, il n'y a pas d'héritage : composez, ou passez par une interface.\n\nVoir `docs/IMPOSSIBLE.md#heritage`.", None),
    e("super", Kind::Refused, "Trait::m(self, ...)", "`super.m(...)` est traduit — mais seulement pour une méthode que la classe ne redéfinit pas. Sur celle que vous redéfinissez, l'appel boucle.\n\nVoir `docs/IMPOSSIBLE.md#heritage`.", None),
    e("synchronized", Kind::Refused, "— rien —", "Pas de moniteur par objet en Rust. Protégez la donnée : `Mutex<T>`, `RwLock<T>`.\n\nVoir `docs/IMPOSSIBLE.md#concurrence`.", None),
    e("finalize", Kind::Refused, "impl Drop", "Traduit en `impl Drop` — et là, l'exécution est déterministe, à la sortie de portée.\n\nVoir `docs/IMPOSSIBLE.md#masques`.", None),
];

/// Recherche un mot dans toutes les tables, `is_annot` orientant vers `@Nom`.
pub fn lookup(word: &str, is_annot: bool) -> Option<&'static Entry> {
    if is_annot {
        return ANNOTATIONS.iter().find(|x| x.label == word);
    }
    REFUSED
        .iter()
        .chain(KEYWORDS)
        .chain(FACADES)
        .chain(TYPES)
        .find(|x| x.label == word)
}

pub fn members_of(facade: &str) -> Option<&'static [Entry]> {
    MEMBERS.iter().find(|(n, _)| *n == facade).map(|(_, m)| *m)
}
