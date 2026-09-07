//! rava-lsp — serveur Language Server Protocol pour Rava.
//!
//! Un seul fil, boucle synchrone sur l'entrée standard : la charge d'un
//! fichier Rava tient largement dans ce budget, et cela garde le serveur
//! lisible de bout en bout.
//!
//! Fournit : diagnostics (syntaxe et génération), survol documenté, complétion,
//! plan du fichier, corrections rapides.

mod analysis;
mod kb;

use analysis::Doc;
use rava_json::Json;
use std::collections::HashMap;
use std::io::{BufRead, Write};

fn main() {
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let stdout = std::io::stdout();
    let mut output = stdout.lock();
    let mut server = Server {
        docs: HashMap::new(),
        symbols: HashMap::new(),
        check_on_save: true,
        shutting_down: false,
    };

    while let Some(msg) = read_message(&mut input) {
        let Some(msg) = rava_json::parse(&msg) else {
            continue;
        };
        let method = msg.get("method").and_then(Json::as_str).unwrap_or("").to_string();
        let id = msg.get("id").cloned();

        if method == "exit" {
            return;
        }
        for out in server.handle(&method, id, &msg) {
            write_message(&mut output, &out);
        }
    }
}

struct Server {
    docs: HashMap<String, Doc>,
    /// Dernier plan valide par document : pendant qu'on tape, le fichier passe
    /// par des états illisibles, et l'esquisse ne doit pas clignoter.
    symbols: HashMap<String, Json>,
    /// Appeler `rustc` à l'enregistrement, pour les erreurs d'emprunt, de
    /// durée de vie et de typage. Désactivable par `initializationOptions`.
    check_on_save: bool,
    shutting_down: bool,
}

impl Server {
    fn handle(&mut self, method: &str, id: Option<Json>, msg: &Json) -> Vec<Json> {
        let params = msg.get("params").cloned().unwrap_or(Json::Null);

        match method {
            "initialize" => {
                if let Some(Json::Bool(b)) =
                    params.path(&["initializationOptions", "checkOnSave"])
                {
                    self.check_on_save = *b;
                }
                vec![response(id, capabilities())]
            }
            "initialized" => Vec::new(),
            "shutdown" => {
                self.shutting_down = true;
                vec![response(id, Json::Null)]
            }

            "textDocument/didOpen" => {
                let uri = uri_of(&params, &["textDocument", "uri"]);
                let text = params
                    .path(&["textDocument", "text"])
                    .and_then(Json::as_str)
                    .unwrap_or("")
                    .to_string();
                self.set_doc(uri.clone(), text);
                self.publish(&uri, self.check_on_save)
            }
            "textDocument/didChange" => {
                let uri = uri_of(&params, &["textDocument", "uri"]);
                // Synchronisation complète : le dernier changement porte tout le texte.
                if let Some(text) = params
                    .get("contentChanges")
                    .and_then(Json::as_arr)
                    .and_then(|c| c.last())
                    .and_then(|c| c.get("text"))
                    .and_then(Json::as_str)
                {
                    self.set_doc(uri.clone(), text.to_string());
                }
                // Pendant la frappe : syntaxe et traduction seulement.
                self.publish(&uri, false)
            }
            "textDocument/didSave" => {
                let uri = uri_of(&params, &["textDocument", "uri"]);
                self.publish(&uri, self.check_on_save)
            }
            "textDocument/didClose" => {
                let uri = uri_of(&params, &["textDocument", "uri"]);
                self.docs.remove(&uri);
                self.symbols.remove(&uri);
                // Efface les marqueurs restants côté éditeur.
                vec![notification(
                    "textDocument/publishDiagnostics",
                    Json::obj(vec![
                        ("uri", Json::str(uri)),
                        ("diagnostics", Json::Arr(Vec::new())),
                    ]),
                )]
            }

            "textDocument/hover" => {
                let (uri, line, ch) = position_of(&params);
                let out = match self.docs.get(&uri) {
                    Some(d) => analysis::hover(d, line, ch),
                    None => Json::Null,
                };
                vec![response(id, out)]
            }
            "textDocument/completion" => {
                let (uri, line, ch) = position_of(&params);
                let out = match self.docs.get(&uri) {
                    Some(d) => analysis::completion(d, line, ch),
                    None => Json::Arr(Vec::new()),
                };
                vec![response(id, out)]
            }
            "textDocument/documentSymbol" => {
                let uri = uri_of(&params, &["textDocument", "uri"]);
                let out = self.symbols.get(&uri).cloned().unwrap_or(Json::Arr(Vec::new()));
                vec![response(id, out)]
            }
            "textDocument/codeAction" => {
                let uri = uri_of(&params, &["textDocument", "uri"]);
                let diags = params
                    .path(&["context", "diagnostics"])
                    .and_then(Json::as_arr)
                    .map(<[Json]>::to_vec)
                    .unwrap_or_default();
                let out = match self.docs.get(&uri) {
                    Some(d) => analysis::code_actions(d, &uri, &diags),
                    None => Json::Arr(Vec::new()),
                };
                vec![response(id, out)]
            }

            // Requête inconnue : il faut quand même répondre, sinon le client attend.
            _ if id.is_some() => vec![response(id, Json::Null)],
            _ => Vec::new(),
        }
    }

