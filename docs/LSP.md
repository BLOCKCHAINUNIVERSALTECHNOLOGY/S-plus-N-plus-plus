# S+N++ LSP (v0.4)

## Start

```bash
snp lsp
```

Speaks **Language Server Protocol** over **stdio** (Content-Length framing).

## Capabilities

| Feature | Support |
|---------|---------|
| `initialize` | yes |
| `textDocument/didOpen` / `didChange` / `didClose` | full sync |
| `textDocument/hover` | keywords, builtins, identifiers |
| `textDocument/publishDiagnostics` | parse + sema errors |

## VS Code / editor

Point the editor's LSP client to the `snp` binary with args `["lsp"]`.

Example (generic):

```json
{
  "command": "snp",
  "args": ["lsp"],
  "filetypes": ["snp"]
}
```

No extra crates — pure Rust std.
