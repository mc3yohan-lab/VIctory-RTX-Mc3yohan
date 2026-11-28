use std::fs;
use std::process::Command;

#[test]
fn warm_retries_a_refused_pipeline_even_with_a_legacy_completion() {
    let temp = tempfile::tempdir().unwrap();
    let cache = temp.path().join("cache");
    let pipelines = cache.join("pipelines");
    fs::create_dir_all(&pipelines).unwrap();
    fs::write(pipelines.join("known-shaders.bin"), []).unwrap();
    // One compute pipeline referring to a shader absent from the synthetic book.
    let mut entry = vec![0xab; 16];
    entry.extend([1, 5]);
    entry.extend([0xcd; 32]);
    entry.push(0);
    let mut book = (entry.len() as u32).to_le_bytes().to_vec();
    book.extend(entry);
    fs::write(pipelines.join("known-pipelines.bin"), book).unwrap();

    let warm = |args: &[&str]| {
        let output = Command::new(env!("CARGO_BIN_EXE_victory"))
            .args(["warm", "--jobs", "1"])
            .args(args)
            .current_dir(temp.path())
            .env_remove("VICTORY_CONFIG")
            .env("VICTORY_CACHE_DIR", &cache)
            .env("VICTORY_DATA_DIR", temp.path().join("data"))
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        String::from_utf8(output.stdout).unwrap()
    };
    let first = warm(&[]);
    assert!(first.contains("1 have no host pipeline"), "{first}");
    let (_, tail) = first.split_once("made before (").unwrap();
    let (fingerprint, _) = tail.split_once(')').unwrap();
    let record = format!("{fingerprint} {}\n", "ab".repeat(16));
    fs::write(pipelines.join("warm-done.txt"), &record).unwrap();
    let second = warm(&[]);
    assert!(second.contains("1 have no host pipeline"), "{second}");
    let ready = pipelines.join("warm-host-ready.txt");
    assert!(fs::read(&ready).unwrap().is_empty());

    // A positive record from the new format still avoids redundant work.
    fs::write(&ready, record).unwrap();
    let third = warm(&[]);
    assert!(third.contains("1 made before"), "{third}");
    assert!(third.contains("0 to make"), "{third}");

    // The isolated worker must approve a target before the same-process library
    // pass tries it, even when an older run succeeded under this fingerprint.
    let retry = warm(&["--again", "--save-library"]);
    assert!(retry.contains("preparing 0 verified pipelines"), "{retry}");
    let subsequent = warm(&[]);
    assert!(subsequent.contains("0 made before"), "{subsequent}");
    assert!(subsequent.contains("1 have no host pipeline"), "{subsequent}");
}
