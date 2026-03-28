use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

fn fixture_path(name: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../testData")
        .join(name);
    assert!(path.exists(), "fixture should exist: {}", path.display());
    path.canonicalize().expect("fixture path should resolve")
}

fn run_open_sequence(path: &PathBuf) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_dwg-worker"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("worker process should spawn");

    let request_stream = format!(
        "{{\"id\":1,\"method\":\"openFile\",\"params\":{{\"path\":\"{}\"}}}}\n\
         {{\"id\":2,\"method\":\"listFileTypes\",\"params\":{{}}}}\n\
         {{\"id\":3,\"method\":\"queryObjects\",\"params\":{{\"typeName\":\"AcDbTable\",\"mode\":\"summary\",\"limit\":1}}}}\n\
         {{\"id\":4,\"method\":\"closeFile\",\"params\":{{}}}}\n",
        path.display()
    );

    {
        let stdin = child.stdin.as_mut().expect("child stdin should be present");
        stdin
            .write_all(request_stream.as_bytes())
            .expect("requests should be written");
    }

    child
        .wait_with_output()
        .expect("worker process should terminate")
}

fn assert_worker_open_sequence_succeeds(fixture_name: &str) {
    let fixture = fixture_path(fixture_name);
    let output = run_open_sequence(&fixture);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "worker should exit successfully for {} (status: {:?}).\nstdout:\n{}\nstderr:\n{}",
        fixture.display(),
        output.status,
        stdout,
        stderr
    );

    let responses = stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();

    assert!(
        responses.len() >= 4,
        "expected open/list/query/close responses for {}.\nstdout:\n{}",
        fixture.display(),
        stdout
    );
    assert!(
        responses[0].contains("\"id\":1") && responses[0].contains("\"result\""),
        "openFile should return a result for {}.\nfirst response:\n{}",
        fixture.display(),
        responses[0]
    );
    assert!(
        responses[0].contains("\"backend\":\"libredwg-native\""),
        "openFile should report libredwg backend for {}.\nfirst response:\n{}",
        fixture.display(),
        responses[0]
    );
    assert!(
        responses[1].contains("\"id\":2") && responses[1].contains("\"total\":"),
        "listFileTypes should return totals for {}.\nsecond response:\n{}",
        fixture.display(),
        responses[1]
    );
    assert!(
        responses[2].contains("\"id\":3")
            && responses[2].contains("\"total\":")
            && responses[2].contains("\"items\":["),
        "queryObjects should return table summaries for {}.\nthird response:\n{}",
        fixture.display(),
        responses[2]
    );
    assert!(
        !responses[2].contains("\"total\":0"),
        "queryObjects should find at least one AcDbTable in {}.\nthird response:\n{}",
        fixture.display(),
        responses[2]
    );
    assert!(
        responses[2].contains("\"typeName\":\"AcDbTable\""),
        "queryObjects should include AcDbTable items for {}.\nthird response:\n{}",
        fixture.display(),
        responses[2]
    );
    assert!(
        responses[3].contains("\"id\":4") && responses[3].contains("\"closed\":true"),
        "closeFile should report closed=true for {}.\nfourth response:\n{}",
        fixture.display(),
        responses[3]
    );
}

#[test]
fn worker_opens_blocks_and_tables_imperial_without_crashing() {
    assert_worker_open_sequence_succeeds("blocks_and_tables_-_imperial.dwg");
}

#[test]
fn worker_opens_blocks_and_tables_metric_without_crashing() {
    assert_worker_open_sequence_succeeds("blocks_and_tables_-_metric.dwg");
}
