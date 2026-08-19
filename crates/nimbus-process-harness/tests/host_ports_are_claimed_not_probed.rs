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
//! Three stereotyped spellings carry the defect, and all are recognised here
//! by the fact that **the number outlives the socket**:
//!
//! 1. *Temporary probe.* `TcpListener::bind(..)` and `.port()` in one
//!    statement. The listener is an unnamed temporary, so it is dropped at the
//!    semicolon and only the number survives. Keeping a listener requires
//!    naming it, so this spelling is never a false positive.
//! 2. *Named probe, later dropped.* The listener is bound to a name, its port
//!    is read into a variable, the name is passed to `drop`, and the frame
//!    goes on using that variable. Dropping alone is not the defect: a test
//!    that holds a port to force a collision and releases it on the way out
//!    binds nothing afterwards. Using the number after the drop is.
//! 3. *Escaping probe.* A helper binds port zero, reads the number, and
//!    returns it. Nothing calls `drop`, because the frame ending is the drop,
//!    so the caller receives a port that already belongs to nobody.
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
    let scopes = scopes(source);
    // A bind outside any function -- as this gate's own fixtures are written --
    // is judged against the whole file.
    let whole_file = Scope {
        lines: 1..=usize::MAX,
        released: dropped_names(source).into_iter().collect(),
        body: source.to_owned(),
    };

    for (line, statement) in statements(source) {
        let Some(bind) = statement.find("TcpListener::bind(") else {
            continue;
        };
        let arguments = call_arguments(&statement[bind..]);
        if !binds_port_zero(&arguments) {
            continue;
        }

        // Shape 1: the address is read in the same statement that binds it,
        // so whatever the statement names is the *address*, not the listener
        // -- the listener is an unnamed temporary that dies at the semicolon.
        //
        // Both spellings count. `.port()` keeps the bare number, and
        // `.local_addr()` alone keeps a `SocketAddr` that outlives the socket
        // it was read from. Testing only for `.port()` let the second spelling
        // through, because the name the statement binds is the address rather
        // than a listener, so the named-and-dropped check below never sees it
        // either.
        let tail = &statement[bind..];
        if tail.contains(".port()") || tail.contains(".local_addr") {
            found.push((
                line,
                "binds port zero and reads its address in the same statement, so the listener \
                 is a temporary and only the address survives"
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
        let scope = scope_for(&scopes, line).unwrap_or(&whole_file);
        if scope.released.contains(&name) && number_outlives_socket(&scope.body, &name) {
            found.push((
                line,
                format!(
                    "binds port zero into `{name}`, drops it, and goes on using the number, \
                     which belongs to nobody from the drop onwards"
                ),
            ));
        }
    }

    found.extend(escaping_probes(source));
    found.sort_by_key(|(line, _)| *line);
    found
}

/// Shape 3: a helper whose own frame is the release. It binds port zero into a
/// name, reads the address, and returns the number; the listener is dropped
/// when the call returns, so the caller receives a port that belongs to
/// nobody.
///
/// Neither rule above sees it. The statement names the listener, so shape 1
/// does not apply, and nothing ever calls `drop`, because the frame ending
/// *is* the drop. The shape is worth its own rule because it arrives by
/// convergent evolution rather than by copying: several files had
/// independently grown a private helper of exactly this form, under a
/// different name in each.
///
/// Two conditions keep it exact. The signature must hand back a port or an
/// address and not the listener, so the number outlives the socket. And the
/// listener's last mention in the body must be the address read, so nothing is
/// left holding the socket. A helper that moves the listener into a thread and
/// returns where it landed names it again after the read; that is the correct
/// spelling of the same idea, and it is not flagged.
fn escaping_probes(source: &str) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    let mut cursor = 0usize;
    while let Some(at) = source[cursor..].find("fn ") {
        let start = cursor + at;
        cursor = start + "fn ".len();
        let Some((returns, body)) = signature_and_body(&source[start..]) else {
            continue;
        };
        let hands_back_the_listener = returns.contains("TcpListener");
        let hands_back_a_port = returns.contains("u16") || returns.contains("SocketAddr");
        if hands_back_the_listener || !hands_back_a_port {
            continue;
        }

        let text = &source[start + body.start..start + body.end];
        let Some(bind) = text.find("TcpListener::bind(") else {
            continue;
        };
        if !binds_port_zero(&call_arguments(&text[bind..])) {
            continue;
        }
        // The bind's own statement, so `let_binding` sees the `let`.
        let opens = text[..bind]
            .rfind([';', '{', '}'])
            .map_or(0, |index| index + 1);
        let Some(name) = let_binding(&text[opens..]) else {
            continue;
        };

        let flat = flatten_field_access(text);
        let read = format!("{name}.local_addr");
        let Some(read_at) = flat.find(&read) else {
            continue;
        };
        if flat[read_at + read.len()..].contains(&name) {
            // Named again after the read, so something outlives it.
            continue;
        }

        let line = source[..start + body.start + bind].lines().count();
        let helper = function_name(&source[start..]).unwrap_or_else(|| name.clone());
        found.push((
            line,
            format!(
                "`{helper}` binds port zero into `{name}` and returns the number, so the \
                 listener is dropped with the call frame and the port is free before the \
                 caller binds it"
            ),
        ));
    }
    found
}

/// The name of the `fn` beginning at `text`.
fn function_name(text: &str) -> Option<String> {
    let rest = text.strip_prefix("fn ")?.trim_start();
    let name: String = rest
        .chars()
        .take_while(|character| character.is_alphanumeric() || *character == '_')
        .collect();
    is_identifier(&name).then_some(name)
}

/// The return type and body of the `fn` beginning at `text`, as the return
/// type's text and the body's range within `text`.
///
/// `None` for a signature with no return type: it can hand nothing back, so
/// nothing can escape the frame through it.
fn signature_and_body(text: &str) -> Option<(String, std::ops::Range<usize>)> {
    let body = body_range(text)?;
    let head = &text[..body.start - 1];
    let arrow = head.find("->")?;
    Some((head[arrow + "->".len()..].to_owned(), body))
}

/// The brace-balanced body of the `fn` beginning at `text`, as a range within
/// `text`. `None` for a declaration that has no body of its own.
fn body_range(text: &str) -> Option<std::ops::Range<usize>> {
    let open = text.find('{')?;
    // A trait method or a function pointer ends at `;`, so the next `{`
    // belongs to whatever follows it rather than to this signature.
    if text[..open].contains(';') {
        return None;
    }
    let mut depth = 0usize;
    for (index, character) in text[open..].char_indices() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + 1..open + index);
                }
            }
            _ => {}
        }
    }
    None
}

