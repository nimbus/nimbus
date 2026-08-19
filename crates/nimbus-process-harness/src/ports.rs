//! Cross-process host-port windows for deterministic test harnesses.
//!
//! A test that must hand a concrete host port to the code under test cannot
//! discover one by binding `127.0.0.1:0`, reading the assigned port, and
//! closing the socket. Between that close and the real bind the port belongs
//! to nobody, so any other test process can take it. The race is not
//! theoretical: it reached CI as
//! `failed to bind egress proxy on 127.0.0.1:38373: address in use`.
//!
//! Widening the caller's range does not help either. The sandbox port
//! coordinator walks its configured range lowest-first and checks candidates
//! only against its own durable state, which every test roots inside its own
//! temporary directory. Two test processes sharing one range therefore agree
//! on the same first candidate, turning a rare collision into a certain one.
//!
//! Two properties make a window here exclusive rather than merely lucky.
//!
//! **The claim is a socket, not a number.** A window is claimed by binding its
//! first port and *keeping* that listener for as long as the window lives. The
//! kernel refuses a second bind of the same address, so the claim holds across
//! processes with no lock file, and it is released by process exit — including
//! a crash or a kill — with no state to reap.
//!
//! **The region sits below the ephemeral range.** `bind(0)` draws from the
//! host's ephemeral range, so a window carved below that range can never be
//! handed to an unrelated `bind(0)`. That is the property the old probe
//! violated, and [`tests::ephemeral_range_never_overlaps_the_region`] proves
//! it against the running host rather than trusting documented defaults.
//!
//! The claim covers the window's first port, so exclusion against *other
//! claims* rests on that one socket. It does not bind the rest of the host:
//! an unrelated program that already listens inside a window still holds that
//! port, and a caller that takes it fails loudly rather than silently sharing
//! it. [`RESERVED_PORTS`] withholds the windows where that is predictable —
//! the conventional Nimbus ports a developer's own server would occupy.
//!
//! An unrelated program that takes a *sentinel* costs a claim its first choice,
//! not its correctness: [`PortWindow::try_claim`] walks on to the next window.

use std::io;
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener};
use std::ops::RangeInclusive;
use std::sync::atomic::{AtomicU32, Ordering};

/// First port of the claimable region.
///
/// Above the sandbox backends' default published range (15000-16000) so a
/// window never collides with a fixture that deliberately exercises the
/// production default, and far below the lowest ephemeral start of any
/// supported host (32768 on Linux, 49152 on macOS).
const REGION_START: u16 = 17_000;

/// Last port of the claimable region, inclusive.
const REGION_END: u16 = 31_999;

/// Ports inside the region that a running Nimbus may already own.
///
/// A claim holds only its window's sentinel, so a program that binds *inside*
/// a window still collides with a claimed port. Nimbus serves its MongoDB
/// adapter on the conventional 27017, which falls in the region, so the window
/// containing it is never handed out and a developer can run the server beside
/// the suite. The other conventional ports (8000 DynamoDB, 9000 S3, and the
/// 15000-16000 published sandbox range) already sit below the region.
const RESERVED_PORTS: &[u16] = &[27_017];

/// Ports per window, sentinel included.
///
/// 32 leaves 31 usable, which covers the widest existing fixture — a
/// twenty-one port publication window — with room to spare.
const WINDOW_LEN: u16 = 32;

/// Rotates where each claim starts scanning, so concurrent claimers do not all
/// walk the region from the same end and queue behind the same busy windows.
static SCAN_ROTATION: AtomicU32 = AtomicU32::new(0);

/// Whether the window opening at `start` spans a reserved port.
fn window_is_reserved(start: u16) -> bool {
    let span = start..start + WINDOW_LEN;
    RESERVED_PORTS.iter().any(|port| span.contains(port))
}

/// Number of whole windows the region holds.
const fn window_count() -> u16 {
    (REGION_END - REGION_START + 1) / WINDOW_LEN
}

/// A block of host ports held exclusively by this process.
///
/// Dropping the window releases the claim. Keep it alive for exactly as long
/// as the ports matter — binding it to a `let _ = ...` drops it immediately
/// and gives up the exclusion.
#[derive(Debug)]
pub struct PortWindow {
    /// Held for the window's lifetime. This socket *is* the claim.
    sentinel: TcpListener,
    start: u16,
}

impl PortWindow {
    /// Claim a window, panicking with context when the region is exhausted.
    #[must_use]
    pub fn claim() -> Self {
        match Self::try_claim() {
            Ok(window) => window,
            Err(error) => panic!("{error}"),
        }
    }

