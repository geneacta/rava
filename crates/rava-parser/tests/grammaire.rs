//! Chaque production documentée dans `docs/GRAMMAIRE.md` est exercée ici.
//!
//! Une grammaire écrite à la main dérive de son implémentation ; ces tests
//! l'en empêchent. Si l'un d'eux casse, c'est le document qu'il faut corriger,
//! ou le parser — jamais le test qu'il faut assouplir.

use rava_parser::parse;

#[track_caller]
fn ok(src: &str) {
    if let Err(e) = parse(src) {
        panic!("devrait se lire :\n{src}\n\nerreur : {e}");
    }
}

#[track_caller]
fn refuse(src: &str, attendu: &str) {
    match parse(src) {
        Ok(_) => panic!("devrait être refusé :\n{src}"),
        Err(e) => {
            let texte = format!("{e}");
            assert!(texte.contains(attendu), "message inattendu : {texte}");
        }
    }
}

/// Enveloppe un corps de méthode.
fn m(body: &str) -> String {
    format!("class T {{ void f() {{ {body} }} }}")
}

// ------------------------------------------------------------------ § 1 lexique

#[test]
fn litteraux() {
    ok(&m(r#"var a = 1; var b = 1_000; var c = 0xFF; var d = 0b1010; var e = 12L;"#));
    ok(&m(r#"var a = 1.5; var b = 1e9; var c = 1.5e-3; var d = 2.0f; var e = 3.0d;"#));
    ok(&m(r#"var s = "x"; var t = "café"; var c = 'é'; var n = '\n';"#));
    ok("class T { String s = \"\"\"\n    ligne 1\n    ligne 2\n    \"\"\"; }");
}

#[test]
fn un_point_suivi_dun_chiffre_est_un_flottant_sinon_un_champ() {
    ok(&m("var a = 1.5; var b = p.x;"));
}

#[test]
fn commentaires_et_documentation() {
    ok("// tête\n/// doc\npublic class T { /* bloc */ }");
    ok("/** Javadoc.\n * suite\n */\npublic class T { }");
}

#[test]
fn les_mots_contextuels_restent_des_identifiants() {
    ok(&m("var type = 1; var loop = 2; var when = 3;"));
    ok("class T { i32 sealed; i32 permits; }");
}

// ------------------------------------------------------------------ § 2 unité

#[test]
fn unite_de_compilation() {
    ok("package a.b.c;\nimport std.collections.HashMap;\nimport static a.b.C;\nimport std.fmt.*;\nclass T { }");
}

// ------------------------------------------------------------------ § 3 annotations

#[test]
fn formes_dannotation() {
    ok("@Derive({\"Debug\", \"Clone\"}) class T { }");
    ok("@Derive({Debug.class}) class T { }");
    ok("@Repr(\"C\") @Inline class T { }");
    ok("@Attr(value = \"cfg(test)\") class T { }");
    ok("@Lifetime({\"a\", \"b: a\"}) class T { }");
    ok("@Inconnue class T { }"); // une annotation inconnue est ignorée
}

// ------------------------------------------------------------------ § 4 types

#[test]
fn formes_de_type() {
    ok("class T { void f(int a, i64 b, usize c, @Ref str d, @Ref @Mut Vec<i32> e) { } }");
    ok("class T { Map<String, Vec<i32>> m; }"); // `>>` scindé
    ok("class T { Vec<Vec<Vec<i32>>> m; }");
    ok("class T { i32[] a; @Ref i32[] b; i32[][] c; }");
    ok("class T { Box<@Dyn Forme> f; }");
    ok("class T { void f(@Impl Iterator<i32> it) { } }");
    ok("class T { @Ptr(\"mut\") u8 p; }");
    ok("class T { Tuple<i32, String> t; Array<i32, 3> a; Fn2<i32, i32, i32> g; }");
    ok(&m("var v = new Vec<>();"));
}

#[test]
fn le_joker_generique_est_refuse() {
    refuse("class T { void f(Vec<? extends Forme> v) { } }", "jokers génériques");
}

// ------------------------------------------------------------------ § 5 génériques

#[test]
fn parametres_generiques() {
    ok("class T<A> { }");
    ok("class T<A extends Display> { }");
    ok("class T<A extends Display & Clone, B> { }");
    ok("class T<@Const usize N> { }");
    ok("class T { <A extends Clone> A f(A a) { return a; } }");
}

// ------------------------------------------------------------------ § 6 déclarations

#[test]
fn declarations_de_type() {
    ok("public class C implements A, B { }");
    ok("public record P(i32 x, i32 y) implements A { i32 s() { return this.x; } }");
    ok("public interface I extends A, B { type Item; i32 MAX = 1; i32 f(); default i32 g() { return 1; } }");
    ok("public enum E { A, B(i32 v), @Struct C(i32 x), D = 3; i32 f() { return 1; } }");
    ok("public enum E { A, B, }"); // virgule finale
    ok("class Ext { static class Interne { } }");
}

#[test]
fn membres() {
    ok("class T { T(i32 a) { } }");
    ok("class T { i32 a, b = 2, c; }");
    ok("class T { static final i32 MAX = 10; }");
    ok("class T { abstract i32 f(); }");
    ok("class T { type Cle = String; }");
    ok("class T { <A> void f(A a) { } }");
    ok("class T { ; ; }");
}

// ------------------------------------------------------------------ § 7 instructions

#[test]
fn instructions() {
    ok(&m("if (a) { g(); } else if (b) { h(); } else { i(); }"));
    ok(&m("unless (a) { g(); } else unless (b) { h(); } else { i(); }"));
    ok(&m("while (a) { g(); }"));
    ok(&m("while (true) { g(); }"));
    ok(&m("do { g(); } while (a);"));
    ok(&m("loop { g(); }"));
    ok(&m("for (;;) { g(); }"));
    ok(&m("for (@Mut var i = 0; i < 10; i++) { g(); }"));
    ok(&m("for (var i = 0, j = 1; i < j; i++, j--) { g(); }"));
    ok(&m("var a = 1, b = 2, c;"));
    ok(&m("for (var x : liste) { g(); }"));
    ok(&m("for (@Ref @Mut var x : liste) { g(); }"));
    ok(&m("outer: while (true) { break outer; }"));
    ok(&m("outer: for (;;) { continue outer; }"));
    ok(&m("return;"));
    ok("class T { i32 f() { return 1; } }");
    ok(&m("@Unsafe { g(); }"));
    ok(&m("{ g(); }"));
    ok(&m(";"));
}

#[test]
fn declarations_locales() {
    ok(&m("var x = 1;"));
    ok(&m("i32 x = 1;"));
    ok(&m("@Mut var x = 1;"));
    ok(&m("final i32 x = 1;"));
    ok(&m("Vec<String> v = Vec.new();"));
    ok(&m("var Point(var a, var b) = p;"));
    ok(&m("var Point(x = var a, y = var b) = p;"));
}

#[test]
fn une_declaration_de_type_dans_une_methode_est_refusee() {
    refuse(&m("class Interne { }"), "déclaration de type dans un corps de méthode");
}

// ------------------------------------------------------------------ § 8 filtrage

#[test]
fn motifs() {
    ok(&m("var r = switch (e) { case 1 -> a; default -> b; };"));
    ok(&m("var r = switch (e) { case 1, 2, 3 -> a; default -> b; };"));
    ok(&m("var r = switch (e) { case 1 ... 5 -> a; default -> b; };"));
    ok(&m("var r = switch (e) { case -1 -> a; default -> b; };"));
    ok(&m("var r = switch (e) { case \"x\" -> a; default -> b; };"));
    ok(&m("var r = switch (e) { case true -> a; case false -> b; };"));
    ok(&m("var r = switch (e) { case Color.RED -> a; default -> b; };"));
    ok(&m("var r = switch (e) { case var x -> x; };"));
    ok(&m("var r = switch (e) { case Some(var v) -> v; case None -> b; };"));
    ok(&m("var r = switch (e) { case Point(x = var a, ...) -> a; };"));
    ok(&m("var r = switch (e) { case @Ref var x -> x; };"));
    ok(&m("var r = switch (e) { case Circle c -> c; default -> b; };"));
    ok(&m("var r = switch (e) { case Some(var v) when v > 0 -> v; default -> 0; };"));
    ok(&m("switch (e) { case Some(var v) -> { g(v); } default -> { } }"));
}

#[test]
fn une_garde_ne_se_confond_pas_avec_une_lambda() {
    ok(&m("var r = switch (e) { case Pair(var x, var y) when x == y -> 1; default -> 0; };"));
}

#[test]
fn la_forme_deux_points_du_switch_est_refusee() {
    refuse(&m("switch (e) { case 1: g(); }"), "forme `case X:`");
}

// ------------------------------------------------------------------ § 9 expressions

#[test]
fn operateurs() {
    ok(&m("var r = a + b * c - d / e % f;"));
    ok(&m("var r = a << 1 | b >> 2 & c ^ d;"));
    ok(&m("var r = a && b || c;"));
    ok(&m("var r = a == b != c;"));
    ok(&m("var r = a < b <= c;"));
    ok(&m("var r = -a + +b + !c + ~d;"));
    ok(&m("var r = a ? b : c;"));
    ok(&m("a = b; a += 1; a -= 1; a *= 2; a /= 2; a %= 2;"));
    ok(&m("a &= 1; a |= 1; a ^= 1; a <<= 1; a >>= 1;"));
    ok(&m("i++; i--; ++i; --i;"));
    ok(&m("var r = i++ + ++i;"));
}

#[test]
fn operateurs_litteraux() {
    ok(&m("var r = a and b or c;"));
    ok(&m("var r = a nand b nor c xnor d;"));
    ok(&m("var r = a xor b;"));
    ok(&m("var r = a implies b implies c;"));
    ok(&m("var r = not a;"));
    ok(&m("var r = not a == b;"));
    ok(&m("var r = a and not b;"));
    ok(&m("var r = a > 0 implies b and not c;"));
}

#[test]
fn expressions_primaires() {
    ok(&m("var u = ();"));
    ok(&m("var u = Unit.of();"));
    ok(&m("var r = (a + b) * c;"));
    ok(&m("var r = (i64) x;"));
    ok(&m("var r = this.champ;"));
    ok(&m("var r = new Point(1, 2);"));
    ok(&m("var r = new Point() { x = 1, y = 2 };"));
    ok(&m("var r = new Point() { x = 1, ... base };"));
    ok(&m("var r = new i32[]{1, 2, 3};"));
    ok(&m("var r = tableau[0];"));
    ok(&m("var r = Foo.bar(1).baz().qux;"));
    ok(&m("var r = Foo::bar;"));
    ok(&m("var r = Vec::<i32>::new();"));
    ok(&m("var r = x.<i32>parse();"));
    ok(&m("var r = x.parse::<i32>();"));
}

#[test]
fn lambdas() {
    ok(&m("var f = x -> x + 1;"));
    ok(&m("var f = (a, b) -> a + b;"));
    ok(&m("var f = (i32 a, i32 b) -> a + b;"));
    ok(&m("var f = () -> 1;"));
    ok(&m("var f = (x) -> { return x + 1; };"));
    ok(&m("var f = Move.of(x -> x + n);"));
}

#[test]
fn facades() {
    ok(&m("Macro.println(\"{}\", x);"));
    ok(&m("var r = Macro.format(\"{}\", x);"));
    ok(&m("var r = Ref.of(x); var s = Ref.mut_(x); var t = Deref.of(p);"));
    ok(&m("var r = Tuple.of(a, b); var s = Arr.of(1, 2); var t = Arr.fill(0, 4);"));
    ok(&m("var r = Range.of(0, 5); var s = Range.closed(0, 5); var t = Range.from(0); var u = Range.to(5);"));
    ok(&m("var r = Rust.expr(\"1 + 1\");"));
    ok(&m("System.out.println(\"{}\", x); System.err.println(\"{}\", x);"));
}

#[test]
fn extensions_hors_java() {
    ok(&m("var r = f().q();"));
    ok(&m("var r = f()?;"));
    ok(&m("var r = f().await();"));
    ok(&m("var r = f().await;"));
}

/// L'extension `?` et le ternaire Java partagent le même caractère. La
/// syntaxe Java l'emporte : `?` n'est l'opérateur de Rust que là où aucune
/// branche de ternaire ne peut commencer. Ailleurs, on écrit `.q()`.
#[test]
fn le_point_dinterrogation_cede_la_priorite_au_ternaire() {
    ok(&m("var r = a ? b : c;"));
    ok(&m("var r = a ? -1 : 1;"));
    ok(&m("var r = f()? * 2;"));   // `*` ne peut pas ouvrir une expression
    ok(&m("var r = f()?;"));
    ok(&m("var r = f()?.g();"));
    ok(&m("var r = f().q() + 1;")); // forme stricte, jamais ambiguë
    refuse(&m("var r = f()? + 1;"), "`:` attendu");
}

// ------------------------------------------------------------------ § 11 refus

#[test]
fn constructions_refusees() {
    refuse(&m("var x = null;"), "`null` n'existe pas");
    refuse(&m("try { g(); } catch (E e) { }"), "`try` n'existe pas");
    refuse(&m("throw new E();"), "`throw` n'existe pas");
    refuse("class T { void f() throws E { } }", "`throws` n'existe pas");
}

// ------------------------------------------------------------------ mise en page

#[test]
fn lindentation_ne_porte_aucun_sens() {
    ok("class T{void f(){var x=1;if(x>0){g();}}}");
    ok("class\nT\n{\nvoid\nf\n(\n)\n{\nvar\nx\n=\n1\n;\n}\n}");
}

#[test]
fn un_point_virgule_manquant_est_une_erreur() {
    refuse(&m("var x = 1\nvar y = 2;"), "`;`");
}