/// One function body, carrying what the named-probe rule asks of it.
struct Scope {
    /// The lines the body spans, so a violation's line finds its own frame.
    lines: std::ops::RangeInclusive<usize>,
    /// Names this body passes to `drop`.
    released: BTreeSet<String>,
    /// The body's own text, read statement by statement to place the drop
    /// against the uses of the number around it.
    body: String,
}

/// Whether the number read from `name` is still in use after `name` is
/// dropped.
///
/// Dropping a listener is not itself the defect. A test that holds a port to
/// force a collision and releases it on the way out is correct: nothing binds
/// that number afterwards, so there is no window to lose. The defect is the
/// frame that drops the socket and then keeps using the number -- from the
/// drop onwards it belongs to nobody, and the next bind of it is a race with
/// every other process on the host.
///
/// Reading the address inline, without giving the number a name, is likewise
/// not the defect: the value is consumed where it is read, so it cannot
/// outlive anything.
fn number_outlives_socket(body: &str, name: &str) -> bool {
    let statements = statements(body);
    let read = format!("{name}.local_addr");
    let Some(carrier) = statements
        .iter()
        .find(|(_, statement)| flatten_field_access(statement).contains(&read))
        .and_then(|(_, statement)| let_binding(statement))
    else {
        return false;
    };
    let Some(released_at) = statements
        .iter()
        .position(|(_, statement)| dropped_names(statement).iter().any(|held| held == name))
    else {
        return false;
    };
    statements[released_at + 1..]
        .iter()
        .any(|(_, statement)| mentions(statement, &carrier))
}

