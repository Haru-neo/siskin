# Using Siskin in your editor

Siskin ships with a language server for editors: `siskin lsp`.
When your editor runs it in the background, you get:

- Error squiggles (the same checks as `siskin check`, on every keystroke)
- Format Document = `siskin fmt`
- Go to definition, hover documentation, outline (table of contents), name completion

## VS Code

Install `vscode/siskin-0.1.0.vsix`: in the Extensions view, click `…` at the top right → "Install from VSIX...".
Syntax highlighting and a ▷ run button are included too. See [vscode/README.md](vscode/README.md) for details.

To rebuild it: `cd vscode && npm install && npx @vscode/vsce package`

## Neovim (0.11 or later)

In `init.lua`:

```lua
vim.filetype.add({ extension = { skn = "siskin" } })
vim.lsp.config("siskin", { cmd = { "siskin", "lsp" }, filetypes = { "siskin" }, root_markers = { "siskin.toml", ".git" } })
vim.lsp.enable("siskin")
```

## Helix

In `~/.config/helix/languages.toml`:

```toml
[language-server.siskin]
command = "siskin"
args = ["lsp"]

[[language]]
name = "siskin"
scope = "source.siskin"
file-types = ["skn"]
comment-token = "#"
indent = { tab-width = 4, unit = "    " }
language-servers = ["siskin"]
```

## Other editors

Wherever the editor asks for an "LSP server command", enter `siskin lsp`. The file extension is `.skn`.
