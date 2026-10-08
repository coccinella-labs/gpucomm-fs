use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};

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

    /// re-hash every stored object and report corruption
    Verify { store: PathBuf },

    /// retrieve object by hash into output path
    Get {
        store: PathBuf,
        hash: String,
        out: PathBuf,
    },
}

#[derive(Serialize, Deserialize)]
struct Meta {
    hash: String,
    size_bytes: u64,
    /// Values are lists so that repeating `--meta key=value` accumulates
    /// provenance instead of silently discarding earlier values.
    meta: BTreeMap<String, Vec<String>>,
}

fn store_layout(store: &Path) -> (PathBuf, PathBuf) {
    (store.join("objects"), store.join("meta"))
}

fn object_path(objects_dir: &Path, hash: &str) -> PathBuf {
    let prefix = &hash[0..2];
    objects_dir.join(prefix).join(hash)
}

fn parse_meta(pairs: Vec<String>) -> Result<BTreeMap<String, Vec<String>>, String> {
    let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for pair in pairs {
        let Some((k, v)) = pair.split_once('=') else {
            return Err(format!("invalid --meta '{pair}', expected key=value"));
        };
        if k.is_empty() {
            return Err(format!("invalid --meta '{pair}', empty key"));
        }
        let entry = map.entry(k.to_string()).or_default();
        if !entry.iter().any(|existing| existing == v) {
            entry.push(v.to_string());
        }
    }
    Ok(map)
}

/// Merge new metadata into whatever is already recorded for an object.
///
/// A later `put` of identical content should not drop earlier provenance,
/// so keys present in both are unioned rather than replaced.
fn merge_meta(
    existing: &BTreeMap<String, Vec<String>>,
    incoming: BTreeMap<String, Vec<String>>,
) -> BTreeMap<String, Vec<String>> {
    let mut merged = existing.clone();
    for (key, values) in incoming {
        let entry = merged.entry(key).or_default();
        for value in values {
            if !entry.iter().any(|existing| existing == &value) {
                entry.push(value);
            }
        }
    }
    merged
}

fn hash_file(path: &Path) -> Result<(String, Vec<u8>), String> {
    let mut f = fs::File::open(path).map_err(|e| format!("open {path:?}: {e}"))?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)
        .map_err(|e| format!("read {path:?}: {e}"))?;
    let hash = blake3::hash(&buf).to_hex().to_string();
    Ok((hash, buf))
}

