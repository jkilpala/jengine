//! Live build output — a channel from the build pipeline to whatever is
//! watching it.
//!
//! A build spends most of its wall time inside `cargo` or `wasm-pack`, and
//! that output only ever reached the terminal the editor was launched from.
//! In the editor itself a build was a spinner and nothing else. [`set_sink`]
//! installs a listener (the editor's Build panel does), and [`run_streaming`]
//! forwards every line a child process writes to it as the line arrives.
//!
//! With no sink installed - the CLI, tests, `build_web_cli` - everything here
//! still runs and just logs, so nothing depends on someone listening.

use std::cell::RefCell;
use std::io::{BufRead, BufReader, Read};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

/// A listener for build output lines. Called from the reader threads of a
/// running child process, so it must be cheap and must not block.
///
/// The `bool` is what a terminal would do with the segment: `true` means it
/// arrived terminated by a bare carriage return and **replaces** the previous
/// line rather than following it. Progress bars redraw that way, and a
/// listener that appends every redraw ends up with hundreds of near-identical
/// lines instead of one that counts up.
pub type Sink = Arc<dyn Fn(&str, bool) + Send + Sync>;

thread_local! {
    /// This thread's listener, and the number it is registered under.
    static MINE: RefCell<Option<(u64, Sink)>> = const { RefCell::new(None) };
}

/// Every listener installed and not yet removed, by number.
fn live() -> &'static Mutex<Vec<(u64, Sink)>> {
    static LIVE: OnceLock<Mutex<Vec<(u64, Sink)>>> = OnceLock::new();
    LIVE.get_or_init(|| Mutex::new(Vec::new()))
}

/// Install (or with `None`, remove) the listener for build output - **this
/// thread's**. A build runs on one thread from start to finish (an editor
/// command, the CLI's main), and its child processes' output is carried to
/// the same listener by [`run_streaming`], so two builds at once - a server
/// built while a game builds - each send their lines to their own panel, and
/// the first to finish clears only its own. (It was one process-wide sink:
/// the lines went to whichever build set it last, and the first to finish
/// cleared it for both.) Callers should clear it when the build ends - the
/// editor does that from a drop guard so a failed build cannot leave a stale
/// listener behind.
pub fn set_sink(sink: Option<Sink>) {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let old = MINE.with(|m| m.borrow_mut().take());
    let mut live = live().lock().unwrap_or_else(|e| e.into_inner());
    if let Some((n, _)) = old {
        live.retain(|(k, _)| *k != n);
    }
    if let Some(sink) = sink {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        live.push((n, sink.clone()));
        MINE.with(|m| *m.borrow_mut() = Some((n, sink)));
    }
}

/// The listener for what this thread says: its own, or - for a thread the
/// builder started without one (a worker that logs) - the only one there
/// is. With two builds running, such a line has no build to belong to and
/// goes only to the log.
fn current_sink() -> Option<Sink> {
    if let Some(sink) = MINE.with(|m| m.borrow().as_ref().map(|(_, s)| s.clone())) {
        return Some(sink);
    }
    let live = live().lock().unwrap_or_else(|e| e.into_inner());
    match live.as_slice() {
        [(_, only)] => Some(only.clone()),
        _ => None,
    }
}

/// Send one line to the listener, if there is one.
pub fn emit(line: &str) {
    emit_segment(line, false);
}

/// Send a line that replaces the previous one — a progress-bar redraw.
pub fn emit_progress(line: &str) {
    emit_segment(line, true);
}

fn emit_segment(line: &str, replaces_previous: bool) {
    // Cloned out before calling: the sink emits an IPC event, and holding a
    // lock across that would serialize the reader threads for no reason.
    if let Some(sink) = current_sink() {
        sink(line, replaces_previous);
    }
}

/// Log a build milestone and show it in the Build panel.
///
/// For the steps between child processes - copying assets, running the asset
/// pipeline - so the panel does not go silent while they run.
pub fn step(message: &str) {
    log::info!("{message}");
    emit(message);
}