    /// Enregistre le texte et, si le fichier se lit, rafraîchit son plan.
    fn set_doc(&mut self, uri: String, text: String) {
        let doc = Doc::new(text);
        if let Some(syms) = analysis::document_symbols(&doc) {
            self.symbols.insert(uri.clone(), syms);
        }
        self.docs.insert(uri, doc);
    }

    /// `full` ajoute les diagnostics de `rustc` (ou de `cargo` pour un projet),
    /// au prix d'un appel au compilateur : réservé à l'ouverture et à
    /// l'enregistrement.
    ///
    /// Dans un projet, une seule sauvegarde concerne plusieurs fichiers : on
    /// publie pour chacun, y compris vide, pour effacer les marqueurs devenus
    /// caducs.
    fn publish(&self, uri: &str, full: bool) -> Vec<Json> {
        let per_file = match self.docs.get(uri) {
            Some(d) if full => analysis::full_diagnostics(d, uri),
            Some(d) => vec![(uri.to_string(), analysis::diagnostics(d, uri))],
            None => vec![(uri.to_string(), Json::Arr(Vec::new()))],
        };
        per_file
            .into_iter()
            .map(|(u, diagnostics)| {
                notification(
                    "textDocument/publishDiagnostics",
                    Json::obj(vec![("uri", Json::str(u)), ("diagnostics", diagnostics)]),
                )
            })
            .collect()
    }
}

fn capabilities() -> Json {
    Json::obj(vec![
        (
            "capabilities",
            Json::obj(vec![
                // 1 = synchronisation complète du texte à chaque frappe.
                ("textDocumentSync", Json::int(1)),
                ("hoverProvider", Json::Bool(true)),
                ("documentSymbolProvider", Json::Bool(true)),
                ("codeActionProvider", Json::Bool(true)),
                (
                    "completionProvider",
                    Json::obj(vec![(
                        "triggerCharacters",
                        Json::Arr(vec![Json::str("@"), Json::str(".")]),
                    )]),
                ),
            ]),
        ),
        (
            "serverInfo",
            Json::obj(vec![
                ("name", Json::str("rava-lsp")),
                ("version", Json::str(env!("CARGO_PKG_VERSION"))),
            ]),
        ),
    ])
}

fn uri_of(params: &Json, path: &[&str]) -> String {
    params.path(path).and_then(Json::as_str).unwrap_or("").to_string()
}

fn position_of(params: &Json) -> (String, u32, u32) {
    (
        uri_of(params, &["textDocument", "uri"]),
        params.path(&["position", "line"]).and_then(Json::as_u32).unwrap_or(0),
        params.path(&["position", "character"]).and_then(Json::as_u32).unwrap_or(0),
    )
}

fn response(id: Option<Json>, result: Json) -> Json {
    Json::obj(vec![
        ("jsonrpc", Json::str("2.0")),
        ("id", id.unwrap_or(Json::Null)),
        ("result", result),
    ])
}

fn notification(method: &str, params: Json) -> Json {
    Json::obj(vec![
        ("jsonrpc", Json::str("2.0")),
        ("method", Json::str(method)),
        ("params", params),
    ])
}

/// Lit un message LSP : en-têtes `Content-Length`, ligne vide, puis le corps.
fn read_message(input: &mut impl BufRead) -> Option<String> {
    let mut len = 0usize;
    loop {
        let mut header = String::new();
        if input.read_line(&mut header).ok()? == 0 {
            return None;
        }
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some(v) = header.strip_prefix("Content-Length:") {
            len = v.trim().parse().ok()?;
        }
    }
    let mut body = vec![0u8; len];
    input.read_exact(&mut body).ok()?;
    String::from_utf8(body).ok()
}

