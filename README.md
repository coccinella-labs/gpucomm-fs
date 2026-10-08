<p align="center">
  <img src="https://raw.githubusercontent.com/coccinella-labs/gpucomm-fs/main/.github/assets/thumbnail.png" alt="gpucomm-fs" width="100%">
</p>

# GPUComm-FS

A content-addressed store for GPU artifacts: model weights, kernels, datasets, benchmark bundles.

Objects are addressed by their Blake3 hash, so identical files are stored once. Every retrieval
is verified against the hash it was filed under, and repeated metadata keys accumulate instead of
overwriting. Written in Rust, roughly 450 lines in a single file, no runtime dependencies.

Status: v0.1.0. The store format and CLI are stable. FUSE mounting is not implemented.

## Do you need this?

**Use it** when you have a few large files that are written once and read many times, you want
copies of the same weights file to cost nothing extra, and you would rather know a file is
intact than find out later.

**Skip it** for anything else. This is not a filesystem, not a backup tool, and not fast enough
to be one. Specifically:

- No compression. Objects are stored at full size.
- No cross-store deduplication. Two stores share nothing.
- No metadata search. `ls` prints hashes only, and you filter yourself.
- No repair. `verify` detects corruption, nothing rebuilds it.
- No partial or resumable uploads.

If you need any of those, use an artifact store built for it.

## Quickstart

```bash
git clone https://github.com/coccinella-labs/gpucomm-fs
cd gpucomm-fs
cargo build --release
```

Initialize a store, then add a file:

```bash
gpucomm-fs init .gpucomm-fs
hash=$(gpucomm-fs put .gpucomm-fs weights.bin --meta kind=weights --meta cuda=12.1 | tail -1)
```

Putting the same file twice returns the same hash and writes no second copy:

```bash
gpucomm-fs put .gpucomm-fs weights.bin
gpucomm-fs ls .gpucomm-fs
```

Retrieve it, then check the whole store:

```bash
gpucomm-fs get .gpucomm-fs "$hash" restored.bin
gpucomm-fs verify .gpucomm-fs
```

Every command works from `cargo run -- <command>` if you have not built the binary.

## Commands

| Command | Purpose |
|---|---|
| `init <store>` | create the store layout |
| `put <store> <file> [--meta key=value]...` | store a file, print its hash |
| `ls <store>` | print one hash per line |
| `get <store> <hash> <out>` | verified retrieval |
| `verify <store>` | re-hash every object, cross-check metadata |

All of them exit non-zero on failure. `get` and `verify` refuse to write output for an object
that fails verification.

## How storage works

```
.gpucomm-fs/
  objects/<hh>/<hash>     raw file bytes, 64 hex chars, <hh> is the first two
  meta/<hash>.json        size, hash, and your metadata
```

Objects are immutable and untransformed. Two files with identical bytes produce the same hash
and therefore the same path, so the second `put` is a no-op. Sharding on the first two hex
characters spreads objects across up to 256 directories instead of one.

Metadata is a separate file, so tags can be added without touching the object. A record looks
like this:

```json
{
  "hash": "8b6351e283842383c54d03811bd9900b68cf0505efff3c6e21756a3e5eb6daf1",
  "size_bytes": 2048,
  "meta": { "cuda": ["12.1"], "kind": ["weights"] }
}
```

Values are lists by design. `--meta tag=v1 --meta tag=v2` records `["v1", "v2"]`, and re-putting
the same content merges into the existing record instead of dropping earlier keys. Keys are not
validated; useful conventions are `kind` (weights, dataset, kernel, benchmark), `framework`,
`cuda`, `arch` (sm_80), and `split` (train, val, test).

## Integrity

`get` re-hashes the stored bytes and compares them to the requested hash before writing
anything, so a corrupted object is reported and no output file is produced:

```console
$ gpucomm-fs get .gpucomm-fs 8b63... out.bin
Error: "integrity check failed for 8b63...: stored object hashes to ffc3..."
$ echo $?
1
```

`verify` applies the same check to the whole store and also cross-checks each metadata record
against the object it describes, so a missing sidecar file or a stale `size_bytes` is caught:

```console
$ gpucomm-fs verify .gpucomm-fs
ok 83f42badff9e601f7267ef3902940398760fe22a44887d4d05a3f05408bea7ba
FAILED d5b33ecc...: integrity check failed: stored object hashes to 55d4838...
2 checked, 1 failed
```

Recovery is manual. Re-put a good copy of the artifact to restore it.

## Measured performance

One 1 GiB file, one store, on the machine this was last tested on. Absolute numbers are
disk-dependent; the useful part is the ratio between operations.

| Operation | Time | Effective rate |
|---|---|---|
| `put` (hash + write) | 0.89 s | 1.12 GiB/s |
| `get` (verify + write) | 1.57 s | 0.64 GiB/s |
| `verify` (re-hash) | 1.06 s | 0.94 GiB/s |
| `ls` (single object) | 4 ms | n/a |

For reference, `cp` of the same file took 1.19 s on this disk. The store is I/O bound, not
hash bound, so throughput tracks the underlying disk rather than Blake3.

`ls` walks the object directories, so it grows with the number of objects and stays fast enough
for stores in the low thousands of files. Metadata is never read by `ls`.

## Development

```bash
cargo build --release
cargo test          # unit and README-drift tests, no network required
cargo fmt --all
```

Hooks run on commit and push if you have `pre-commit install` set up. They check formatting, run
the test suite, and refuse `.DS_Store` and `.pem` files.

`tests/readme.rs` asserts that this file stays in step with the code: every command is
documented, and the known-false claims that previously shipped here cannot come back. It runs as
part of `cargo test`, so a README that drifts from the implementation fails CI.

## Contributing

Open an issue first for anything that changes the store format, since the object layout and
metadata schema are compatibility surfaces. For changes that do not, a branch and a pull
request are enough.

New commands go next to the existing handlers in `src/main.rs`. Errors are returned as
`Result<_, String>` with a message that names the failing path; there is no error framework in
use and adding one is a separate decision.

## License

MIT. See [LICENSE](LICENSE).