    /// Claim a window, reporting exhaustion instead of panicking.
    ///
    /// # Errors
    ///
    /// Returns [`io::ErrorKind::AddrInUse`] when every window in the region is
    /// already claimed.
    pub fn try_claim() -> io::Result<Self> {
        let count = u32::from(window_count());
        // Both terms matter: the counter separates repeat claims inside one
        // process, and the pid separates processes that start together.
        let rotation = SCAN_ROTATION
            .fetch_add(1, Ordering::Relaxed)
            .wrapping_add(std::process::id());
        let mut last_error: Option<io::Error> = None;
        for step in 0..count {
            let index = rotation.wrapping_add(step) % count;
            let offset = index * u32::from(WINDOW_LEN);
            // `index < count`, so the offset is bounded by the region and the
            // conversion cannot fail. Loud rather than silent if that ever
            // stops holding: a fallback offset would hand out a window that
            // another claim already owns.
            let offset =
                u16::try_from(offset).expect("a window offset should stay inside the region");
            let start = REGION_START + offset;
            if window_is_reserved(start) {
                continue;
            }
            match TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, start)) {
                Ok(sentinel) => return Ok(Self { sentinel, start }),
                Err(error) => last_error = Some(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AddrInUse,
            format!(
                "every claimable test port window in {REGION_START}-{REGION_END} is taken \
                 ({count} windows of {WINDOW_LEN} ports, less those spanning \
                 {RESERVED_PORTS:?}); last bind failed with: {}",
                last_error.map_or_else(
                    || "no candidate was attempted".to_owned(),
                    |error| error.to_string()
                )
            ),
        ))
    }

    /// A contiguous sub-range of `len` usable ports starting at `offset`.
    ///
    /// A caller that needs both a range and single ports must partition the
    /// window explicitly — there is deliberately no accessor for "the whole
    /// window", because handing out the full span and a single port from it
    /// is how a fixture collides with itself.
    ///
    /// # Panics
    ///
    /// Panics when the requested span runs past the window, which is a test
    /// asking for more ports than it claimed rather than a runtime condition.
    #[must_use]
    pub fn ports(&self, offset: u16, len: u16) -> RangeInclusive<u16> {
        assert!(len > 0, "a port span needs at least one port");
        let end = offset
            .checked_add(len - 1)
            .expect("port span should not overflow");
        assert!(
            end < self.usable(),
            "port span {offset}..={end} is past the {} usable ports in window {}-{}",
            self.usable(),
            self.start + 1,
            self.start + WINDOW_LEN - 1
        );
        self.port(offset)..=self.port(end)
    }

    /// One usable port, addressed from the start of the window.
    ///
    /// # Panics
    ///
    /// Panics when `offset` is past the end of the window, which is a test
    /// asking for more ports than it claimed rather than a runtime condition.
    #[must_use]
    pub fn port(&self, offset: u16) -> u16 {
        assert!(
            offset < self.usable(),
            "port offset {offset} is past the {} usable ports in window {}-{}",
            self.usable(),
            self.start + 1,
            self.start + WINDOW_LEN - 1
        );
        self.start + 1 + offset
    }

    /// How many usable ports the window carries.
    #[must_use]
    pub const fn usable(&self) -> u16 {
        WINDOW_LEN - 1
    }

    /// The held port that *is* this window's claim.
    ///
    /// Exposed so a test can prove the exclusion rather than assume it.
    #[must_use]
    pub fn sentinel_port(&self) -> u16 {
        self.sentinel
            .local_addr()
            .map_or(self.start, |address| address.port())
    }
}

#[cfg(test)]
mod tests {
    use super::{PortWindow, REGION_END, REGION_START, RESERVED_PORTS, WINDOW_LEN, window_count};
    use std::net::{Ipv4Addr, SocketAddrV4, TcpListener};
    use std::ops::RangeInclusive;
    use std::process::Command;

    /// Carries the held sentinel port to the child role below.
    const PROBE_ENV: &str = "NIMBUS_PORT_WINDOW_PROBE";
    const CHILD_TEST: &str = "ports::tests::child_role_cannot_bind_a_held_sentinel";

    /// The whole usable span. Only the tests below want the window as one
    /// range; callers partition it deliberately through [`PortWindow::ports`].
    fn whole(window: &PortWindow) -> RangeInclusive<u16> {
        window.ports(0, window.usable())
    }

