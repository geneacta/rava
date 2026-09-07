# Grammaire de Rava

Grammaire du langage tel que le parser l'accepte réellement. Elle décrit ce qui
**se lit** ; certaines constructions lues ici sont ensuite refusées par le
générateur, avec un diagnostic — c'est signalé au fil du texte, et détaillé dans
[IMPOSSIBLE.md](IMPOSSIBLE.md).

Notation :

| | |
|---|---|
| `a b` | séquence |
| `a \| b` | alternative |
| `[ a ]` | facultatif |
| `{ a }` | répétition, zéro ou plus |
| `( a )` | groupement |
| `"x"` | terminal |
| *italique* | non-terminal |

---

## 1. Lexique

L'indentation et les sauts de ligne n'ont **aucune signification** : ce sont des
séparateurs, au même titre que l'espace. Une instruction se termine par `;`, un
bloc par `}`.

```
ident        = ( lettre | "_" | "$" | non-ascii ) { lettre | chiffre | "_" | "$" | non-ascii } .

entier       = chiffre { chiffre | "_" } [ suffixe-entier ]
             | "0" ( "x" | "X" ) hexa { hexa | "_" }
             | "0" ( "b" | "B" ) ( "0" | "1" ) { "0" | "1" | "_" } .
suffixe-entier = "L" | "l" .

flottant     = chiffre { chiffre | "_" } "." chiffre { chiffre | "_" } [ exposant ] [ suffixe-flottant ]
             | chiffre { chiffre | "_" } exposant [ suffixe-flottant ]
             | chiffre { chiffre | "_" } suffixe-flottant .
exposant     = ( "e" | "E" ) [ "+" | "-" ] chiffre { chiffre } .
suffixe-flottant = "f" | "F" | "d" | "D" .

chaîne       = '"' { car-échappé } '"' .
bloc-texte   = '"""' saut-de-ligne { car } '"""' .       (* indentation commune retirée *)
caractère    = "'" car-échappé "'" .
car-échappé  = ? tout caractère sauf " et saut de ligne ? | "\" ? tout caractère ? .
```

Un `.` suivi d'un chiffre appartient au flottant ; sinon c'est un accès à un
champ : `1.5` est un nombre, `x.y` un accès.

Les échappements `\uXXXX` de Java deviennent `\u{XXXX}` en Rust ; les autres
(`\n`, `\t`, `\\`, `\"`, `\'`) coïncident.

**Commentaires.** `// …` jusqu'à la fin de ligne, `/* … */` sur plusieurs
lignes. `/// …` et `/** … */` sont de la documentation : ils sont attachés à la
déclaration qui suit et deviennent des `///` Rust.

**Opérateurs**, reconnus au plus long d'abord :

```
>>>=  <<=  >>=  >>>  ...  ->  ::  ++  --  &&  ||  ==  !=  <=  >=
+=  -=  *=  /=  %=  &=  |=  ^=  <<  >>
{  }  (  )  [  ]  ;  ,  .  =  >  <  !  ~  ?  :  +  -  *  /  &  |  ^  %  @
```

### Mots réservés

Ceux de Java, plus neuf mots propres à Rava :

```
unless    and    or    xor    nand    nor    xnor    implies    not
```

Contextuels — réservés uniquement en position de mot-clé, utilisables comme
identifiants ailleurs :

```
when    type    loop    sealed    default    permits
```

Refusés avec un renvoi documenté (voir [IMPOSSIBLE.md](IMPOSSIBLE.md)) :

```
null    try    catch    finally    throw    throws    instanceof
```

---

## 2. Unité de compilation

```
unité        = [ paquet ] { import } { item } .

paquet       = "package" nom-qualifié ";" .
import       = "import" [ "static" ] ident { "." ident } [ "." "*" ] ";" .

nom-qualifié = ident { "." ident } .
```

Dans un projet, le répertoire fait foi et la déclaration `package` doit le
confirmer — voir [PROJETS.md](PROJETS.md).

---

## 3. Annotations et modificateurs

```
annotations  = { annotation } .
annotation   = "@" ident [ "(" [ arg-annot { "," arg-annot } ] ")" ] .

arg-annot    = chaîne
             | entier | flottant
             | nom-qualifié                    (* `Debug.class` -> `Debug` *)
             | ident "=" arg-annot
             | "{" [ arg-annot { "," arg-annot } ] "}" .

modificateurs = { "public" | "private" | "protected" | "static" | "final"
                | "abstract" | "default" | "native" | "synchronized"
                | "transient" | "volatile" | "strictfp" | "sealed" } .
```

