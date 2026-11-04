//! M14 supervisor integration (`#[ignore]`): needs the built sidecar jar and
//! a local Java 17, but nothing beyond loopback — the health endpoint is the
//! restart probe. Run with:
//!
//! ```sh
//! cargo test -p sources --test yt_supervisor -- --ignored --nocapture
//! ```

use sources::yt::YtMusicSource;
use sources::MusicSource;
use std::time::Duration;

fn java() -> String {
    std::env::var("JAVA_HOME")
        .map(|h| format!("{h}/bin/java"))
        .unwrap_or_else(|_| "/opt/homebrew/opt/openjdk@17/libexec/openjdk.jdk/Contents/Home/bin/java".to_string())
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "needs the built sidecar jar + local Java"]
async fn killed_sidecar_restarts_through_the_backoff_gate() {
    let source = YtMusicSource::launch(None, Some(java())).expect("sidecar launches");
    let first_pid = source.pid().expect("pid known");
    println!("first pid: {first_pid}");

    // Kill the JVM out from under the supervisor. SIGKILL simulates the
    // crash the supervisor exists for. The killed JVM stays an unreaped
    // zombie until the supervisor's `try_wait` runs, so liveness is asserted
    // through the supervisor itself: health must answer again, with a NEW
    // pid, inside the 20 s spawn deadline + backoff.
    kill9(first_pid);
    let mut restarted = false;
    for _ in 0..60 {
        match source.health().await {
            Ok(sources::SourceHealth::Ok { .. }) if source.pid() != Some(first_pid) => {
                restarted = true;
                break;
            }
            _ => std::thread::sleep(Duration::from_millis(300)),
        }
    }
    assert!(restarted, "supervisor restarted the sidecar");
    let second_pid = source.pid().expect("pid after restart");
    println!("second pid: {second_pid}");
    assert_ne!(second_pid, first_pid, "a NEW jvm process");
    assert!(source.version().is_some());
}

fn kill9(pid: u32) {
    let status = std::process::Command::new("kill")
        .args(["-9", &pid.to_string()])
        .status()
        .expect("kill runs");
    assert!(status.success(), "kill -9 {pid} failed");
}