/// Whether `text` uses `ident` as a whole identifier, so `port` does not match
/// inside `port_probe`.
fn mentions(text: &str, ident: &str) -> bool {
    let mut rest = text;
    while let Some(at) = rest.find(ident) {
        let is_word = |character: Option<char>| {
            character.is_some_and(|character| character.is_alphanumeric() || character == '_')
        };
        let before = rest[..at].chars().next_back();
        let after = rest[at + ident.len()..].chars().next();
        if !is_word(before) && !is_word(after) {
            return true;
        }
        rest = &rest[at + ident.len()..];
    }
    false
}

/// Every function body in the file.
///
/// Scope is not a refinement here, it is the rule's correctness. Test files
/// reuse the obvious names: one file binds four separate listeners called
/// `external`, and another drops a production field called `main_listener`
/// nine hundred lines above three unrelated tests that name a local the same
/// thing. Asked file-wide, `drop` marks all of them released and the gate
/// fails correct tests. The question belongs to one frame: does *this*
/// listener's own scope let the number outlive the socket.
fn scopes(source: &str) -> Vec<Scope> {
    let mut scopes = Vec::new();
    let mut cursor = 0usize;
    while let Some(at) = source[cursor..].find("fn ") {
        let start = cursor + at;
        cursor = start + "fn ".len();
        let Some(body) = body_range(&source[start..]) else {
            continue;
        };
        let body = start + body.start..start + body.end;
        let text = &source[body.clone()];
        scopes.push(Scope {
            lines: source[..body.start].lines().count()..=source[..body.end].lines().count(),
            released: dropped_names(text).into_iter().collect(),
            body: text.to_owned(),
        });
    }
    scopes
}

/// The innermost body containing `line`, so a closure or a nested `fn` answers
/// for its own contents rather than deferring to the function around it.
fn scope_for(scopes: &[Scope], line: usize) -> Option<&Scope> {
    scopes
        .iter()
        .filter(|scope| scope.lines.contains(&line))
        .max_by_key(|scope| *scope.lines.start())
}

