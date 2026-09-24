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

---

# Siskin 패키지 목록

이 저장소가 Siskin 패키지 목록입니다. 서버는 없습니다. `siskin add 이름` 이 이 저장소를
(`~/.siskin/registry/` 에) 받아서 `packages/이름.toml` 을 찾습니다.

**올리는 법:** 내 패키지를 GitHub 에 올리고(뿌리에 `siskin.toml` 과 `lib.skn`), 그 폴더에서 `siskin publish` 를 치면
여기에 더할 파일 내용이 나옵니다. 그 파일 하나를 더하는 PR 을 보내 주세요. 새 판은 내 저장소에 git 태그만 올리면 됩니다.
