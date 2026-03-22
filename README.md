# gpucomm-fs

Binary-aware artifact store and filesystem foundation for gpucomm.

## goals
- store large binaries (datasets, weights, cubin/ptx, benchmark bundles)
- dedupe + integrity via content addressing
- metadata to make artifacts searchable and reproducible
- optional future: mount via fuse

## status
v0 provides a minimal content-addressed store (cas) + cli.

## cli
```bash
cargo run -- init .gpucomm-fs
cargo run -- put .gpucomm-fs path/to/file.bin --meta kind=weights --meta cuda=12.1
cargo run -- ls .gpucomm-fs
cargo run -- get .gpucomm-fs <hash> out.bin
```

## design
- objects: `.gpucomm-fs/objects/<hh>/<hash>`
- metadata: `.gpucomm-fs/meta/<hash>.json`
- hash: blake3(file bytes)
