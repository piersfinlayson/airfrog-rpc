# Release Process

Update version number:
- [`Cargo.toml`](Cargo.toml)

Publish dry-run

```bash
cargo publish --dry-run
```

Publish

```bash
cargo publish
```

Tag the version in git:

```bash
git tag -s -a v<x.y.z> -m "Release v<x.y.z>"
git push origin v<x.y.z>
```
