# casc-rs

Reads the game's files from a local CASC storage, as Battle.net installs
Warcraft III Reforged. Nothing is written.

```rust
let s = casc_rs::Storage::open(Path::new("/Applications/Warcraft III"))?;
let footman = s.read("War3.w3mod:Units/Human/Footman/Footman.mdx")?;
let hd = s.read("War3.w3mod:_HD.w3mod:Units/Human/Footman/Footman.mdx")?;
```

## What it reads

| Part | Where | What's taken |
| --- | --- | --- |
| `.build.info` | the game's folder | a table, `\|` between cells: the active row's `Build Key` |
| the build's configuration | `Data/config/ab/cd/<key>` | `encoding` (its second key: encoded), `root` (a content key), `build-name` |
| the local indices | `Data/data/BBVVVVVVVV.idx` | each bucket's highest version; an entry: an encoded key's first 9 bytes, where its data is (5 bytes big-endian: the archive above the offset's bits), its size |
| an entry's data | `Data/data/data.NNN` | a 30-byte header, then BLTE |
| BLTE | | chunks raw (`N`) or zlib (`Z`); others (encrypted `E`, frames, LZ4) said, not read |
| the encoding file | | each content key's encoded key |
| the root | | Warcraft III's: text, a line a file, `name\|content key\|..` |

Names are the root's: `War3.w3mod:` the game's files, a mod's under it
(`War3.w3mod:_HD.w3mod:` the HD ones, `War3.w3mod:_Locales/ruRU.w3mod:`,
`War3.w3mod:_Tilesets/L.w3mod:` ...). They're looked up in any case,
with `\` or `/`. The root names every locale; only what's installed has
its data on disk (`Storage::has`).

## The command

```
casc-rs <game folder>                 the build, how many files
casc-rs <game folder> list [text]     the names (with text in them), * when on disk
casc-rs <game folder> read <name> [file]
```

## Tests

`cargo test`; with a Reforged install, `CASC_GAME=<its folder> cargo
test` reads a few of its files, and `cargo test -- --ignored` every
file on disk (3.0.1: 93777 of 175345 named, none failing).
