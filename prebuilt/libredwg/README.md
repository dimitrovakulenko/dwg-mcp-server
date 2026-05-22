# LibreDWG Prebuilt Bundles

CI and Docker builds use Git LFS archives from this directory instead of
rebuilding LibreDWG from source.

Expected archive name:

```text
<rust-target-triple>.tar.gz
```

The first CI target is:

```text
x86_64-unknown-linux-gnu.tar.gz
```

Create or refresh a bundle after building LibreDWG for the target platform:

```bash
bash scripts/pack-libredwg-prebuilt.sh x86_64-unknown-linux-gnu
```

Each archive extracts to a LibreDWG root containing:

- `src/.libs/libredwg.a`
- generated `src/*.h`
- `src/classes.c`
- `src/dynapi.c`
- public `include/*.h`

