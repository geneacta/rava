//! AST de Rava : la forme est celle de Java, la sémantique celle de Rust.
//!
//! Le parser produit cet arbre ; le codegen le traduit en Rust.

pub type Ident = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub line: u32,
    pub col: u32,
}

// ---------------------------------------------------------------- annotations

/// Une annotation Java : `@Mut`, `@Ref("a")`, `@Derive({"Debug", "Clone"})`.
#[derive(Debug, Clone)]
pub struct Annot {
    pub name: Ident,
    pub args: Vec<AnnotArg>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum AnnotArg {
    Str(String),
    Num(String),
    Ident(String),
    Array(Vec<AnnotArg>),
    /// `@Foo(key = value)`
    Named(Ident, Box<AnnotArg>),
}

impl Annot {
    /// Premier argument textuel, quelle que soit sa forme (`"a"`, `a`, `1`).
    pub fn first_text(&self) -> Option<&str> {
        self.args.first().and_then(|a| a.as_text())
    }

    /// Tous les arguments textuels, en aplatissant les tableaux `{...}`.
    pub fn texts(&self) -> Vec<String> {
        let mut out = Vec::new();
        for a in &self.args {
            a.collect_text(&mut out);
        }
        out
    }
}

impl AnnotArg {
    pub fn as_text(&self) -> Option<&str> {
        match self {
            AnnotArg::Str(s) | AnnotArg::Num(s) | AnnotArg::Ident(s) => Some(s),
            AnnotArg::Named(_, v) => v.as_text(),
            AnnotArg::Array(_) => None,
        }
    }

    fn collect_text(&self, out: &mut Vec<String>) {
        match self {
            AnnotArg::Str(s) | AnnotArg::Num(s) | AnnotArg::Ident(s) => out.push(s.clone()),
            AnnotArg::Array(v) => v.iter().for_each(|a| a.collect_text(out)),
            AnnotArg::Named(_, v) => v.collect_text(out),
        }
    }
}

/// Recherche d'une annotation par nom (insensible à la casse du premier caractère).
pub fn find_annot<'a>(annots: &'a [Annot], name: &str) -> Option<&'a Annot> {
    annots.iter().find(|a| a.name == name)
}

pub fn has_annot(annots: &[Annot], name: &str) -> bool {
    find_annot(annots, name).is_some()
}

// ---------------------------------------------------------------- modificateurs

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modifier {
    Public,
    Private,
    Protected,
    Static,
    Final,
    Abstract,
    Default,
    Native,
    Synchronized,
    Transient,
    Volatile,
    Strictfp,
    Sealed,
    NonSealed,
}

// ---------------------------------------------------------------- types

#[derive(Debug, Clone)]
pub struct Type {
    /// `@Ref`, `@Mut`, `@Ref("a")`, `@Ptr("mut")`, ... portées par le type.
    pub annots: Vec<Annot>,
    pub kind: TypeKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum TypeKind {
    /// `java.util.List<String>` -> segments + arguments génériques.
    Named { path: Vec<Ident>, args: Vec<Type> },
    /// `T[]`
    Array(Box<Type>),
    /// `void`
    Void,
    /// `var` : à inférer.
    Infer,
    /// Type fonction, écrit `Fn<(A, B), R>` en Rava.
    Never,
}

impl Type {
    pub fn simple(name: &str) -> Type {
        Type {
            annots: Vec::new(),
            kind: TypeKind::Named { path: vec![name.to_string()], args: Vec::new() },
            span: Span::default(),
        }
    }

