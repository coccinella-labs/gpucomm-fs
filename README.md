<p align="center">
  <img src="https://raw.githubusercontent.com/coccinella-labs/gpucomm-fs/main/.github/assets/thumbnail.png" alt="gpucomm-fs" width="100%">
</p>

# GPUComm-FS

GPUComm-FS is a content-addressed artifact store designed for GPU-related binaries and datasets. It provides deduplication, integrity verification, and searchable metadata for large files like model weights, CUDA kernels, datasets, and benchmark bundles. The store uses Blake3 content hashing to ensure binary integrity and enable efficient deduplication across multiple copies of the same file, while a metadata layer allows artifacts to be tagged with properties like CUDA version, framework, or dataset split.

Status: v0. Minimal CAS and CLI stable. Future work includes FUSE mounting and higher-level abstractions.

## Getting Started

GPUComm-FS requires Rust 1.70+ and is built with `cargo build --release`. The binary provides a command-line interface for initializing stores, adding artifacts, and retrieving them by content hash.

To create a new store, run `cargo run -- init .gpucomm-fs`. This creates the directory structure at `.gpucomm-fs` with subdirectories for objects and metadata. To add a file, run `cargo run -- put .gpucomm-fs path/to/weights.bin --meta kind=weights --meta cuda=12.1`. The command hashes the file, stores it under the content hash, and saves metadata as JSON. To list artifacts in the store, run `cargo run -- ls .gpucomm-fs`. To retrieve a file by its hash, run `cargo run -- get .gpucomm-fs <hash> output.bin`.

Metadata is optional but recommended for discoverability. When adding files, pass `--meta key=value` pairs to tag artifacts with provenance information. Multiple values for the same key can be added by repeating the flag. When listing, the CLI shows all stored artifacts with their hashes and metadata.

For development, ensure pre-commit hooks are installed with `pre-commit install` and run them before committing.

## Architecture

GPUComm-FS uses a simple content-addressed design. Every file is hashed with Blake3, producing a 32-character hex string. Objects are stored at `.gpucomm-fs/objects/<hh>/<hash>`, where `<hh>` is the first two characters of the hash (for filesystem sharding). Metadata is stored separately at `.gpucomm-fs/meta/<hash>.json`, containing user-provided key-value pairs plus computed properties like file size, hash, and addition timestamp.

This architecture has several benefits. Deduplication is automatic: if two users add the same file, both hashes compute identically, and the second add is skipped if the object already exists. Integrity verification is built-in: a hash mismatch indicates corruption. Metadata is independent of storage, so you can update tags without re-storing the file. The sharded directory structure (`objects/<hh>/`) prevents filesystem slowdown from too many files in a single directory.

Key code anchors are `src/store.rs` (store initialization and object/metadata management), `src/cli.rs` (command-line interface), and `src/hash.rs` (Blake3 hashing and serialization).

## Data Organization

Objects are stored immutably at `.gpucomm-fs/objects/<hh>/<hash>`. The `<hh>` prefix is the first two characters of the hash, creating up to 256 subdirectories. Each object is the raw binary data of the file; no compression or transformation is applied. Metadata is stored as JSON at `.gpucomm-fs/meta/<hash>.json` with fields for user-provided tags, computed size, timestamp, and the hash itself.

A typical metadata file looks like this: `{"hash":"abc123...","size":1073741824,"created_at":"2026-10-01T12:34:56Z","kind":"weights","framework":"torch","cuda":"12.1"}`. Tags are arbitrary strings; the store does not validate them. When retrieving an artifact, you need only the hash; metadata is fetched separately if needed for discovery.

## CLI Usage

The CLI provides four main commands. `init <store-path>` creates a new store. `put <store-path> <file> [--meta key=value]...` adds a file to the store and returns its hash. `ls <store-path>` lists all artifacts with their hashes and metadata. `get <store-path> <hash> <output-path>` retrieves a file by hash and writes it to the output path. Help is available with `--help` on any command.

The `put` command prints the hash of the added file. Capture this hash for later retrieval: `hash=$(cargo run -- put .gpucomm-fs model.bin --meta kind=weights | tail -1)`. The `ls` command outputs metadata as JSON (with `--format json`) or plain text. The `get` command fails with an error if the hash does not exist in the store.

