# Rava dans votre éditeur

Deux briques, réutilisables partout :

| Brique | Ce qu'elle apporte |
|---|---|
| **`rava-lsp`** — serveur de langage | erreurs en direct, survol documenté, complétion, plan du fichier, corrections rapides |
| **`rava.tmLanguage.json`** — grammaire TextMate | coloration syntaxique |

Le serveur partage le lexer, le parser et le générateur de `ravac` : ce que
l'éditeur signale est exactement ce que le compilateur refusera.

## Installer le serveur

```sh
cargo install --path crates/rava-lsp     # depuis le dépôt
# ou, sans installation :
cargo build --release                    # -> target/release/rava-lsp
```

Vérifiez qu'il est joignable : `rava-lsp --help` n'existe pas, le serveur parle
uniquement le protocole sur l'entrée standard. S'il démarre sans rien afficher
et attend, c'est bon.

---

## Le raccourci universel : traiter `.rava` comme du Java

Un fichier `.rava` est du **Java syntaxiquement valide**. Avant toute
installation, associer l'extension au langage Java donne déjà l'essentiel de la
coloration, le repli de code et la navigation par accolades — dans n'importe
quel éditeur.

| Éditeur | Où |
|---|---|
| VS Code | `"files.associations": { "*.rava": "java" }` |
| IntelliJ / Android Studio | Settings → Editor → File Types → Java → ajouter `*.rava` |
| Eclipse | Preferences → General → Content Types → Text → Java Source File → ajouter `*.rava` |
| Sublime | View → Syntax → Open all with current extension as… → Java |

Ce que cela ne donne pas : `unless`, les opérateurs logiques littéraux, les
types Rust, les façades — et surtout, aucun diagnostic Rava. D'où ce qui suit.

---

## VS Code

```sh
cd editors/vscode
npm install
code --install-extension .          # ou : F5 dans VS Code pour un essai
```

Ou, sans empaqueter : copiez `editors/vscode` dans `~/.vscode/extensions/rava/`
après le `npm install`.

Réglages disponibles :

```jsonc
{
  // Si `rava-lsp` n'est pas dans le PATH :
  "rava.server.path": "${workspaceFolder}/target/release/rava-lsp",
  "rava.server.enable": true,

  // Appelle `rustc` à l'enregistrement : emprunt, durées de vie, typage.
  "rava.check.onSave": true
}
```

Deux commandes, dans la palette :

- **Rava : afficher le Rust généré** — ouvre le `.rs` produit à côté du source.
  C'est le meilleur moyen d'apprendre Rust : votre code, traduit.
- **Rava : redémarrer le serveur de langage**

---

## IntelliJ IDEA / PyCharm / CLion

**Coloration** — IntelliJ lit les grammaires TextMate nativement :

> Settings → Editor → TextMate Bundles → **+** → sélectionner le dossier
> `editors/vscode` du dépôt.

Le dossier contient déjà `package.json`, `syntaxes/` et
`language-configuration.json` : IntelliJ y trouve tout ce qu'il attend.

**Diagnostics et complétion** — via le pont LSP :

> Installer le plugin **LSP4IJ** (JetBrains Marketplace), puis
> Settings → Languages & Frameworks → Language Servers → **+** :
>
> - Name : `Rava`
> - Command : `rava-lsp`
> - File name patterns : `*.rava`, Language ID : `rava`

Sur IntelliJ **Ultimate** 2023.2+, le support LSP natif fonctionne aussi, mais
LSP4IJ marche sur toutes les éditions, Community comprise.

---

## Neovim

Coloration : `.rava` est du Java, le greffon Tree-sitter Java suffit.

```lua
vim.filetype.add({ extension = { rava = "rava" } })

vim.api.nvim_create_autocmd("FileType", {
  pattern = "rava",
  callback = function()
    vim.treesitter.start(0, "java")          -- coloration
    vim.bo.commentstring = "// %s"
  end,
})

-- Serveur de langage (Neovim 0.11+)
vim.lsp.config.rava = {
  cmd = { "rava-lsp" },
  filetypes = { "rava" },
  root_markers = { "Cargo.toml", ".git" },
}
vim.lsp.enable("rava")
```

