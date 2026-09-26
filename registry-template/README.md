# Siskin package registry

This repository is the list of Siskin packages. There is no server: `siskin add <name>` clones this
repository (into `~/.siskin/registry/`) and looks up `packages/<name>.toml`.
This is the same approach as Rust's crates.io-index and Homebrew taps.

## Adding a package

1. Put your package on GitHub (or any git host). Its root needs `siskin.toml` and `lib.skn`.
2. Run `siskin publish` in your package folder. It prints the one file to add here.
3. Open a pull request that adds `packages/<name>.toml`:

```toml
git = "https://github.com/you/siskin-colors"
description = "Paint terminal text with colors"
```

Rules checked on review:

- The file name is the package name: letters, digits and `_` only, not a keyword or a `std` module name.
- First come, first served. A name is not transferred without the current owner's consent.
- New versions need no change here: push a git tag to your own repository.