/// Re-hash a stored object and confirm it matches the hash it is filed under.
/// `size` is the byte length recorded in metadata, if available.
fn verify_object(objects_dir: &Path, meta_dir: &Path, hash: &str) -> Result<(), String> {
    let obj_path = object_path(objects_dir, hash);
    let bytes = fs::read(&obj_path).map_err(|e| format!("read {obj_path:?}: {e}"))?;

    let actual = blake3::hash(&bytes).to_hex().to_string();
    if actual != hash {
        return Err(format!(
            "integrity check failed for {hash}: stored object hashes to {actual}"
        ));
    }

    // Metadata is written on put, so a size mismatch means the record drifted
    // from the object it describes.
    let meta_path = meta_dir.join(format!("{hash}.json"));
    match fs::read(&meta_path) {
        Ok(raw) => match serde_json::from_slice::<Meta>(&raw) {
            Ok(record) => {
                if record.hash != hash {
                    return Err(format!(
                        "metadata {hash}.json records hash {} instead",
                        record.hash
                    ));
                }
                if record.size_bytes != bytes.len() as u64 {
                    return Err(format!(
                        "metadata {hash}.json records size {} but object is {} bytes",
                        record.size_bytes,
                        bytes.len()
                    ));
                }
            }
            Err(e) => return Err(format!("parse {meta_path:?}: {e}")),
        },
        Err(_) => return Err(format!("missing metadata for {hash}: {meta_path:?}")),
    }

    Ok(())
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

            let mut merged_meta = BTreeMap::new();
            if let Ok(existing) = fs::read(&meta_path) {
                if let Ok(previous) = serde_json::from_slice::<Meta>(&existing) {
                    merged_meta = previous.meta;
                }
            }

            let meta_payload = Meta {
                hash: hash.clone(),
                size_bytes: bytes.len() as u64,
                meta: merge_meta(&merged_meta, parsed_meta),
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
            let (objects_dir, meta_dir) = store_layout(&store);
            if hash.len() < 2 {
                return Err("hash too short".to_string());
            }

            // Verify before handing anything back. A content-addressed store
            // that trusts the filename would serve corrupted bytes silently.
            verify_object(&objects_dir, &meta_dir, &hash)?;

            let obj_path = object_path(&objects_dir, &hash);
            let bytes = fs::read(&obj_path).map_err(|e| format!("read {obj_path:?}: {e}"))?;
            fs::write(&out, &bytes).map_err(|e| format!("write {out:?}: {e}"))?;
            println!("wrote {}", out.display());
        }
        Command::Verify { store } => {
            let (objects_dir, meta_dir) = store_layout(&store);
            if !objects_dir.exists() {
                return Err(format!("no store at {}", store.display()));
            }

            let hashes = list_hashes(&objects_dir)?;
            let mut bad = 0usize;
            for hash in &hashes {
                match verify_object(&objects_dir, &meta_dir, hash) {
                    Ok(()) => println!("ok {hash}"),
                    Err(e) => {
                        bad += 1;
                        println!("FAILED {hash}: {e}");
                    }
                }
            }

            println!("{} checked, {bad} failed", hashes.len());
            if bad > 0 {
                return Err(format!(
                    "{bad} of {} objects failed verification",
                    hashes.len()
                ));
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store_in_temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("gpucomm-fs-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        dir.join(".gpucomm-fs")
    }

    #[test]
    fn repeated_meta_key_accumulates() {
        let m = parse_meta(vec!["tag=v1".into(), "tag=v2".into()]).unwrap();
        assert_eq!(
            m.get("tag").unwrap(),
            &vec!["v1".to_string(), "v2".to_string()]
        );
    }

    #[test]
    fn duplicate_meta_value_is_not_repeated() {
        let m = parse_meta(vec!["tag=v1".into(), "tag=v1".into()]).unwrap();
        assert_eq!(m.get("tag").unwrap().len(), 1);
    }

    #[test]
    fn merge_meta_unions_keys_and_keeps_earlier_ones() {
        let mut existing = BTreeMap::new();
        existing.insert("cuda".to_string(), vec!["12.1".to_string()]);
        let mut incoming = BTreeMap::new();
        incoming.insert("kind".to_string(), vec!["weights".to_string()]);
        let merged = merge_meta(&existing, incoming);
        assert_eq!(merged.get("cuda").unwrap(), &vec!["12.1".to_string()]);
        assert_eq!(merged.get("kind").unwrap(), &vec!["weights".to_string()]);
    }

    #[test]
    fn bad_meta_is_rejected() {
        assert!(parse_meta(vec!["nokey".into()]).is_err());
        assert!(parse_meta(vec!["=v".into()]).is_err());
    }

    #[test]
    fn hash_is_blake3_and_stable() {
        let p = std::env::temp_dir().join("gpucomm-fs-hash-probe.bin");
        fs::write(&p, b"abc").unwrap();
        let (h1, _) = hash_file(&p).unwrap();
        let (h2, _) = hash_file(&p).unwrap();
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64, "blake3 hex is 64 chars");
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn object_path_shards_by_prefix() {
        let (objects, _) = store_layout(Path::new("/tmp/s"));
        let p = object_path(&objects, &"ab".repeat(32));
        assert_eq!(p.file_name().unwrap(), "ab".repeat(32).as_str());
        assert!(p.parent().unwrap().ends_with("ab"));
    }

    #[test]
    fn init_creates_layout() {
        let store = store_in_temp("init");
        let (objects, meta) = ensure_store(&store).unwrap();
        assert!(objects.exists());
        assert!(meta.exists());
        let _ = fs::remove_dir_all(store.parent().unwrap());
    }
}

#[cfg(test)]
mod verify_tests {
    use super::*;

    struct TempStore(PathBuf);

    impl Drop for TempStore {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(self.0.parent().unwrap());
        }
    }

    fn store(name: &str) -> TempStore {
        let dir = std::env::temp_dir().join(format!("gpucomm-fs-vt-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        TempStore(dir.join(".gpucomm-fs"))
    }

    fn put(store: &TempStore, bytes: &[u8], meta: &[(&str, &str)]) -> String {
        let src = store.0.parent().unwrap().join("src.bin");
        fs::write(&src, bytes).unwrap();
        let (hash, _) = hash_file(&src).unwrap();
        let (objects_dir, meta_dir) = ensure_store(&store.0).unwrap();
        let obj_path = object_path(&objects_dir, &hash);
        fs::create_dir_all(obj_path.parent().unwrap()).unwrap();
        fs::write(&obj_path, bytes).unwrap();
        let parsed = parse_meta(meta.iter().map(|(k, v)| format!("{k}={v}")).collect()).unwrap();
        let payload = Meta {
            hash: hash.clone(),
            size_bytes: bytes.len() as u64,
            meta: parsed,
        };
        fs::write(
            meta_dir.join(format!("{hash}.json")),
            serde_json::to_vec_pretty(&payload).unwrap(),
        )
        .unwrap();
        hash
    }

    #[test]
    fn clean_object_verifies() {
        let s = store("clean");
        let h = put(&s, b"hello world", &[("kind", "weights")]);
        let (o, m) = ensure_store(&s.0).unwrap();
        verify_object(&o, &m, &h).expect("clean object must verify");
    }

    #[test]
    fn corrupted_object_is_caught() {
        let s = store("corrupt");
        let h = put(&s, b"hello world", &[]);
        let (o, m) = ensure_store(&s.0).unwrap();
        fs::write(object_path(&o, &h), b"tampered").unwrap();
        let err = verify_object(&o, &m, &h).unwrap_err();
        assert!(err.contains("integrity check failed"), "got: {err}");
    }

    #[test]
    fn missing_metadata_is_caught() {
        let s = store("nometa");
        let h = put(&s, b"payload", &[]);
        let (o, m) = ensure_store(&s.0).unwrap();
        fs::remove_file(m.join(format!("{h}.json"))).unwrap();
        let err = verify_object(&o, &m, &h).unwrap_err();
        assert!(err.contains("missing metadata"), "got: {err}");
    }

    #[test]
    fn size_drift_between_object_and_metadata_is_caught() {
        let s = store("size");
        let h = put(&s, b"payload", &[]);
        let (o, m) = ensure_store(&s.0).unwrap();
        let meta_path = m.join(format!("{h}.json"));
        let mut record: Meta = serde_json::from_slice(&fs::read(&meta_path).unwrap()).unwrap();
        record.size_bytes = 999;
        fs::write(&meta_path, serde_json::to_vec(&record).unwrap()).unwrap();
        let err = verify_object(&o, &m, &h).unwrap_err();
        assert!(err.contains("records size 999"), "got: {err}");
    }

    #[test]
    fn single_byte_corruption_is_detected() {
        let s = store("onebyte");
        let original: Vec<u8> = (0..=255u8).collect();
        let h = put(&s, &original, &[]);
        let (o, m) = ensure_store(&s.0).unwrap();
        let mut tampered = original.clone();
        tampered[100] ^= 0x01;
        fs::write(object_path(&o, &h), &tampered).unwrap();
        assert!(verify_object(&o, &m, &h).is_err());
    }
}
