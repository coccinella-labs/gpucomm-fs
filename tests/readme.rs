//! Guards README claims against the implementation.
//!
//! This repository shipped a README that contradicted its own source in several
//! places: it described metadata timestamps that do not exist, told readers
//! `ls` shows metadata, and prescribed error-handling crates that are not
//! dependencies. Documentation drift is a build failure, not a comment.

use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn readme() -> String {
    std::fs::read_to_string(repo_root().join("README.md")).expect("README.md")
}

fn binary() -> PathBuf {
    // Built by cargo test as part of the bin target, next to the test binary.
    let mut path = std::env::current_exe().expect("current exe");
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.join("gpucomm-fs")
}

fn cli_help() -> String {
    let output = Command::new(binary())
        .arg("--help")
        .output()
        .expect("run --help");
    assert!(output.status.success(), "--help exited non-zero");
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Command names as listed under the "Commands:" heading of `--help`.
fn documented_command_names(help: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut in_commands = false;

    for line in help.lines() {
        if line.trim() == "Commands:" {
            in_commands = true;
            continue;
        }
        if !in_commands {
            continue;
        }
        if line.trim().is_empty() {
            break;
        }
        let Some(name) = line.split_whitespace().next() else {
            break;
        };
        if !name.chars().all(|c| c.is_ascii_lowercase()) {
            break;
        }
        names.push(name.to_string());
    }

    names
}

/// Every command the CLI actually exposes must appear in the README.
#[test]
fn every_cli_command_is_documented() {
    let help = cli_help();
    let names = documented_command_names(&help);
    assert!(
        !names.is_empty(),
        "could not parse commands from help:\n{help}"
    );

    // `help` is provided by clap itself, not by this crate.
    let readme = readme();
    for name in names.iter().filter(|n| n.as_str() != "help") {
        assert!(
            readme.contains(name.as_str()),
            "command `{name}` is not mentioned in README.md"
        );
    }

    for expected in ["init", "put", "ls", "get", "verify"] {
        assert!(
            names.iter().any(|n| n == expected),
            "expected `{expected}` in CLI help, parsed: {names:?}"
        );
    }
}

/// A README claiming five commands must not still describe four.
#[test]
fn command_count_claim_matches_help() {
    let readme = readme();
    let count = documented_command_names(&cli_help()).len();

    for (claim, spelled) in [
        ("four main commands", 4),
        ("five main commands", 5),
        ("six main commands", 6),
    ] {
        if let Some(at) = readme.find(claim) {
            assert_eq!(
                count, spelled,
                "README says \"{claim}\" at line offset {at} but the CLI exposes {count}"
            );
        }
    }
}

/// Claims that were false when this test was written. If the implementation
/// changes such that one becomes true, update the README and delete it here.
#[test]
fn known_false_claims_stay_absent() {
    let readme = readme();

    let forbidden: [(&str, &str); 8] = [
        ("created_at", "Meta has no created_at field"),
        ("timestamp", "Meta has no timestamp field"),
        (
            "metadata is shown alongside the hash",
            "ls prints hashes only",
        ),
        ("thiserror", "not a dependency"),
        ("anyhow", "not a dependency"),
        ("--all-features", "no cargo features are defined"),
        ("about 170 lines", "line-count claims go stale"),
        ("about 280 lines", "line-count claims go stale"),
    ];

    for (claim, why) in forbidden {
        assert!(
            !readme.contains(claim),
            "README contains `{claim}`, which is false: {why}"
        );
    }
}

/// The documented metadata example must match the serialized shape of `Meta`.
#[test]
fn documented_metadata_example_matches_schema() {
    let readme = readme();
    let source = std::fs::read_to_string(repo_root().join("src/main.rs")).unwrap();

    for field in ["hash", "size_bytes", "meta"] {
        assert!(
            source.contains(field),
            "Meta no longer mentions `{field}`; update this test"
        );
        assert!(readme.contains(field), "README does not mention `{field}`");
    }

    // Values are lists, so the example must show arrays, not bare strings.
    assert!(
        readme.contains(r#""cuda": ["12.1"]"#),
        "README example must show list-valued metadata"
    );
    assert!(
        !readme.contains(r#""cuda": "12.1""#),
        "README example shows scalar metadata, which the schema cannot produce"
    );
}

/// The layout the README documents must match the layout the code builds.
#[test]
fn documented_layout_matches_implementation() {
    let readme = readme();
    let source = std::fs::read_to_string(repo_root().join("src/main.rs")).unwrap();

    for literal in ["objects", "meta"] {
        assert!(
            source.contains(&format!("join(\"{literal}\")")),
            "source no longer builds a `{literal}` directory"
        );
        assert!(
            readme.contains(literal),
            "README omits the `{literal}` directory"
        );
    }
    assert!(
        readme.contains(".json"),
        "README omits the metadata file suffix"
    );
}

/// The package must not describe itself as a filesystem or as format-aware.
/// It hashes raw bytes and never inspects them, and there is no FUSE layer.
#[test]
fn package_metadata_does_not_claim_features_absent() {
    let source = std::fs::read_to_string(repo_root().join("src/main.rs")).unwrap();

    for (claim, why) in [
        (
            "filesystem foundation",
            "there is no FUSE layer or filesystem abstraction",
        ),
        (
            "binary-aware",
            "the store hashes raw bytes and never inspects formats",
        ),
    ] {
        assert!(
            !source.contains(claim),
            "package metadata claims `{claim}`, which is false: {why}"
        );
    }

    // The replacement must be accurate rather than merely different.
    assert!(
        source.contains("content-addressed artifact store"),
        "the about string should describe the store as content-addressed"
    );
}

/// Performance numbers must be labelled as measurements, not predictions.
#[test]
fn performance_numbers_are_marked_as_measured() {
    let readme = readme();
    let has_numbers = readme.contains("GiB/s") || readme.contains("GB/s");

    if has_numbers {
        assert!(
            readme.contains("Measured") || readme.contains("measured"),
            "README quotes throughput but never says it was measured"
        );
        assert!(
            readme.contains("machine") || readme.contains("disk-dependent"),
            "README quotes throughput without noting it is hardware dependent"
        );
    }
}