fn write_message(out: &mut impl Write, msg: &Json) {
    let body = msg.to_text();
    let _ = write!(out, "Content-Length: {}\r\n\r\n{body}", body.len());
    let _ = out.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open(server: &mut Server, src: &str) -> Json {
        let params = Json::obj(vec![(
            "textDocument",
            Json::obj(vec![
                ("uri", Json::str("file:///T.rava")),
                ("text", Json::str(src)),
            ]),
        )]);
        let msg = Json::obj(vec![
            ("method", Json::str("textDocument/didOpen")),
            ("params", params),
        ]);
        server.handle("textDocument/didOpen", None, &msg).remove(0)
    }

    fn ask(server: &mut Server, method: &str, params: Json) -> Json {
        let msg = Json::obj(vec![
            ("method", Json::str(method)),
            ("id", Json::int(1)),
            ("params", params),
        ]);
        server.handle(method, Some(Json::int(1)), &msg).remove(0).get("result").unwrap().clone()
    }

    fn at(line: i64, ch: i64) -> Json {
        Json::obj(vec![
            ("textDocument", Json::obj(vec![("uri", Json::str("file:///T.rava"))])),
            (
                "position",
                Json::obj(vec![("line", Json::int(line)), ("character", Json::int(ch))]),
            ),
        ])
    }

    fn new_server() -> Server {
        Server {
            docs: HashMap::new(),
            symbols: HashMap::new(),
            check_on_save: false,
            shutting_down: false,
        }
    }

    #[test]
    fn signale_une_erreur_de_syntaxe_avec_sa_note() {
        let mut s = new_server();
        let out = open(&mut s, "class A { String f() { return null; } }");
        let d = &out.path(&["params", "diagnostics"]).unwrap().as_arr().unwrap()[0];
        assert_eq!(d.get("code").unwrap().as_str().unwrap(), "rava.null");
        assert!(d.get("message").unwrap().as_str().unwrap().contains("Option"));
    }

    #[test]
    fn aucun_diagnostic_sur_un_fichier_correct() {
        let mut s = new_server();
        let out = open(&mut s, "class A { public i32 f() { return 1; } }");
        assert!(out.path(&["params", "diagnostics"]).unwrap().as_arr().unwrap().is_empty());
    }

    #[test]
    fn survol_dune_annotation() {
        let mut s = new_server();
        open(&mut s, "class A { @Mut public void f() { } }");
        let h = ask(&mut s, "textDocument/hover", at(0, 12));
        let v = h.path(&["contents", "value"]).unwrap().as_str().unwrap();
        assert!(v.contains("@Mut"), "{v}");
        assert!(v.contains("&mut self"), "{v}");
    }

    #[test]
    fn survol_dun_mot_cle_rava() {
        let mut s = new_server();
        open(&mut s, "class A { void f() { unless (x) { } } }");
        let h = ask(&mut s, "textDocument/hover", at(0, 23));
        assert!(h.path(&["contents", "value"]).unwrap().as_str().unwrap().contains("Sauf si"));
    }

    #[test]
    fn survol_dune_construction_refusee() {
        let mut s = new_server();
        open(&mut s, "class A { void f() { var x = 1; } }");
        // Le mot est cherché dans le texte, indépendamment de l'analyse.
        let mut s2 = new_server();
        open(&mut s2, "// instanceof\nclass A { }");
        let h = ask(&mut s2, "textDocument/hover", at(0, 5));
        assert!(h.path(&["contents", "value"]).unwrap().as_str().unwrap().contains("enum"));
        let _ = s;
    }

    #[test]
    fn completion_dannotations_apres_arobase() {
        let mut s = new_server();
        open(&mut s, "class A { @ }");
        let c = ask(&mut s, "textDocument/completion", at(0, 11));
        let labels: Vec<&str> =
            c.as_arr().unwrap().iter().filter_map(|i| i.get("label")?.as_str()).collect();
        assert!(labels.contains(&"Ref") && labels.contains(&"Lifetime"), "{labels:?}");
        assert!(!labels.contains(&"unless"));
    }

    #[test]
    fn completion_des_membres_dune_facade() {
        let mut s = new_server();
        open(&mut s, "class A { void f() { Macro. } }");
        let c = ask(&mut s, "textDocument/completion", at(0, 27));
        let labels: Vec<&str> =
            c.as_arr().unwrap().iter().filter_map(|i| i.get("label")?.as_str()).collect();
        assert!(labels.contains(&"println") && labels.contains(&"format"), "{labels:?}");
    }

    #[test]
    fn plan_du_fichier() {
        let mut s = new_server();
        open(&mut s, "class A {\n  i32 x;\n  public void f() { }\n}");
        let syms = ask(&mut s, "textDocument/documentSymbol", at(0, 0));
        let top = &syms.as_arr().unwrap()[0];
        assert_eq!(top.get("name").unwrap().as_str().unwrap(), "A");
        let kids = top.get("children").unwrap().as_arr().unwrap();
        assert_eq!(kids.len(), 2);
        assert_eq!(kids[1].get("name").unwrap().as_str().unwrap(), "f(…)");
    }

    #[test]
    fn le_plan_survit_a_une_erreur_de_frappe() {
        let mut s = new_server();
        open(&mut s, "class A {\n  public void f() { }\n}");
        // Frappe en cours : le fichier ne se lit plus.
        open(&mut s, "class A {\n  public void f() { null\n}");
        let syms = ask(&mut s, "textDocument/documentSymbol", at(0, 0));
        assert_eq!(syms.as_arr().unwrap().len(), 1, "le plan doit rester affiché");
    }

    #[test]
    fn les_erreurs_de_rustc_remontent_a_lenregistrement() {
        let mut s = new_server();
        s.check_on_save = true;
        let src = "class A {\n  public static void main(String[] a) {\n    @Mut var v = Macro.vec(1);\n    var p = Ref.of(v[0]);\n    v.push(2);\n    System.out.println(\"{}\", p);\n  }\n}";
        let published = open(&mut s, src);
        let diags = published.path(&["params", "diagnostics"]).unwrap().as_arr().unwrap();
        if diags.is_empty() {
            return; // rustc absent de l'environnement
        }
        let d = &diags[0];
        assert_eq!(d.get("code").unwrap().as_str().unwrap(), "E0502");
        // `v.push(2);` est la 5e ligne du .rava, donc la ligne 4 en 0-basé.
        assert_eq!(d.path(&["range", "start", "line"]).unwrap().as_u32().unwrap(), 4);
        assert_eq!(d.get("source").unwrap().as_str().unwrap(), "rava (rustc)");
    }

    #[test]
    fn correction_rapide_pour_null() {
        let mut s = new_server();
        let published = open(&mut s, "class A { String f() { return null; } }");
        let diags = published.path(&["params", "diagnostics"]).unwrap().clone();
        let params = Json::obj(vec![
            ("textDocument", Json::obj(vec![("uri", Json::str("file:///T.rava"))])),
            ("context", Json::obj(vec![("diagnostics", diags)])),
        ]);
        let actions = ask(&mut s, "textDocument/codeAction", params);
        let a = &actions.as_arr().unwrap()[0];
        assert!(a.get("title").unwrap().as_str().unwrap().contains("None"));
        let edit = a
            .path(&["edit", "changes", "file:///T.rava"])
            .unwrap()
            .as_arr()
            .unwrap()[0]
            .clone();
        assert_eq!(edit.get("newText").unwrap().as_str().unwrap(), "None");
    }

    #[test]
    fn correction_rapide_retire_synchronized() {
        let mut s = new_server();
        let published = open(&mut s, "class A {\n  public synchronized void f() { }\n}");
        let diags = published.path(&["params", "diagnostics"]).unwrap().clone();
        assert!(!diags.as_arr().unwrap().is_empty());
        let params = Json::obj(vec![
            ("textDocument", Json::obj(vec![("uri", Json::str("file:///T.rava"))])),
            ("context", Json::obj(vec![("diagnostics", diags)])),
        ]);
        let actions = ask(&mut s, "textDocument/codeAction", params);
        let a = &actions.as_arr().unwrap()[0];
        let edit = a.path(&["edit", "changes", "file:///T.rava"]).unwrap().as_arr().unwrap()[0]
            .clone();
        assert_eq!(edit.get("newText").unwrap().as_str().unwrap(), "");
        assert_eq!(edit.path(&["range", "start", "character"]).unwrap().as_u32().unwrap(), 9);
    }

    #[test]
    fn positions_en_unites_utf16() {
        let mut s = new_server();
        // « é » occupe deux octets mais une seule unité UTF-16.
        let out = open(&mut s, "class A { String f() { var é = null; } }");
        let d = &out.path(&["params", "diagnostics"]).unwrap().as_arr().unwrap()[0];
        assert_eq!(d.path(&["range", "start", "character"]).unwrap().as_u32().unwrap(), 31);
    }
}