    #[test]
    fn concurrent_windows_never_overlap() {
        let windows: Vec<PortWindow> = (0..8).map(|_| PortWindow::claim()).collect();
        for (index, window) in windows.iter().enumerate() {
            for other in &windows[index + 1..] {
                let disjoint = whole(window).end() < whole(other).start()
                    || whole(other).end() < whole(window).start();
                assert!(
                    disjoint,
                    "windows {:?} and {:?} overlap",
                    whole(window),
                    whole(other)
                );
            }
        }
    }

    #[test]
    fn every_usable_port_in_a_claimed_window_binds() {
        // The claim covers the sentinel, not the interior, so an unrelated
        // program on this host can occupy a port inside one window. It cannot
        // occupy one inside every window, which is what separates that from
        // the failure this asserts against: a window that overlaps another
        // claim would block a port in each fresh attempt too.
        const ATTEMPTS: u8 = 5;
        let mut blocked = None;
        for _ in 0..ATTEMPTS {
            let window = PortWindow::claim();
            // Held together rather than one at a time: a port freed between
            // two binds could be re-handed by the kernel and hide an overlap.
            let mut held: Vec<TcpListener> = Vec::with_capacity(usize::from(window.usable()));
            for port in whole(&window) {
                match TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port)) {
                    Ok(listener) => held.push(listener),
                    Err(error) => {
                        blocked = Some((port, error));
                        break;
                    }
                }
            }
            if held.len() == usize::from(window.usable()) {
                return;
            }
        }
        let (port, error) = blocked.expect("a short attempt records the port that blocked it");
        panic!(
            "no claimed window bound cleanly in {ATTEMPTS} attempts; \
             port {port} failed with: {error}"
        );
    }

    #[test]
    fn the_usable_range_excludes_the_sentinel() {
        let window = PortWindow::claim();
        assert_eq!(*whole(&window).start(), window.sentinel_port() + 1);
        assert_eq!(
            *whole(&window).end(),
            window.sentinel_port() + WINDOW_LEN - 1
        );
        assert_eq!(whole(&window).count(), usize::from(window.usable()));
    }

    #[test]
    fn sub_ranges_partition_the_window_without_overlapping() {
        let window = PortWindow::claim();
        let first = window.ports(0, 4);
        let second = window.ports(4, 4);
        assert_eq!(first, window.port(0)..=window.port(3));
        assert_eq!(second, window.port(4)..=window.port(7));
        assert!(
            first.end() < second.start(),
            "{first:?} overlaps {second:?}"
        );
    }

    #[test]
    #[should_panic(expected = "is past the")]
    fn a_sub_range_past_the_window_is_rejected() {
        let window = PortWindow::claim();
        let _ = window.ports(window.usable() - 1, 2);
    }

    #[test]
    fn port_offsets_address_the_usable_range() {
        let window = PortWindow::claim();
        assert_eq!(window.port(0), *whole(&window).start());
        assert_eq!(window.port(window.usable() - 1), *whole(&window).end());
    }

    #[test]
    #[should_panic(expected = "is past the")]
    fn a_port_offset_past_the_window_is_rejected() {
        let window = PortWindow::claim();
        let _ = window.port(window.usable());
    }

    #[test]
    fn dropping_a_window_releases_its_claim() {
        // A released window is immediately claimable again, so a concurrent
        // claimant walking the region can take this sentinel before the
        // re-bind below reaches it. That outcome proves the release just as
        // well, so it retries with a fresh window instead of failing. A
        // sentinel that outlived its window would instead lose every attempt.
        const ATTEMPTS: u8 = 8;
        let mut last_error = None;
        for _ in 0..ATTEMPTS {
            let sentinel = {
                let window = PortWindow::claim();
                window.sentinel_port()
            };
            match TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, sentinel)) {
                Ok(_) => return,
                Err(error) => last_error = Some(error),
            }
        }
        panic!(
            "a dropped window should release its sentinel, but {ATTEMPTS} windows \
             stayed bound; last bind failed with: {}",
            last_error.expect("a failed attempt records its error")
        );
    }

    /// A claim holds only its sentinel, so a conventional port inside a handed
    /// out window would let a locally running Nimbus server collide with a
    /// claimed port and fail an unrelated test from inside product code.
    #[test]
    fn no_conventional_nimbus_port_falls_in_a_claimable_window() {
        // Spelled out rather than imported: this crate sits below the adapters
        // and must not depend on them to state its own region invariant.
        for (port, name) in [
            (8000_u16, "DynamoDB"),
            (9000, "S3"),
            (15_000, "published sandbox range start"),
            (16_000, "published sandbox range end"),
            (27_017, "MongoDB"),
        ] {
            let outside = !(REGION_START..=REGION_END).contains(&port);
            assert!(
                outside || RESERVED_PORTS.contains(&port),
                "conventional {name} port {port} sits in the claimable region \
                 {REGION_START}-{REGION_END} without a RESERVED_PORTS entry"
            );
        }
    }

    #[test]
    fn a_reserved_port_is_never_inside_a_claimed_window() {
        let windows: Vec<PortWindow> = (0..16).map(|_| PortWindow::claim()).collect();
        for window in &windows {
            for reserved in RESERVED_PORTS {
                assert!(
                    !whole(window).contains(reserved) && *reserved != window.sentinel_port(),
                    "claimed window {:?} spans reserved port {reserved}",
                    whole(window)
                );
            }
        }
    }

    #[test]
    fn the_region_divides_into_whole_windows() {
        let count = window_count();
        assert!(count > 0, "the region must hold at least one window");
        let last_start = REGION_START + (count - 1) * WINDOW_LEN;
        assert!(
            last_start + WINDOW_LEN - 1 <= REGION_END,
            "window {count} would run past the region end"
        );
    }

    /// The property the whole design rests on: a window claimed here is
    /// unavailable to a *separate process*, not merely to this one.
    #[test]
    fn a_claimed_sentinel_excludes_another_process() {
        let window = PortWindow::claim();
        let status =
            Command::new(std::env::current_exe().expect("current test executable should resolve"))
                .arg("--exact")
                .arg(CHILD_TEST)
                .arg("--ignored")
                .arg("--nocapture")
                .env(PROBE_ENV, window.sentinel_port().to_string())
                .status()
                .expect("child role should spawn");
        assert!(
            status.success(),
            "child process bound sentinel {} that this process holds",
            window.sentinel_port()
        );
    }

    #[test]
    #[ignore = "child role, driven by a_claimed_sentinel_excludes_another_process"]
    fn child_role_cannot_bind_a_held_sentinel() {
        let Some(raw) = std::env::var_os(PROBE_ENV) else {
            // Reached by a plain `--ignored` run with no parent. Nothing to
            // prove, and nothing to fail.
            return;
        };
        let port: u16 = raw
            .to_string_lossy()
            .parse()
            .expect("probe port should parse");
        let outcome = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port));
        assert!(
            outcome.is_err(),
            "bound sentinel {port} while another process holds it"
        );
    }

    /// Proves the region sits below the host's ephemeral range instead of
    /// trusting the documented default, because a host tuned otherwise would
    /// reintroduce exactly the collision this module exists to remove.
    #[test]
    fn ephemeral_range_never_overlaps_the_region() {
        let Some((first, last)) = ephemeral_range() else {
            return;
        };
        assert!(
            REGION_END < first,
            "test port region {REGION_START}-{REGION_END} overlaps this host's \
             ephemeral range {first}-{last}; bind(0) elsewhere could be handed \
             a port inside a claimed window"
        );
    }

    /// The host's ephemeral range, read from the running kernel.
    ///
    /// Split per platform rather than branched inside one body: whichever
    /// `cfg` block compiled last would own the function's tail expression, so
    /// a single body cannot spell its returns in a way that satisfies
    /// `clippy::needless_return` on both hosts at once.
    #[cfg(target_os = "linux")]
    fn ephemeral_range() -> Option<(u16, u16)> {
        let raw = std::fs::read_to_string("/proc/sys/net/ipv4/ip_local_port_range").ok()?;
        let mut parts = raw.split_whitespace();
        Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
    }

    /// The host's ephemeral range, read from the running kernel.
    #[cfg(target_os = "macos")]
    fn ephemeral_range() -> Option<(u16, u16)> {
        let read = |name: &str| -> Option<u16> {
            let output = Command::new("/usr/sbin/sysctl")
                .arg("-n")
                .arg(name)
                .output()
                .ok()?;
            String::from_utf8(output.stdout).ok()?.trim().parse().ok()
        };
        Some((
            read("net.inet.ip.portrange.first")?,
            read("net.inet.ip.portrange.last")?,
        ))
    }

    /// `None` on a platform this test does not know how to interrogate, which
    /// skips the assertion rather than failing it.
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    fn ephemeral_range() -> Option<(u16, u16)> {
        None
    }
}
