//! Structural gate: no test may discover a host port by probing and releasing.
//!
//! The banned shape binds `127.0.0.1:0`, reads the assigned port number, and
//! lets the socket go — then hands the bare number to code that binds it
//! later. Between the release and the real bind the port belongs to nobody,
//! so any concurrent process can take it. Under nextest, where every test is
//! its own process, that races the rest of the suite. It reached CI as
//! `failed to bind egress proxy on 127.0.0.1:38373: address in use`.
//!
//! [`nimbus_process_harness::PortWindow`] replaces the shape: it claims a
//! window of ports by holding a socket on the window's first port, below the
//! host's ephemeral range, so the ports inside it are the claimant's alone.
//!
//! Two stereotyped spellings carry the defect, and both are recognised here
//! by the fact that **the number outlives the socket**:
//!
//! 1. *Temporary probe.* `TcpListener::bind(..)` and `.port()` in one
//!    statement. The listener is an unnamed temporary, so it is dropped at the
//!    semicolon and only the number survives. Keeping a listener requires
//!    naming it, so this spelling is never a false positive.
//! 2. *Named probe, later dropped.* The listener is bound to a name, its port
//!    is read, and the name is later passed to `drop`.
//!
//! Binding port zero and *keeping* the listener is correct and common — a
//! server that reports where it landed — so neither rule fires on it.
//!
//! The gate carries no allowlist. Every occurrence was migrated, so the
//! correct count is zero and any new one is a regression rather than a
//! pre-existing debt to be tracked.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Directories with no first-party Rust source to judge.
const SKIPPED_DIRS: &[&str] = &["target", "node_modules", "vendor", ".git"];

/// This file quotes the banned shapes in prose and in its own fixtures.
const SELF: &str = "host_ports_are_claimed_not_probed.rs";

#[test]
fn no_test_discovers_a_host_port_by_probing_and_releasing_it() {
    let root = repository_root();
    let mut violations = Vec::new();
    let mut scanned = 0usize;

    for file in rust_sources(&root) {
        if file.file_name().and_then(|name| name.to_str()) == Some(SELF) {
            continue;
        }
        let Ok(text) = fs::read_to_string(&file) else {
            continue;
        };
        scanned += 1;
        let source = strip_comments(&text);
        let shown = file.strip_prefix(&root).unwrap_or(&file).display();
        for (line, shape) in probe_violations(&source) {
            violations.push(format!("{shown}:{line} — {shape}"));
        }
    }

    assert!(
        scanned > 0,
        "the gate scanned no Rust sources under {}, so it was proving nothing",
        root.display()
    );

    assert!(
        violations.is_empty(),
        "{} host port(s) are discovered by probing and releasing, which races every \
         other test process. Claim a window instead:\n    \
         use nimbus_process_harness::PortWindow;\n    \
         let window = PortWindow::claim();   // hold it for as long as the ports matter\n    \
         let port = window.port(0);\n\n{}",
        violations.len(),
        violations.join("\n")
    );
}

/// Both banned spellings, as `(line, description)` pairs.
fn probe_violations(source: &str) -> Vec<(usize, String)> {
    // Nearly every file in the workspace has no port bind at all, so one
    // substring check keeps the gate off the hot path for the vast majority.
    if !source.contains("TcpListener::bind(") {
        return Vec::new();
    }

    let mut found = Vec::new();
    let released: BTreeSet<String> = dropped_names(source).into_iter().collect();

    for (line, statement) in statements(source) {
        let Some(bind) = statement.find("TcpListener::bind(") else {
            continue;
        };
        let arguments = call_arguments(&statement[bind..]);
        if !binds_port_zero(&arguments) {
            continue;
        }

        // Shape 1: the port is read in the same statement that binds it, so
        // whatever the statement names is the *number*, not the listener --
        // the listener is an unnamed temporary that dies at the semicolon.
        if statement[bind..].contains(".port()") {
            found.push((
                line,
                "binds port zero and reads `.port()` in one statement, so the listener is a \
                 temporary and only the number survives"
                    .to_owned(),
            ));
            continue;
        }

        // Shape 2: the statement names the listener. That is only a defect if
        // the file later reads its port *and* drops it, leaving the number in
        // circulation after the socket is gone.
        let Some(name) = let_binding(&statement) else {
            continue;
        };
        let port_is_read = source.contains(&format!("{name}.local_addr"));
        if released.contains(&name) && port_is_read {
            found.push((
                line,
                format!(
                    "binds port zero into `{name}`, reads its port, and later drops it, leaving \
                     the number in use after the socket is gone"
                ),
            ));
        }
    }

    found
}