/// Run a child process, forwarding its output line by line as it arrives.
///
/// Both streams are piped and drained by their own thread: cargo writes
/// progress to stderr and results to stdout, and a pipe that nobody reads
/// fills up and blocks the child. Lines are logged as well as emitted, so a
/// terminal-run build still shows what it always did.
pub fn run_streaming(cmd: &mut Command, what: &str) -> anyhow::Result<ExitStatus> {
    // Echo the command itself, the way a shell transcript would - it is the
    // one line that says which flags this build actually used.
    let rendered = render_command(cmd);
    log::info!("$ {rendered}");
    emit(&format!("$ {rendered}"));

    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| anyhow::anyhow!("Failed to run {what}: {e}"))?;

    // The readers speak for the build that started the child.
    let sink = current_sink();
    let mut readers = Vec::new();
    if let Some(out) = child.stdout.take() {
        let sink = sink.clone();
        readers.push(std::thread::spawn(move || pump_for(out, sink)));
    }
    if let Some(err) = child.stderr.take() {
        let sink = sink.clone();
        readers.push(std::thread::spawn(move || pump_for(err, sink)));
    }

    let status = child
        .wait()
        .map_err(|e| anyhow::anyhow!("Failed to wait for {what}: {e}"))?;
    // Join after waiting: the threads end when the pipes close, which is when
    // the child exits. A panicked reader loses output but must not fail the
    // build, so the result is discarded deliberately.
    for r in readers {
        let _ = r.join();
    }
    Ok(status)
}

/// [`pump`], on a reader thread speaking for `sink`'s build.
fn pump_for(stream: impl std::io::Read, sink: Option<Sink>) {
    MINE.with(|m| *m.borrow_mut() = sink.map(|s| (0, s)));
    pump(stream);
}

/// Drain one output stream, forwarding each line.
///
/// Reads bytes rather than `lines()` so output that is not valid UTF-8 (a
/// compiler emitting a raw byte, a stray control character) is shown lossily
/// instead of ending the stream early.
pub(crate) fn pump(stream: impl std::io::Read) {
    let mut reader = BufReader::new(stream);
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        match reader.read(&mut byte) {
            Ok(0) => break,
            Ok(_) => {}
            Err(_) => break,
        }
        match byte[0] {
            b'\r' => {
                // `\r\n` is a line ending; a **bare** `\r` is a progress
                // redraw. Every line a Windows child writes ends `\r\n`, so
                // without this peek every line on this platform would be
                // flagged as replacing the one before it.
                let crlf = matches!(reader.fill_buf(), Ok([b'\n', ..]));
                if crlf {
                    reader.consume(1);
                }
                flush(&mut buf, !crlf);
            }
            b'\n' => flush(&mut buf, false),
            b => buf.push(b),
        }
    }
    // Whatever a child left without a terminator still happened.
    flush(&mut buf, false);
}

/// Emit one segment and clear the buffer. Empty segments are dropped: they
/// are what a `\r\n` pair produces, and a blank progress redraw is noise.
fn flush(buf: &mut Vec<u8>, replaces_previous: bool) {
    if buf.is_empty() {
        return;
    }
    let line = String::from_utf8_lossy(buf);
    let line = line.trim_end();
    if !line.is_empty() {
        log::info!("{line}");
        emit_segment(line, replaces_previous);
    }
    buf.clear();
}