## Deduplication and Integrity

Deduplication works automatically because two identical files always hash to the same value. If you `put` the same file twice, the second operation detects that the hash already exists, skips the copy, and returns the same hash. This saves space when multiple users or projects reference the same dataset or weights file. To verify integrity, compute the hash of a retrieved file and compare it against the stored hash. The CLI does not yet provide a `verify` command, but you can use `blake3` directly: `blake3 output.bin | grep <hash>`.

If a file is corrupted on disk, its hash will change, making corruption detectable. The store does not automatically validate hashes on retrieval; you must check the hash manually if you want verification. A future version may add automatic integrity checks and repair strategies.

## Adding Metadata

Metadata is stored in JSON and is independent of the object. When adding a file, pass `--meta key=value` flags to tag it. Multiple tags can be added: `put .gpucomm-fs weights.bin --meta kind=weights --meta framework=torch --meta cuda=12.1 --meta size=large`. All metadata is stored as strings; the store does not parse or validate them.

Common metadata keys for GPU artifacts include `kind` (weights, dataset, kernel, benchmark), `framework` (torch, tensorflow, jax), `cuda` (CUDA version), `arch` (GPU architecture like sm_80), and `split` (for datasets, e.g., train/val/test). You can invent custom keys as needed; they are purely for discovery and are not used by the store itself.

When listing with `ls`, metadata is shown alongside the hash. This helps you quickly find the artifact you need without having to retrieve all objects. In the future, a `search` command may allow filtering by metadata.

## Contributing

Fork the repository, create a feature branch, make changes to `src/`, add tests as appropriate, run `pre-commit run --all-files`, and open a PR. Code standards: use `thiserror` and `anyhow` for error handling, keep error messages clear and actionable, and test at least the happy path for new commands.

When adding a new command, implement it in `src/cli.rs` and add a corresponding method in `src/store.rs`. Test locally with `cargo test` and `cargo run -- --help`. If your change affects the store format (object layout, metadata schema), document the compatibility implications.

## Build and Test

Build with `cargo build --release` or `cargo build` for debug. Run tests with `cargo test --all-features`. Benchmarking the store's performance is not yet automated; for now, measure manually by timing `put` and `get` operations on files of varying sizes.

## Known Limitations

Version 0 provides basic content-addressed storage and metadata tagging but lacks several planned features. FUSE mounting is not yet implemented; future versions will allow treating the store as a filesystem. Compression is not applied; files are stored at full size. Deduplication works only within a single store; multiple stores do not cross-reference shared objects. Metadata is not indexed; `ls` returns all artifacts and must be filtered in memory. Partial file uploads and resumable transfers are not supported. Backup and replication strategies are not built-in.

The store is designed for relatively static artifacts (weights, datasets) rather than frequently-changing files. It works best when used as a stable reference for reproducible experiments, not as a live working directory.

## Performance and Sizing

Hashing speed with Blake3 is around 5-10 GB/s on modern CPUs, so adding large files (> 10 GB) is I/O bound. Storage overhead is minimal: the store adds only the object file plus a small JSON metadata file per artifact. Deduplication is instant (hash lookup is O(1) in the filesystem sharding scheme). Retrieval is simply a file copy and is I/O limited. No in-memory caching is currently used.

For datasets in the 10-100 GB range, expect `put` to take 1-10 seconds and `get` to take a few seconds, depending on disk speed. Metadata operations are fast regardless of file size.

## Roadmap and Future Work

Planned features include FUSE mounting to treat the store as a filesystem, metadata indexing to speed up discovery, compression and deduplication at the block level, and integration with remote storage backends (S3, GCS). A search command to filter artifacts by metadata is planned. Batch operations for adding multiple files at once are under consideration.

See GitHub Issues for the roadmap and to report bugs or request features.

## Related Documentation

The store is designed to integrate with gpucomm benchmarking tools and serves as a foundation for artifact management in GPU compute experiments. See the main gpucomm documentation for context on how artifact management fits into the broader workflow.

## License

MIT. See LICENSE file.

## Contact

Questions? Open an issue on GitHub or see the repository for discussion.