/// Names passed to `drop(..)` anywhere in the file.
fn dropped_names(source: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = source;
    while let Some(at) = rest.find("drop(") {
        // Reject `std::mem::drop`-adjacent identifiers such as `can_drop(`.
        let preceding = rest[..at].chars().next_back();
        if preceding.is_some_and(|character| character.is_alphanumeric() || character == '_') {
            rest = &rest[at + "drop(".len()..];
            continue;
        }
        let tail = &rest[at + "drop(".len()..];
        if let Some(close) = tail.find(')') {
            let inner = tail[..close].trim();
            let inner = inner.strip_prefix("&mut ").unwrap_or(inner);
            let inner = inner.strip_prefix('&').unwrap_or(inner);
            // `drop(self.reservation.take())` releases the field, so take the
            // last path segment before any call.
            let candidate = inner
                .split(['.', ' '])
                .next()
                .unwrap_or(inner)
                .trim()
                .to_owned();
            if is_identifier(&candidate) {
                names.push(candidate);
            }
            if let Some(field) = inner.split('.').nth(1) {
                let field = field.trim_end_matches("()").trim();
                if is_identifier(field) {
                    names.push(field.to_owned());
                }
            }
        }
        rest = &rest[at + "drop(".len()..];
    }
    names
}

/// The name a statement binds, for `let NAME = ..` and `NAME: Some(..)`.
fn let_binding(statement: &str) -> Option<String> {
    let trimmed = statement.trim_start();
    if let Some(rest) = trimmed.strip_prefix("let ") {
        let rest = rest.strip_prefix("mut ").unwrap_or(rest);
        let name: String = rest
            .chars()
            .take_while(|character| character.is_alphanumeric() || *character == '_')
            .collect();
        return is_identifier(&name).then_some(name);
    }
    // Struct-literal field: `reservation: TcpListener::bind(..)..`.
    let head = trimmed.split(':').next()?.trim();
    is_identifier(head).then(|| head.to_owned())
}

/// The text inside the outermost parentheses of a call.
fn call_arguments(call: &str) -> String {
    let Some(open) = call.find('(') else {
        return String::new();
    };
    let mut depth = 0usize;
    for (index, character) in call[open..].char_indices() {
        match character {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return call[open + 1..open + index].to_owned();
                }
            }
            _ => {}
        }
    }
    String::new()
}

/// Whether a bind argument asks the kernel for an ephemeral port.
///
/// Covers the spellings this workspace actually uses: `"127.0.0.1:0"`,
/// `(Ipv4Addr::LOCALHOST, 0)`, and `SocketAddr::new(address, 0)`.
fn binds_port_zero(arguments: &str) -> bool {
    if arguments.contains(":0\"") {
        return true;
    }
    let trimmed = arguments.trim_end().trim_end_matches(')');
    match trimmed.rsplit_once(',') {
        Some((_, last)) => last.trim() == "0",
        None => false,
    }
}

/// Statements, paired with the 1-based line their text begins on.
///
/// Splitting on `;` is enough because both banned shapes are single
/// statements; a `;` inside a string literal would only ever split a
/// statement early, which cannot manufacture a violation.
fn statements(source: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut line = 1usize;
    let mut current = String::new();
    let mut start = 1usize;
    for character in source.chars() {
        if character == ';' {
            if !current.trim().is_empty() {
                out.push((start, current.clone()));
            }
            current.clear();
            start = line;
        } else {
            if current.trim().is_empty() && !character.is_whitespace() {
                start = line;
            }
            current.push(character);
        }
        if character == '\n' {
            line += 1;
        }
    }
    if !current.trim().is_empty() {
        out.push((start, current));
    }
    out
}