/// Render a command roughly as a shell would show it.
fn render_command(cmd: &Command) -> String {
    let mut out = cmd.get_program().to_string_lossy().to_string();
    for arg in cmd.get_args() {
        let arg = arg.to_string_lossy();
        out.push(' ');
        if arg.contains(' ') {
            out.push('"');
            out.push_str(&arg);
            out.push('"');
        } else {
            out.push_str(&arg);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Serializes the tests that install a sink. The sink is process-wide by
    /// design, so two of these running in parallel see each other's output.
    static SINK_LOCK: Mutex<()> = Mutex::new(());

    /// Collect what a sink sees, as `(text, replaces_previous)`.
    fn capture(input: &'static [u8]) -> Vec<(String, bool)> {
        let _guard = SINK_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let seen = Arc::new(Mutex::new(Vec::new()));
        {
            let seen = Arc::clone(&seen);
            set_sink(Some(Arc::new(move |line: &str, replaces: bool| {
                seen.lock().unwrap().push((line.to_string(), replaces));
            })));
        }
        pump(std::io::Cursor::new(input));
        set_sink(None);
        let out = seen.lock().unwrap().clone();
        out
    }

    #[test]
    fn a_progress_bar_streams_instead_of_piling_up() {
        // The bug this fixes: cargo and wasm-pack redraw with a bare `\r` and
        // no newline, so a reader that waits for `\n` shows nothing at all for
        // the length of a compile and then one giant garbled line. Each redraw
        // must arrive on its own, flagged as replacing the previous.
        let seen = capture(b"Building [=>] 1/3\rBuilding [==>] 2/3\rBuilding [===>] 3/3\n");
        assert_eq!(seen.len(), 3, "{seen:?}");
        assert_eq!(seen[0], ("Building [=>] 1/3".to_string(), true));
        assert_eq!(seen[1], ("Building [==>] 2/3".to_string(), true));
        // The last one ended with a newline, so it stays.
        assert_eq!(seen[2], ("Building [===>] 3/3".to_string(), false));
    }

    #[test]
    fn crlf_is_a_line_ending_not_a_redraw() {
        // Windows children write `\r\n`. Treating that `\r` as a redraw would
        // mark every line on this platform as replacing the one before it —
        // the panel would show one line, updating forever.
        let seen = capture(b"first\r\nsecond\r\n");
        assert_eq!(
            seen,
            vec![
                ("first".to_string(), false),
                ("second".to_string(), false),
            ],
            "{seen:?}"
        );
    }

    #[test]
    fn output_without_a_trailing_newline_is_not_lost() {
        // A child that exits mid-line still said something.
        let seen = capture(b"no newline at the end");
        assert_eq!(seen.len(), 1, "{seen:?}");
        assert_eq!(seen[0].0, "no newline at the end");
    }

    /// P2: two builds at once each hear their own lines - their own
    /// `emit`s and their own child's output - and the first to finish does
    /// not silence the other.
    #[test]
    fn two_builds_at_once_each_hear_their_own() {
        let _guard = SINK_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let start = Arc::new(std::sync::Barrier::new(2));
        let build = |name: &'static str, lines: usize| {
            let start = start.clone();
            std::thread::spawn(move || {
                let seen = Arc::new(Mutex::new(Vec::<String>::new()));
                {
                    let seen = Arc::clone(&seen);
                    set_sink(Some(Arc::new(move |line: &str, _| seen.lock().unwrap().push(line.to_string()))));
                }
                start.wait();
                for i in 0..lines {
                    emit(&format!("{name} {i}"));
                    std::thread::yield_now();
                }
                let mut cmd = Command::new("cargo");
                cmd.arg("--version");
                run_streaming(&mut cmd, "cargo").unwrap();
                set_sink(None);
                let out = seen.lock().unwrap().clone();
                out
            })
        };
        let (a, b) = (build("alpha", 50), build("beta", 500));
        let (a, b) = (a.join().unwrap(), b.join().unwrap());
        assert!(a.iter().all(|l| !l.starts_with("beta")) && b.iter().all(|l| !l.starts_with("alpha")), "{a:?}");
        assert_eq!(a.iter().filter(|l| l.starts_with("alpha")).count(), 50);
        assert_eq!(b.iter().filter(|l| l.starts_with("beta")).count(), 500, "beta outlived alpha and still heard itself");
        for seen in [&a, &b] {
            assert_eq!(seen.iter().filter(|l| l.starts_with("$ cargo")).count(), 1, "each its own child: {seen:?}");
            assert!(seen.iter().any(|l| l.starts_with("cargo ")), "and its output: {seen:?}");
        }
    }

    #[test]
    fn emit_is_a_no_op_without_a_sink() {
        set_sink(None);
        emit("nobody is listening");
    }

    #[test]
    fn sink_receives_child_output() {
        let _guard = SINK_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let seen = Arc::new(Mutex::new(Vec::<String>::new()));
        let calls = Arc::new(AtomicUsize::new(0));
        {
            let seen = Arc::clone(&seen);
            let calls = Arc::clone(&calls);
            set_sink(Some(Arc::new(move |line: &str, _replaces: bool| {
                calls.fetch_add(1, Ordering::Relaxed);
                seen.lock().unwrap().push(line.to_string());
            })));
        }

        // `cargo --version` is the one child process guaranteed to exist
        // wherever this test runs.
        let mut cmd = Command::new("cargo");
        cmd.arg("--version");
        let status = run_streaming(&mut cmd, "cargo").expect("spawn cargo");
        set_sink(None);

        assert!(status.success());
        let seen = seen.lock().unwrap();
        // The echoed command line, plus at least the version banner.
        assert!(seen[0].starts_with("$ cargo"), "{seen:?}");
        assert!(
            seen.iter().any(|l| l.contains("cargo")),
            "no child output forwarded: {seen:?}"
        );
    }
}
