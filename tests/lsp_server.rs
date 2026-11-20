use ae_compiler::lsp::{JsonValue, LineIndex, LspServer, Position};

#[test]
fn lsp_initialize_and_capabilities() {
    let mut server = LspServer::new();
    let init_req = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#;
    let resps = server.handle_message(init_req);
    assert_eq!(resps.len(), 1);
    let val = JsonValue::parse(&resps[0]).unwrap();
    assert_eq!(val.get("id").and_then(|v| v.as_i64()), Some(1));
    let caps = val.get("result").and_then(|r| r.get("capabilities")).unwrap();
    assert_eq!(caps.get("textDocumentSync").and_then(|v| v.as_i64()), Some(1));
    assert_eq!(caps.get("hoverProvider"), Some(&JsonValue::Bool(true)));
    assert_eq!(caps.get("definitionProvider"), Some(&JsonValue::Bool(true)));
}

#[test]
fn lsp_text_sync_with_multibyte_and_diagnostics() {
    let mut server = LspServer::new();
    let open_notif = r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///test.ae","version":1,"text":"fn main() {\n    let s = \"你好世界\";\n    print(s);\n}"}}}"#;
    let resps = server.handle_message(open_notif);
    assert_eq!(resps.len(), 1);
    let val = JsonValue::parse(&resps[0]).unwrap();
    assert_eq!(val.get("method").and_then(|m| m.as_str()), Some("textDocument/publishDiagnostics"));

    let change_invalid = r#"{"jsonrpc":"2.0","method":"textDocument/didChange","params":{"textDocument":{"uri":"file:///test.ae","version":2},"contentChanges":[{"text":"fn main() {\n    let s = \"bad escape \\q\";\n}"}]}}"#;
    let resps2 = server.handle_message(change_invalid);
    assert_eq!(resps2.len(), 1);
    let val2 = JsonValue::parse(&resps2[0]).unwrap();
    let diags = val2.get("params").and_then(|p| p.get("diagnostics")).and_then(|d| d.as_array()).unwrap();
    assert!(!diags.is_empty());
}

#[test]
fn lsp_line_index_multibyte_offset_conversion() {
    let text = "a\n你好\nb";
    let index = LineIndex::new(text);
    let pos = index.offset_to_position(text, 5);
    assert_eq!(pos, Position { line: 1, character: 1 });
    let off = index.position_to_offset(text, Position { line: 1, character: 2 });
    assert_eq!(off, 8);
}

#[test]
fn lsp_hover_and_builtins() {
    let mut server = LspServer::new();
    let open = r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///test.ae","version":1,"text":"fn main() {\n    let ad = adapter_new();\n}"}}}"#;
    server.handle_message(open);

    let hover_req = r#"{"jsonrpc":"2.0","id":2,"method":"textDocument/hover","params":{"textDocument":{"uri":"file:///test.ae"},"position":{"line":1,"character":15}}}"#;
    let resps = server.handle_message(hover_req);
    assert_eq!(resps.len(), 1);
    let val = JsonValue::parse(&resps[0]).unwrap();
    let hover_val = val.get("result").and_then(|r| r.get("contents")).and_then(|c| c.get("value")).and_then(|v| v.as_str()).unwrap();
    assert!(hover_val.contains("adapter_new"));
}

#[test]
fn lsp_definition_jumping() {
    let mut server = LspServer::new();
    let open = r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///test.ae","version":1,"text":"fn compute() -> i64 { return 42; }\nfn main() {\n    print(compute());\n}"}}}"#;
    server.handle_message(open);

    let def_req = r#"{"jsonrpc":"2.0","id":3,"method":"textDocument/definition","params":{"textDocument":{"uri":"file:///test.ae"},"position":{"line":2,"character":12}}}"#;
    let resps = server.handle_message(def_req);
    assert_eq!(resps.len(), 1);
    let val = JsonValue::parse(&resps[0]).unwrap();
    let res = val.get("result").unwrap();
    let uri = res.get("uri").and_then(|u| u.as_str()).unwrap();
    assert_eq!(uri, "file:///test.ae");
    let line = res.get("range").and_then(|r| r.get("start")).and_then(|s| s.get("line")).and_then(|l| l.as_i64()).unwrap();
    assert_eq!(line, 0);
}

#[test]
fn lsp_completion_trigger_and_builtins() {
    let mut server = LspServer::new();
    let open = r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///test.ae","version":1,"text":"struct Point { x: i64, y: i64 }\nfn main() {\n    let p = Point { x: 1, y: 2 };\n    p.\n}"}}}"#;
    server.handle_message(open);

    let comp_req = r#"{"jsonrpc":"2.0","id":4,"method":"textDocument/completion","params":{"textDocument":{"uri":"file:///test.ae"},"position":{"line":3,"character":6}}}"#;
    let resps = server.handle_message(comp_req);
    assert_eq!(resps.len(), 1);
    let val = JsonValue::parse(&resps[0]).unwrap();
    let items = val.get("result").and_then(|r| r.as_array()).unwrap();
    let labels: Vec<&str> = items.iter().filter_map(|i| i.get("label").and_then(|l| l.as_str())).collect();
    assert!(labels.contains(&"x") || labels.contains(&"y"));
}

#[test]
fn lsp_rename_symbol_across_file() {
    let mut server = LspServer::new();
    let open = r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///test.ae","version":1,"text":"fn main() {\n    let counter = 10;\n    print(counter);\n}"}}}"#;
    server.handle_message(open);

    let rename_req = r#"{"jsonrpc":"2.0","id":5,"method":"textDocument/rename","params":{"textDocument":{"uri":"file:///test.ae"},"position":{"line":1,"character":10},"newName":"total"}}"#;
    let resps = server.handle_message(rename_req);
    assert_eq!(resps.len(), 1);
    let val = JsonValue::parse(&resps[0]).unwrap();
    let changes = val.get("result").and_then(|r| r.get("changes")).unwrap();
    let edits = changes.get("file:///test.ae").and_then(|e| e.as_array()).unwrap();
    assert_eq!(edits.len(), 2);
}

#[test]
fn lsp_cancel_stale_request() {
    let mut server = LspServer::new();
    let cancel = r#"{"jsonrpc":"2.0","method":"$/cancelRequest","params":{"id":99}}"#;
    server.handle_message(cancel);

    let req = r#"{"jsonrpc":"2.0","id":99,"method":"initialize","params":{}}"#;
    let resps = server.handle_message(req);
    assert_eq!(resps.len(), 1);
    let val = JsonValue::parse(&resps[0]).unwrap();
    let err_code = val.get("error").and_then(|e| e.get("code")).and_then(|c| c.as_i64()).unwrap();
    assert_eq!(err_code, -32800);
}
