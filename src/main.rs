use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use serde::Serialize;

#[derive(Parser, Debug)]
#[command(name = "gpucomm-fs")]
#[command(about = "binary-aware cas + filesystem foundation", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// initialize a store at the given path
    Init { store: PathBuf },

    /// store a file by content hash
    Put {
        store: PathBuf,
        file: PathBuf,
        /// metadata key=value (repeatable)
        #[arg(long = "meta")]
        meta: Vec<String>,
    },

    /// list stored objects (hashes)
    Ls { store: PathBuf },

    /// retrieve object by hash into output path
    Get {
        store: PathBuf,
        hash: String,
        out: PathBuf,
    },
}

#[derive(Serialize)]
struct Meta {
    hash: String,
    size_bytes: u64,
    meta: BTreeMap<String, String>,
}

fn store_layout(store: &Path) -> (PathBuf, PathBuf) {
    (store.join("objects"), store.join("meta"))
}

fn object_path(objects_dir: &Path, hash: &str) -> PathBuf {
    let prefix = &hash[0..2];
    objects_dir.join(prefix).join(hash)
}

fn parse_meta(pairs: Vec<String>) -> Result<BTreeMap<String, String>, String> {
    let mut map = BTreeMap::new();
    for pair in pairs {
        let Some((k, v)) = pair.split_once('=') else {
            return Err(format!("invalid --meta '{pair}', expected key=value"));
        };
        if k.is_empty() {
            return Err(format!("invalid --meta '{pair}', empty key"));
        }
        map.insert(k.to_string(), v.to_string());
    }
    Ok(map)
}

fn hash_file(path: &Path) -> Result<(String, Vec<u8>), String> {
    let mut f = fs::File::open(path).map_err(|e| format!("open {path:?}: {e}"))?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)
        .map_err(|e| format!("read {path:?}: {e}"))?;
    let hash = blake3::hash(&buf).to_hex().to_string();
    Ok((hash, buf))
}

fn ensure_store(store: &Path) -> Result<(PathBuf, PathBuf), String> {
    let (objects_dir, meta_dir) = store_layout(store);
    fs::create_dir_all(&objects_dir).map_err(|e| format!("mkdir {objects_dir:?}: {e}"))?;
    fs::create_dir_all(&meta_dir).map_err(|e| format!("mkdir {meta_dir:?}: {e}"))?;
    Ok((objects_dir, meta_dir))
}

fn list_hashes(objects_dir: &Path) -> Result<Vec<String>, String> {
    let mut hashes = Vec::new();
    if !objects_dir.exists() {
        return Ok(hashes);
    }

    for dir_entry in fs::read_dir(objects_dir).map_err(|e| format!("read_dir: {e}"))? {
        let dir_entry = dir_entry.map_err(|e| format!("read_dir entry: {e}"))?;
        if !dir_entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            continue;
        }
        for obj in fs::read_dir(dir_entry.path()).map_err(|e| format!("read_dir: {e}"))? {
            let obj = obj.map_err(|e| format!("read_dir entry: {e}"))?;
            if obj.file_type().map_err(|e| e.to_string())?.is_file() {
                if let Some(name) = obj.file_name().to_str() {
                    hashes.push(name.to_string());
                }
            }
        }
    }
    hashes.sort();
    Ok(hashes)
}

fn main() -> Result<(), String> {
    let cli = Cli::parse();

    match cli.command {
        Command::Init { store } => {
            ensure_store(&store)?;
            println!("initialized store at {}", store.display());
        }
        Command::Put { store, file, meta } => {
            let (objects_dir, meta_dir) = ensure_store(&store)?;
            let (hash, bytes) = hash_file(&file)?;

            let obj_path = object_path(&objects_dir, &hash);
            if let Some(parent) = obj_path.parent() {
                fs::create_dir_all(parent).map_err(|e| format!("mkdir {parent:?}: {e}"))?;
            }

            if !obj_path.exists() {
                fs::write(&obj_path, &bytes).map_err(|e| format!("write {obj_path:?}: {e}"))?;
            }

            let parsed_meta = parse_meta(meta)?;
            let meta_path = meta_dir.join(format!("{hash}.json"));
            let meta_payload = Meta {
                hash: hash.clone(),
                size_bytes: bytes.len() as u64,
                meta: parsed_meta,
            };
            let json = serde_json::to_vec_pretty(&meta_payload)
                .map_err(|e| format!("serialize meta: {e}"))?;
            let mut out =
                fs::File::create(&meta_path).map_err(|e| format!("write {meta_path:?}: {e}"))?;
            out.write_all(&json)
                .map_err(|e| format!("write {meta_path:?}: {e}"))?;
            out.write_all(b"\n")
                .map_err(|e| format!("write {meta_path:?}: {e}"))?;

            println!("{hash}");
        }
        Command::Ls { store } => {
            let (objects_dir, _) = store_layout(&store);
            for hash in list_hashes(&objects_dir)? {
                println!("{hash}");
            }
        }
        Command::Get { store, hash, out } => {
            let (objects_dir, _) = store_layout(&store);
            if hash.len() < 2 {
                return Err("hash too short".to_string());
            }
            let obj_path = object_path(&objects_dir, &hash);
            let bytes = fs::read(&obj_path).map_err(|e| format!("read {obj_path:?}: {e}"))?;
            fs::write(&out, bytes).map_err(|e| format!("write {out:?}: {e}"))?;
            println!("wrote {}", out.display());
        }
    }

    Ok(())
}
