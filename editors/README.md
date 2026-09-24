# 에디터에서 Siskin 쓰기

Siskin 은 에디터용 언어 서버를 가지고 있습니다: `siskin lsp`.
에디터가 이 프로그램을 띄워 두면 다음이 됩니다.

- 오류 밑줄 (칠 때마다 `siskin check` 와 같은 검사)
- 문서 서식 = `siskin fmt`
- 정의로 이동, 마우스 올리면 설명, 개요(목차), 이름 자동 완성

## VS Code

`vscode/siskin-0.1.0.vsix` 를 설치합니다. 확장 창 오른쪽 위 `…` → "VSIX에서 설치...".
색칠과 ▷ 실행 버튼도 같이 들어 있습니다. 자세한 것은 [vscode/README.md](vscode/README.md).

다시 만들려면: `cd vscode && npm install && npx @vscode/vsce package`

## Neovim (0.11 이상)

`init.lua` 에:

```lua
vim.filetype.add({ extension = { skn = "siskin" } })
vim.lsp.config("siskin", { cmd = { "siskin", "lsp" }, filetypes = { "siskin" }, root_markers = { "siskin.toml", ".git" } })
vim.lsp.enable("siskin")
```

## Helix

`~/.config/helix/languages.toml` 에:

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

## 다른 에디터

"LSP 서버 명령" 을 적는 칸에 `siskin lsp` 를 적으면 됩니다. 파일 확장자는 `.skn` 입니다.