/// Whitespace around a field access, removed, so a chain that rustfmt broke
/// across lines reads the same as one that fits on a single line.
///
/// Without this, a check written as a plain `name.local_addr` substring stops
/// matching the moment the line grows past the width limit and rustfmt wraps
/// it. The shape would be unchanged and the gate would simply stop seeing it.
fn flatten_field_access(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    for character in source.chars() {
        if character.is_whitespace() {
            if !out.ends_with(' ') && !out.ends_with('.') {
                out.push(' ');
            }
            continue;
        }
        if character == '.' {
            while out.ends_with(' ') {
                out.pop();
            }
        }
        out.push(character);
    }
    out
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
/// Braces end a statement as surely as `;` does. Splitting on `;` alone glues
/// the previous function's closing `}` onto the first statement of the next
/// one, so `let_binding` sees a brace where it expects `let` and every bind
/// that opens a function becomes invisible. Ten real sites hid behind exactly
/// that.
///
/// Every banned shape is a single statement, so an early split -- a `;` or a
/// brace inside a string literal -- can only ever lose context. It cannot
/// manufacture a violation.
fn statements(source: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut line = 1usize;
    let mut current = String::new();
    let mut start = 1usize;
    for character in source.chars() {
        if matches!(character, ';' | '{' | '}') {
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

    // The spelling that slipped past an earlier `.port()`-only test: the
    // statement keeps a `SocketAddr`, never a port number and never a
    // listener, so neither shape recognised it.
    #[test]
    fn flags_a_temporary_probe_that_keeps_only_the_address() {
        let hits = scan(
            r#"let address = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap();"#,
        );
        assert_eq!(hits.len(), 1, "expected one hit, got {hits:?}");
    }

    #[test]
    fn flags_a_named_probe_that_is_later_dropped() {
        let hits = scan(
            r#"
            let reservation = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = reservation.local_addr().unwrap().port();
            drop(reservation);
            serve_on(port);
            "#,
        );
        assert_eq!(hits.len(), 1, "expected one hit, got {hits:?}");
    }

    // The same three lines without the fourth. The listener is held to force a
    // collision and released on the way out, and nothing binds the number
    // afterwards, so there is no window and nothing to fix.
    #[test]
    fn accepts_a_held_port_released_on_the_way_out() {
        let hits = scan(
            r#"
            let external = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = external.local_addr().unwrap();
            assert!(bind_again(address).is_err());
            drop(external);
            "#,
        );
        assert!(hits.is_empty(), "cleanup drop should pass, got {hits:?}");
    }

    // A bind that opens a function, which a `;`-only statement split cannot
    // see: the previous function's `}` lands in front of the `let`.
    #[test]
    fn flags_a_probe_that_opens_a_function() {
        let hits = scan(
            r#"
            fn earlier() {
                assert!(true);
            }

            fn later() {
                let probe = TcpListener::bind("127.0.0.1:0").unwrap();
                let port = probe.local_addr().unwrap().port();
                drop(probe);
                serve_on(port);
            }
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

    // The shape a `.port()`-and-`drop`-only pair of rules cannot see: the
    // helper's own frame is the release.
    #[test]
    fn flags_a_helper_that_returns_a_probed_port() {
        let hits = scan(
            r#"
            fn unused_local_port() -> u16 {
                let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind");
                listener
                    .local_addr()
                    .expect("address")
                    .port()
            }
            "#,
        );
        assert_eq!(hits.len(), 1, "expected one hit, got {hits:?}");
    }

    // Same signature, but the listener moves into the thread that serves it,
    // so the socket outlives the call and the address stays true.
    #[test]
    fn accepts_a_helper_that_moves_the_listener_into_a_thread() {
        let hits = scan(
            r#"
            fn spawn_test_server() -> (SocketAddr, JoinHandle<()>) {
                let listener = TcpListener::bind("127.0.0.1:0").unwrap();
                let address = listener.local_addr().unwrap();
                let handle = thread::spawn(move || serve(listener));
                (address, handle)
            }
            "#,
        );
        assert!(hits.is_empty(), "served listener should pass, got {hits:?}");
    }

    // Two functions in one file, each naming its listener `external`. Only the
    // second releases the port it hands on. Asked file-wide, `drop` would fail
    // the first one too -- which is how real test files are written.
    #[test]
    fn accepts_a_kept_listener_that_shares_a_name_with_a_released_one() {
        let hits = scan(
            r#"
            fn holds_the_port() {
                let external = TcpListener::bind("127.0.0.1:0").unwrap();
                let occupied = external.local_addr().unwrap();
                assert!(TcpListener::bind(occupied).is_err());
            }

            fn releases_the_port() {
                let external = TcpListener::bind("127.0.0.1:0").unwrap();
                let port = external.local_addr().unwrap().port();
                drop(external);
                rebind(port);
            }
            "#,
        );
        assert_eq!(
            hits.len(),
            1,
            "only the released one should flag, got {hits:?}"
        );
    }

    // Rustfmt wraps a chain as soon as it is long enough. The rule has to read
    // the wrapped spelling as the same shape, or it stops seeing a defect that
    // only grew a few characters. Same defect as the unwrapped case above,
    // rebind included, so the only difference under test is the line breaks.
    #[test]
    fn flags_a_named_probe_whose_chain_is_wrapped() {
        let hits = scan(
            r#"
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener
                .local_addr()
                .unwrap()
                .port();
            drop(listener);
            rebind(port);
            "#,
        );
        assert_eq!(hits.len(), 1, "expected one hit, got {hits:?}");
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