Un commentaire de documentation précédant une déclaration équivaut à une
annotation `@Doc`. La liste des annotations reconnues est dans
[ANNOTATIONS.md](ANNOTATIONS.md) ; une annotation inconnue est ignorée.

`synchronized` se lit mais est refusé à la génération.

---

## 4. Types

```
type         = annotations base { "[" "]" } .
base         = "void" | "var" | nom-qualifié [ args-type ] .

args-type    = "<" ">"                                     (* diamant *)
             | "<" arg-type { "," arg-type } ">" .
arg-type     = type | entier .                             (* générique constant *)
```

Le `>` fermant est scindé au besoin : `Map<String, Vec<i32>>` se lit malgré le
`>>` du lexeur.

Le joker `?` (`List<? extends T>`) est reconnu **pour être refusé**, avec un
renvoi vers [IMPOSSIBLE.md](IMPOSSIBLE.md#wildcards).

---

## 5. Génériques

```
génériques   = "<" param-générique { "," param-générique } ">" .
param-générique = annotations [ type ] ident [ "extends" type { "&" type } ] .
```

Le `type` en tête n'apparaît qu'avec `@Const` : `<@Const usize N>` déclare un
générique constant. Les durées de vie et les clauses `where` passent par
`@Lifetime` et `@Where` — ce sont des annotations, pas de la grammaire.

---

## 6. Déclarations de type

```
item         = annotations modificateurs déclaration .
déclaration  = classe | interface | enum | record .

classe       = "class" ident [ génériques ] [ "extends" type ] [ implements ]
               "{" { membre } "}" .

record       = "record" ident [ génériques ] "(" [ params ] ")" [ implements ]
               "{" { membre } "}" .

interface    = "interface" ident [ génériques ] [ "extends" type { "," type } ]
               "{" { type-associé | membre } "}" .

enum         = "enum" ident [ génériques ] [ implements ]
               "{" [ variantes ] [ ";" { membre } ] "}" .

implements   = ( "implements" | "permits" ) type { "," type } .

variantes    = variante { "," variante } [ "," ] .
variante     = annotations ident [ "(" [ params ] ")" ] [ "=" expression ] .
```

`extends` sur une **classe** se lit pour produire un diagnostic : Rust n'a pas
d'héritage. Sur une **interface**, il donne `trait A: B`.

### Membres

```
membre       = ";"
             | annotations modificateurs
               ( déclaration | type-associé | constructeur | méthode | champ ) .

constructeur = ident "(" [ params ] ")" bloc .        (* ident = nom du type *)
méthode      = [ génériques ] type ident "(" [ params ] ")" ( bloc | ";" ) .
champ        = type ident [ "=" expression ]
               { "," ident [ "=" expression ] } ";" .

type-associé = annotations "type" ident [ "extends" type { "&" type } ]
               [ "=" type ] ";" .

params       = param { "," param } .
param        = annotations [ "final" ] type [ "..." ] ident .
```

Un champ `static final` devient une constante associée. Les varargs `...` se
lisent pour être refusés.

---

## 7. Instructions

```
bloc         = "{" { instruction } "}" .

instruction  = ";"
             | annotation-unsafe bloc
             | bloc
             | conditionnelle
             | [ étiquette ] boucle
             | "return" [ expression ] ";"
             | "break" [ ident | expression ] ";"
             | "continue" [ ident ] ";"
             | switch [ ";" ]
             | déclaration-locale
             | expression ";" .

étiquette    = ident ":" .          (* seulement devant for, while, do, loop *)

conditionnelle = ( "if" | "unless" ) "(" expression ")" instruction
                 [ "else" instruction ] .

boucle       = "while" "(" expression ")" instruction
             | "do" instruction "while" "(" expression ")" ";"
             | "loop" bloc
             | "for" "(" tête-for ")" instruction .

tête-for     = ";" ";"                                          (* infinie *)
             | annotations [ "final" ] type ident ":" expression  (* for-each *)
             | [ init-for ] ";" [ expression ] ";" [ maj-for ] .
init-for     = déclaration-locale-sans-point-virgule
             | expression { "," expression } ";" .
maj-for      = expression { "," expression } .

déclaration-locale = annotations modificateurs type
                     ( déclarateur { "," déclarateur }
                     | motif-décomposition "=" expression ) ";" .
déclarateur  = ident [ "=" expression ] .
```

`var a = 1, b = 2;` déclare deux variables sans ouvrir de portée — le `for`
classique en dépend : `for (var i = 0, j = n; i < j; i++, j--)`.

`while (true)` et `for (;;)` deviennent la boucle infinie de Rust.
`unless (c)` vaut `if (!c)`, l'opérateur de comparaison étant inversé plutôt
que préfixé d'un `!`. Une déclaration de type dans un corps de méthode est
refusée avec un renvoi.

**Ambiguïté résolue par retour arrière** : `Type ident` peut ouvrir une
déclaration locale ou une expression. Le parser tente la déclaration ; en cas
d'échec il rembobine et lit une expression.

---

## 8. Filtrage

```
switch       = "switch" "(" expression ")" "{" { bras } "}" .

bras         = ( "default" | "case" motif { "," motif } )
               [ "when" expression ] "->" ( expression ";" | bloc ) .

motif        = annotations motif-nu .
motif-nu     = littéral [ "..." motif ]                  (* intervalle fermé *)
             | "-" littéral
             | "default" | "_"
             | "true" | "false"
             | "var" ident
             | nom-qualifié "(" [ champs-motif ] ")"     (* déconstruction *)
             | nom-qualifié ident                        (* motif de type *)
             | nom-qualifié .                            (* chemin nu *)

champs-motif = élément-motif { "," élément-motif } .
élément-motif = ident "=" motif | motif | "..." .
```

Seule la **forme flèche** est acceptée ; `case X:` est refusé avec une
correction rapide. `@Ref` devant un motif donne `ref`, `@Ref @Mut` donne
`ref mut`.

Dans une garde `when`, `->` ferme le bras : `case P when x == y -> …` se lit
sans confondre `y ->` avec une lambda.

---

## 9. Expressions

```
expression   = affectation .

affectation  = ternaire [ op-affectation expression ] .
op-affectation = "=" | "+=" | "-=" | "*=" | "/=" | "%="
               | "&=" | "|=" | "^=" | "<<=" | ">>=" | ">>>=" .

ternaire     = binaire [ "?" affectation ":" affectation ] .

binaire      = [ "not" ] unaire { opérateur-binaire [ "not" ] unaire }
             | unaire "instanceof" type [ ident ] .

unaire       = ( "-" | "+" | "!" | "~" | "++" | "--" ) unaire | postfixe .

postfixe     = primaire { suffixe } .
suffixe      = "." [ args-type ] ident [ "::" args-type ] [ "(" [ args ] ")" ]
             | "." "await"
             | "::" [ args-type ] [ "::" ] ident [ "(" [ args ] ")" ]
             | "[" expression "]"
             | "++" | "--"
             | "?" .
args         = expression { "," expression } .
```

### Précédence

Du plus lâche au plus serré. Tous associatifs à gauche, sauf l'affectation, le
ternaire et `implies`, associatifs à droite.

| | Opérateurs |
|---|---|
| 1 | `=` `+=` `-=` `*=` `/=` `%=` `&=` `\|=` `^=` `<<=` `>>=` `>>>=` |
| 2 | `?:` |
| 3 | `implies` |
| 4 | `or` · `nor` |
| 5 | `xor` · `xnor` |
| 6 | `and` · `nand` |
| 7 | `\|\|` |
| 8 | `&&` |
| 9 | `\|` |
| 10 | `^` |
| 11 | `&` |
| 12 | `==` `!=` |
| 13 | `<` `<=` `>` `>=` `instanceof` |
| 14 | `<<` `>>` `>>>` |
| 15 | `+` `-` |
| 16 | `*` `/` `%` |
| 17 | unaires préfixes `-` `+` `!` `~` `++` `--` |
| 18 | suffixes `.` `::` `[]` `++` `--` `?` |

`not` est un préfixe qui lie plus lâche que les comparaisons et plus serré que
`and` : `not a == b` vaut `not (a == b)`, donc `a != b`.

La précédence des opérateurs littéraux n'est pas celle des symboles Rust
correspondants — `^` lie plus fort que `&&` en Rust, l'inverse en Rava. Le
générateur parenthèse ce qu'il faut pour que le sens écrit soit le sens
compilé.

### Expressions primaires

```
primaire     = littéral
             | "(" ")"                                  (* valeur unité *)
             | lambda
             | "(" type ")" unaire                      (* transtypage *)
             | "(" expression ")"
             | "true" | "false" | "this" | "super"
             | création
             | switch
             | nom-qualifié [ "(" [ args ] ")" ] .

littéral     = entier | flottant | chaîne | bloc-texte | caractère .

lambda       = ident "->" corps-lambda
             | "(" [ param-lambda { "," param-lambda } ] ")" "->" corps-lambda .
param-lambda = annotations [ type ] ident .
corps-lambda = expression | bloc .

création     = "new" type "{" [ args ] "}"                     (* new int[]{…} *)
             | "new" type "(" [ args ] ")" [ "{" litt-struct "}" ] .
litt-struct  = champ-struct { "," champ-struct } .
champ-struct = ident "=" expression | "..." expression .
```

`null` se lit pour être refusé. `super` n'est traduit que dans la forme
`super.méthode(…)`, et seulement sur une méthode que la classe ne redéfinit pas.

**Trois ambiguïtés** résolues par retour arrière, dans cet ordre : la valeur
unité `()`, la lambda `(a, b) -> …`, le transtypage `(Type) x`, puis
l'expression parenthésée.

---

## 10. Façades

Certaines constructions Rust n'ont pas de syntaxe Java. Elles s'écrivent comme
des appels statiques, reconnus par le parser :

```
Macro.<nom>(args)      -> nom!(args)          Ref.of(x) / Ref.mut_(x) -> &x / &mut x
Deref.of(p)            -> *p                  Tuple.of(a, b)          -> (a, b)
Arr.of(a, b)           -> [a, b]              Arr.fill(v, n)          -> [v; n]
Range.of(a, b)         -> a..b                Range.closed(a, b)      -> a..=b
Range.from(a)          -> a..                 Range.to(b)             -> ..b
Move.of(x -> …)        -> move |x| …          Unit.of()               -> ()
Rust.expr("…")         -> Rust brut           System.out.println(…)   -> println!(…)
e.q()                  -> e?                  e.await()               -> e.await
```

---

## 11. Extensions hors Java

Trois formes venues de Rust sortent de la syntaxe Java stricte. Chacune a un
équivalent strict, indiqué à droite :

| Extension | Équivalent Java strict |
|---|---|
| `e?` | `e.q()` |
| `e.await` | `e.await()` |
| `x.parse::<i32>()` | `x.<i32>parse()` |

Un fichier qui n'utilise que la colonne de droite reste du Java syntaxiquement
valide, ouvrable dans un IDE Java.

**Le `?` cède la priorité au ternaire.** Les deux se disputent le même
caractère, et la syntaxe Java l'emporte : `?` n'est l'opérateur de Rust que là
où aucune branche de ternaire ne peut commencer.

| Écriture | Lecture |
|---|---|
| `f()?;` · `f()?.g()` · `f()? * 2` | opérateur `?` — rien ne peut suivre en expression |
| `f()? + 1` | **ternaire** — `+1` est une expression valide |
| `a ? b : c` · `a ? -1 : 1` | ternaire |

En cas de doute, `.q()` n'est jamais ambigu.

---

## 12. Ce que la grammaire lit mais que le générateur refuse

Ces formes se lisent — c'est délibéré : cela permet un diagnostic qui explique,
plutôt qu'une erreur de syntaxe opaque.

| Forme | Refus |
|---|---|
| `class A extends B` | pas d'héritage de classe |
| `super` seul | pas de classe parente |
| `super.m()` depuis `m` | boucle infinie, refusée |
| `synchronized` | pas de moniteur par objet |
| `T... x` | pas de varargs |
| `x instanceof T` | pas de sous-typage nominal |
| `x++` en expression | accepté, mais la place est relue deux fois |
| `@Override` ambigu | plusieurs interfaces, trait à préciser |
| constructeur incomplet | champ non initialisé |

Le détail, et quoi écrire à la place, est dans [IMPOSSIBLE.md](IMPOSSIBLE.md).