/// Blanks out `//` and `/* */` comments, preserving line structure so
/// reported line numbers stay true.
fn strip_comments(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut chars = source.char_indices().peekable();
    let mut in_string = false;
    let mut in_char = false;
    let mut escaped = false;
    while let Some((_, character)) = chars.next() {
        if in_string || in_char {
            out.push(character);
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if in_string && character == '"' {
                in_string = false;
            } else if in_char && character == '\'' {
                in_char = false;
            }
            continue;
        }
        match character {
            '"' => {
                in_string = true;
                out.push(character);
            }
            '\'' => {
                in_char = true;
                out.push(character);
            }
            '/' if chars.peek().map(|(_, next)| *next) == Some('/') => {
                for (_, skipped) in chars.by_ref() {
                    if skipped == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            '/' if chars.peek().map(|(_, next)| *next) == Some('*') => {
                chars.next();
                let mut previous = ' ';
                for (_, skipped) in chars.by_ref() {
                    if skipped == '\n' {
                        out.push('\n');
                    }
                    if previous == '*' && skipped == '/' {
                        break;
                    }
                    previous = skipped;
                }
            }
            _ => out.push(character),
        }
    }
    out
}

fn is_identifier(candidate: &str) -> bool {
    !candidate.is_empty()
        && candidate
            .chars()
            .all(|character| character.is_alphanumeric() || character == '_')
        && !candidate.chars().next().is_some_and(|c| c.is_numeric())
}

/// Every first-party Rust source in the workspace.
///
/// Scoped to `crates/` deliberately: the only Rust outside it is the vendored
/// `third_party/` tree, which this repository does not own and must not be
/// held to its test conventions.
fn rust_sources(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![root.join("crates")];
    while let Some(directory) = pending.pop() {
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if path.is_dir() {
                if !SKIPPED_DIRS.contains(&name.as_ref()) {
                    pending.push(path);
                }
            } else if name.ends_with(".rs") {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

/// The workspace root, from this crate's manifest directory.
///
/// Read at runtime rather than through `env!`, which the repository's test
/// taxonomy gate bans in the test tree (rule F2).
fn repository_root() -> PathBuf {
    let manifest = std::env::var_os("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .expect("cargo should expose CARGO_MANIFEST_DIR to an integration test");
    manifest
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .expect("the crate manifest should sit two levels below the workspace root")
}

mod gate_recognises_the_shapes {
    use super::{probe_violations, strip_comments};

    fn scan(source: &str) -> Vec<String> {
        probe_violations(&strip_comments(source))
            .into_iter()
            .map(|(_, shape)| shape)
            .collect()
    }

    #[test]
    fn flags_a_temporary_probe() {
        let hits = scan(
            r#"let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();"#,
        );
        assert_eq!(hits.len(), 1, "expected one hit, got {hits:?}");
    }

    #[test]
    fn flags_a_tuple_address_probe() {
        let hits =
            scan("let port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?.local_addr()?.port();");
        assert_eq!(hits.len(), 1, "expected one hit, got {hits:?}");
    }

    #[test]
    fn flags_a_named_probe_that_is_later_dropped() {
        let hits = scan(
            r#"
            let reservation = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = reservation.local_addr().unwrap().port();
            drop(reservation);
            "#,
        );
        assert_eq!(hits.len(), 1, "expected one hit, got {hits:?}");
    }

    #[test]
    fn accepts_a_listener_that_is_kept() {
        let hits = scan(
            r#"
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            serve(listener, port);
            "#,
        );
        assert!(hits.is_empty(), "kept listener should pass, got {hits:?}");
    }

    #[test]
    fn accepts_a_bind_of_an_explicit_port() {
        let hits = scan(
            r#"let port = TcpListener::bind(("127.0.0.1", window.port(0))).unwrap().local_addr().unwrap().port();"#,
        );
        assert!(hits.is_empty(), "explicit port should pass, got {hits:?}");
    }

    #[test]
    fn ignores_the_shape_inside_a_comment() {
        let hits = scan(
            r#"
            // let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
            /* TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port(); */
            let window = PortWindow::claim();
            "#,
        );
        assert!(hits.is_empty(), "comments should not count, got {hits:?}");
    }
}