Sur Neovim antérieur à 0.11, avec `nvim-lspconfig` :

```lua
require("lspconfig.configs").rava = {
  default_config = {
    cmd = { "rava-lsp" },
    filetypes = { "rava" },
    root_dir = require("lspconfig.util").root_pattern("Cargo.toml", ".git"),
  },
}
require("lspconfig").rava.setup({})
```

---

## Helix

Dans `~/.config/helix/languages.toml` :

```toml
[[language]]
name = "rava"
scope = "source.rava"
file-types = ["rava"]
comment-token = "//"
indent = { tab-width = 4, unit = "    " }
language-servers = ["rava-lsp"]
grammar = "java"                 # la syntaxe est celle de Java

[language-server.rava-lsp]
command = "rava-lsp"
```

---

## Zed

Dans `~/.config/zed/settings.json` :

```json
{
  "file_types": { "Java": ["rava"] },
  "lsp": { "rava-lsp": { "binary": { "path": "rava-lsp" } } }
}
```

---

## Sublime Text

Coloration : la grammaire TextMate se charge telle quelle. Copiez
`editors/vscode/syntaxes/rava.tmLanguage.json` dans
`Packages/User/` sous le nom `Rava.tmLanguage.json`.

Diagnostics : greffon **LSP**, puis dans `LSP.sublime-settings` :

```json
{
  "clients": {
    "rava": {
      "enabled": true,
      "command": ["rava-lsp"],
      "selector": "source.rava"
    }
  }
}
```

---

## Emacs

```elisp
(add-to-list 'auto-mode-alist '("\\.rava\\'" . java-mode))

(with-eval-after-load 'eglot
  (add-to-list 'eglot-server-programs '(java-mode . ("rava-lsp"))))
```

Attention si vous éditez aussi du Java : la ligne ci-dessus détourne
`java-mode` en entier. Préférez alors un mode dérivé :

```elisp
(define-derived-mode rava-mode java-mode "Rava")
(add-to-list 'auto-mode-alist '("\\.rava\\'" . rava-mode))
(with-eval-after-load 'eglot
  (add-to-list 'eglot-server-programs '(rava-mode . ("rava-lsp"))))
```

---

## Ce que le serveur sait faire

| Fonction | Détail |
|---|---|
| **Diagnostics — frappe** | erreurs de syntaxe et de génération, avec la note qui dit quoi écrire à la place. Republiés à chaque frappe. |
| **Diagnostics — enregistrement** | `rustc` est invoqué sur le Rust généré, et ses erreurs sont **ramenées sur le `.rava`** : emprunt, durées de vie, typage, avec les positions liées. Réglage `checkOnSave`. |
| **Survol** | sur une annotation, un mot-clé, une façade ou un type : ce que la construction devient en Rust, et pourquoi. Y compris sur `null`, `try`, `instanceof` — le survol explique le refus. |
| **Complétion** | annotations après `@`, membres après `Macro.` / `Ref.` / `Range.` / `Arr.` …, mots-clés, types Rust, et six fragments (classe, `main`, filtrage d'un `Result`, d'une `Option`, boucle sur emprunt, méthode d'interface). |
| **Plan du fichier** | classes, records, interfaces, enums, avec champs, constantes, variantes, types associés et méthodes. |
| **Corrections rapides** | `null` → `None` · `case X:` → `case X ->` · retirer `synchronized`. |

Une erreur d'emprunt se lit intégralement dans le `.rava` :

```
erreur[E0502]: cannot borrow `noms` as mutable because it is also borrowed as immutable
  --> Emprunt.rava:9:9
  |
9 |         noms.push("carole".to_string());
  |         ^
  = mutable borrow occurs here
  = ligne 8 : immutable borrow occurs here
  = ligne 11 : immutable borrow later used here
```

Ce qu'il ne fait **pas** encore : renommage, aller à la définition et
formatage.

---

## GitHub

Le dépôt contient un `.gitattributes` qui déclare `*.rava` comme du Java :
les fichiers sont colorés dans les diffs et les vues de fichier, et comptés
comme tels dans les statistiques du dépôt.
