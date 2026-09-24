# Mica usability trial — rules for every participant

You are a test participant. Mica is a brand-new programming language (files end in `.mi`). You have never seen it
before. The goal is to measure how easy Mica is for an AI coder to pick up, and to collect honest feedback.
Friction you hit IS the result, so never hide it or smooth it over.

Rules
- Work only inside your own folder (given in your task). Write your program there.
- Run the compiler ONLY through the wrapper `./mica` in your folder (it logs every call; that log is the data).
  Commands: `./mica check f.mi`, `./mica run f.mi`, `./mica build f.mi -o prog` then `./prog`, `./mica test f.mi`, `./mica fmt --stdout f.mi`.
- Do NOT read the compiler source or anything under /mnt/project-files or /home/claude/build. Do not read other
  participants' folders. Only the documentation you are given in your task.
- Do not call any tool whose name starts with `mcp__hearthbot__`.
- When a compile/run fails, first try to fix it from the error message alone. Only then look things up in the
  docs, and note that you had to.
- Write idiomatic code the way the docs suggest, not a workaround-heavy minimal version. If a feature seems missing,
  note it and then work around it.
- When the program works with `./mica run`, also do `./mica build <file> -o prog && ./prog` and compare the output
  byte-for-byte (`diff`). Any difference, crash, hang, or wrong result is a compiler bug: report it with a minimal repro.
- Stop after the program is done, or after ~40 compiler calls, whichever first.

At the end, write `REPORT.md` in your folder with exactly these sections:
1. **Result** — did it work (run and build identical?), total compiler calls (count lines starting `===` in attempts.log),
   calls until first successful `run`.
2. **Error log** — one entry per failed attempt: the key line of the error message (verbatim, short), what you had
   written, what you guessed from other languages (Python/Rust/Go/C++...), whether the message alone let you fix it
   (yes / partly / no), and the fix.
3. **Wrong guesses** — syntax or behaviour you expected from other languages that turned out wrong.
4. **Doc gaps** — things you needed that the docs did not say, said unclearly, or said wrongly (quote the doc line).
5. **Bugs** — crashes, wrong output, run vs build differences, misleading error messages, with minimal repros (a few lines each).
6. **What worked well** — honest, specific.
7. **Top 3 suggestions** — the changes that would most have helped you, most important first.
Keep REPORT.md under ~150 lines. Your final message back should be a 10-line summary of REPORT.md.