    pub fn last_segment(&self) -> Option<&str> {
        match &self.kind {
            TypeKind::Named { path, .. } => path.last().map(|s| s.as_str()),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------- génériques

#[derive(Debug, Clone, Default)]
pub struct Generics {
    /// Durées de vie déclarées via `@Lifetime({"a", "b: a"})`.
    pub lifetimes: Vec<String>,
    pub params: Vec<GenericParam>,
    /// Clauses brutes issues de `@Where("T: Into<String>")`.
    pub where_clauses: Vec<String>,
}

impl Generics {
    pub fn is_empty(&self) -> bool {
        self.lifetimes.is_empty() && self.params.is_empty()
    }
}

#[derive(Debug, Clone)]
pub struct GenericParam {
    pub name: Ident,
    /// `T extends Display & Clone` -> deux bornes.
    pub bounds: Vec<Type>,
    /// `<@Const int N>` -> `const N: usize`.
    pub const_ty: Option<Type>,
}

// ---------------------------------------------------------------- unité

#[derive(Debug, Clone)]
pub struct Unit {
    pub package: Option<Vec<Ident>>,
    pub imports: Vec<Import>,
    pub items: Vec<Item>,
}

#[derive(Debug, Clone)]
pub struct Import {
    pub path: Vec<Ident>,
    /// `import foo.bar.*;`
    pub glob: bool,
    /// `import static` : ignoré côté Rust (tout `use` l'est déjà).
    pub is_static: bool,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum Item {
    Class(Class),
    Interface(Interface),
    Enum(EnumDecl),
    Record(Record),
}

// ---------------------------------------------------------------- class

/// `class` Rava -> `struct` + blocs `impl` en Rust.
#[derive(Debug, Clone)]
pub struct Class {
    pub annots: Vec<Annot>,
    pub modifiers: Vec<Modifier>,
    pub name: Ident,
    pub generics: Generics,
    /// `implements Display, Clone` -> un bloc `impl Trait for Struct` par entrée.
    pub implements: Vec<Type>,
    /// `extends` : interdit sur une classe (voir docs/IMPOSSIBLE.md), conservé
    /// pour produire un diagnostic clair.
    pub extends: Option<Type>,
    pub fields: Vec<Field>,
    pub methods: Vec<Method>,
    pub consts: Vec<ConstDecl>,
    /// `type Cle = String;` : types associés fournis à un trait implémenté.
    pub assoc_types: Vec<AssocType>,
    /// Types imbriqués -> modules/`impl` associés.
    pub nested: Vec<Item>,
    pub span: Span,
}

/// `record Point(int x, int y)` -> struct tuple-like nommée + constructeur.
#[derive(Debug, Clone)]
pub struct Record {
    pub annots: Vec<Annot>,
    pub modifiers: Vec<Modifier>,
    pub name: Ident,
    pub generics: Generics,
    pub components: Vec<Param>,
    pub implements: Vec<Type>,
    pub methods: Vec<Method>,
    pub consts: Vec<ConstDecl>,
    pub assoc_types: Vec<AssocType>,
    pub span: Span,
}

/// `interface` Rava -> `trait` Rust.
#[derive(Debug, Clone)]
pub struct Interface {
    pub annots: Vec<Annot>,
    pub modifiers: Vec<Modifier>,
    pub name: Ident,
    pub generics: Generics,
    /// `interface A extends B, C` -> `trait A: B + C`.
    pub extends: Vec<Type>,
    pub methods: Vec<Method>,
    pub consts: Vec<ConstDecl>,
    /// Types associés : `type Item;` dans le corps de l'interface.
    pub assoc_types: Vec<AssocType>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct AssocType {
    pub annots: Vec<Annot>,
    pub name: Ident,
    pub bounds: Vec<Type>,
    pub default: Option<Type>,
    pub span: Span,
}

/// `enum` Rava -> `enum` Rust (variantes à charge utile incluses).
#[derive(Debug, Clone)]
pub struct EnumDecl {
    pub annots: Vec<Annot>,
    pub modifiers: Vec<Modifier>,
    pub name: Ident,
    pub generics: Generics,
    pub implements: Vec<Type>,
    pub variants: Vec<Variant>,
    pub methods: Vec<Method>,
    pub consts: Vec<ConstDecl>,
    pub assoc_types: Vec<AssocType>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Variant {
    pub annots: Vec<Annot>,
    pub name: Ident,
    /// `Some(T value)` -> charge utile nommée ; vide -> variante unitaire.
    pub payload: Vec<Param>,
    /// `= 3` pour une enum C-like.
    pub discriminant: Option<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Field {
    pub annots: Vec<Annot>,
    pub modifiers: Vec<Modifier>,
    pub ty: Type,
    pub name: Ident,
    pub init: Option<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ConstDecl {
    pub annots: Vec<Annot>,
    pub modifiers: Vec<Modifier>,
    pub ty: Type,
    pub name: Ident,
    pub value: Expr,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Method {
    pub annots: Vec<Annot>,
    pub modifiers: Vec<Modifier>,
    pub generics: Generics,
    pub ret: Type,
    pub name: Ident,
    pub params: Vec<Param>,
    pub body: Option<Block>,
    /// Vrai pour un constructeur (`Point(...)` sans type de retour).
    pub is_ctor: bool,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub annots: Vec<Annot>,
    pub ty: Type,
    pub name: Ident,
    /// `int... rest` : non supporté en Rust, diagnostic dédié.
    pub varargs: bool,
    pub span: Span,
}

// ---------------------------------------------------------------- statements

#[derive(Debug, Clone, Default)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum Stmt {
    /// `var x = e;` / `int x = e;` / `@Mut var x = e;`
    Local {
        annots: Vec<Annot>,
        modifiers: Vec<Modifier>,
        ty: Type,
        name: Ident,
        init: Option<Expr>,
        span: Span,
    },
    /// Déstructuration : `var Point(x, y) = p;`
    LocalPattern { annots: Vec<Annot>, pat: Pattern, init: Expr, span: Span },
    Expr(Expr),
    Return(Option<Expr>, Span),
    If { cond: Expr, then: Box<Stmt>, otherwise: Option<Box<Stmt>>, span: Span },
    While { label: Option<Ident>, cond: Expr, body: Box<Stmt>, span: Span },
    DoWhile { label: Option<Ident>, body: Box<Stmt>, cond: Expr, span: Span },
    /// `for (init; cond; update) body`
    For {
        label: Option<Ident>,
        init: Vec<Stmt>,
        cond: Option<Expr>,
        update: Vec<Expr>,
        body: Box<Stmt>,
        span: Span,
    },
    /// `for (var x : it) body`
    ForEach {
        label: Option<Ident>,
        annots: Vec<Annot>,
        ty: Type,
        name: Ident,
        iter: Expr,
        body: Box<Stmt>,
        span: Span,
    },
    /// `loop { }` : écrit `for (;;)` en Java, reconnu ici.
    Loop { label: Option<Ident>, body: Box<Stmt>, span: Span },
    Break(Option<Ident>, Option<Expr>, Span),
    Continue(Option<Ident>, Span),
    Block(Block),
    /// `switch` statement (forme flèche uniquement) -> `match`.
    Switch(SwitchExpr),
    /// Bloc `unsafe { }` : `@Unsafe { }` en Rava.
    Unsafe(Block, Span),
    Empty,
}

// ---------------------------------------------------------------- expressions

#[derive(Debug, Clone)]
pub enum Expr {
    Lit(Lit, Span),
    /// Identifiant simple ou chemin `a.b.c` non résolu.
    Name(Vec<Ident>, Span),
    /// `this`
    This(Span),
    /// `super` -> non traduisible tel quel, diagnostic dédié.
    Super(Span),
    Unary { op: UnOp, expr: Box<Expr>, span: Span },
    /// `x++` / `x--`
    PostIncDec { op: UnOp, expr: Box<Expr>, span: Span },
    Binary { op: BinOp, lhs: Box<Expr>, rhs: Box<Expr>, span: Span },
    Assign { op: Option<BinOp>, target: Box<Expr>, value: Box<Expr>, span: Span },
    /// `cond ? a : b` -> `if cond { a } else { b }`
    Ternary { cond: Box<Expr>, then: Box<Expr>, otherwise: Box<Expr>, span: Span },
    /// Appel : `recv.name(args)` ou `name(args)` si `recv` est None.
    Call { recv: Option<Box<Expr>>, name: Ident, generics: Vec<Type>, args: Vec<Expr>, span: Span },
    /// `expr.field`
    Field { recv: Box<Expr>, name: Ident, span: Span },
    /// `a[i]`
    Index { recv: Box<Expr>, index: Box<Expr>, span: Span },
    /// `new Foo(args)` -> `Foo::new(args)`
    New { ty: Type, args: Vec<Expr>, span: Span },
    /// `new Foo{ x = 1, y = 2 }` -> littéral de struct Rust.
    StructLit { ty: Type, fields: Vec<(Ident, Expr)>, rest: Option<Box<Expr>>, span: Span },
    /// `new int[]{1, 2, 3}` -> `vec![1, 2, 3]`
    ArrayLit { ty: Option<Type>, elems: Vec<Expr>, span: Span },
    /// `(long) x` -> `x as i64`
    Cast { ty: Type, expr: Box<Expr>, span: Span },
    /// `(a, b) -> body`
    Lambda { params: Vec<LambdaParam>, body: Box<LambdaBody>, is_move: bool, span: Span },
    /// `Foo::bar`
    MethodRef { ty: Vec<Ident>, name: Ident, span: Span },
    /// `switch (x) { ... }` en position d'expression.
    Switch(Box<SwitchExpr>),
    /// Bloc-expression : `@Block { ... }`, la dernière expression est la valeur.
    Block(Box<Block>, Span),
    /// Opérateur `?` de Rust : `.q()` en Java strict, `expr?` en syntaxe étendue.
    Try(Box<Expr>, Span),
    /// `.await()` en Java strict, `expr.await` en syntaxe étendue.
    Await(Box<Expr>, Span),
    /// Emprunt explicite : `Ref.of(x)` / `Ref.mut_(x)`.
    Borrow { mutable: bool, expr: Box<Expr>, span: Span },
    /// Déréférencement : `Deref.of(x)`.
    Deref(Box<Expr>, Span),
    /// Intervalle : `Range.of(a, b)` / `Range.closed(a, b)`.
    Range { start: Option<Box<Expr>>, end: Option<Box<Expr>>, inclusive: bool, span: Span },
    /// Invocation de macro : `Macro.println("{}", x)` -> `println!("{}", x)`.
    Macro { name: Ident, args: Vec<Expr>, span: Span },
    /// Rust brut injecté tel quel : `Rust.expr("...")`.
    RawRust(String, Span),
    /// `x instanceof Foo f` -> diagnostic (voir docs/IMPOSSIBLE.md).
    InstanceOf { expr: Box<Expr>, ty: Type, binding: Option<Ident>, span: Span },
    /// Parenthèses conservées pour restituer la précédence à l'identique.
    Paren(Box<Expr>, Span),
}

impl Expr {
    pub fn span(&self) -> Span {
        use Expr::*;
        match self {
            Lit(_, s) | Name(_, s) | This(s) | Super(s) | Block(_, s) | RawRust(_, s) => *s,
            Unary { span, .. }
            | PostIncDec { span, .. }
            | Binary { span, .. }
            | Assign { span, .. }
            | Ternary { span, .. }
            | Call { span, .. }
            | Field { span, .. }
            | Index { span, .. }
            | New { span, .. }
            | StructLit { span, .. }
            | ArrayLit { span, .. }
            | Cast { span, .. }
            | Lambda { span, .. }
            | MethodRef { span, .. }
            | Borrow { span, .. }
            | Range { span, .. }
            | Macro { span, .. }
            | InstanceOf { span, .. } => *span,
            Try(_, s) | Await(_, s) | Deref(_, s) | Paren(_, s) => *s,
            Switch(sw) => sw.span,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LambdaParam {
    pub annots: Vec<Annot>,
    pub name: Ident,
    pub ty: Option<Type>,
}

#[derive(Debug, Clone)]
pub enum LambdaBody {
    Expr(Expr),
    Block(Block),
}

#[derive(Debug, Clone)]
pub struct SwitchExpr {
    pub scrutinee: Box<Expr>,
    pub arms: Vec<SwitchArm>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct SwitchArm {
    /// Plusieurs motifs séparés par `,` -> alternative `|` en Rust.
    pub patterns: Vec<Pattern>,
    /// `case X when cond ->` -> garde `if cond`.
    pub guard: Option<Expr>,
    pub body: SwitchArmBody,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum SwitchArmBody {
    Expr(Expr),
    Block(Block),
}

#[derive(Debug, Clone)]
pub enum Pattern {
    /// `default` -> `_`
    Wildcard(Span),
    /// Littéral : `case 1 ->`
    Lit(Lit, Span),
    /// Liaison simple : `case var x ->` ou `case Foo x ->`
    Binding { annots: Vec<Annot>, name: Ident, span: Span },
    /// Chemin sans charge utile : `case Color.RED ->`
    Path(Vec<Ident>, Span),
    /// Motif de déconstruction : `case Some(var v) ->`
    TupleStruct { path: Vec<Ident>, elems: Vec<Pattern>, span: Span },
    /// Motif nommé : `case Point(x = var a, y = var b) ->`
    Struct { path: Vec<Ident>, fields: Vec<(Ident, Pattern)>, rest: bool, span: Span },
    /// `case 1 .. 5 ->` via `Range.of(...)`
    Range { start: Box<Pattern>, end: Box<Pattern>, inclusive: bool, span: Span },
    /// `@Ref case ...` -> `ref`/`&`
    Ref { mutable: bool, inner: Box<Pattern>, span: Span },
}

#[derive(Debug, Clone)]
pub enum Lit {
    Int(String),
    Float(String),
    Str(String),
    Char(String),
    Bool(bool),
    /// `null` -> diagnostic : Rust n'a pas de null (voir docs/IMPOSSIBLE.md).
    Null,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
    BitNot,
    Plus,
    PreInc,
    PreDec,
    PostInc,
    PostDec,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    And,
    Or,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    /// `>>>` : pas d'équivalent direct, diagnostic dédié.
    UShr,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl BinOp {
    pub fn as_rust(self) -> &'static str {
        use BinOp::*;
        match self {
            Add => "+",
            Sub => "-",
            Mul => "*",
            Div => "/",
            Rem => "%",
            And => "&&",
            Or => "||",
            BitAnd => "&",
            BitOr => "|",
            BitXor => "^",
            Shl => "<<",
            Shr => ">>",
            UShr => ">>",
            Eq => "==",
            Ne => "!=",
            Lt => "<",
            Le => "<=",
            Gt => ">",
            Ge => ">=",
        }
    }
}
