# Siskin for VS Code

An extension for Siskin (`.skn`) files.

- **Syntax highlighting**: keywords, strings (including `{expr}` inside f-strings), numbers, comments, types, function names
- **Error squiggles**: runs the same checks as `siskin check` as you type. Hover over a squiggle to see help
- **Formatting**: "Format Document" (Shift+Alt+F) formats with `siskin fmt`
- **Go to Definition** (F12), **hover documentation**, **outline view**, **name completion**
- **Run button**: the ▷ at the top right of the editor runs `siskin run` in a terminal

## Requirements

The `siskin` program must be on your PATH. If it is somewhere else, set its path in the `siskin.path` setting.

## Installation

Download `siskin-0.1.0.vsix`, then in VS Code's Extensions view click `…` at the top right → "Install from VSIX..." and select it.
Or from a terminal: `code --install-extension siskin-0.1.0.vsix`.
