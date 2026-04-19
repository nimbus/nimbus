#[cfg(any(unix, test))]
use std::io::IsTerminal;
#[cfg(any(unix, test))]
use std::io::Read;
use std::io::{self, Write};
#[cfg(any(unix, test))]
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
#[cfg(any(unix, test))]
use std::task::{Context, Poll};

use comfy_table::{
    Cell, CellAlignment, ColumnConstraint, ContentArrangement, Table, Width, presets::NOTHING,
};
#[cfg(any(unix, test))]
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
#[cfg(any(unix, test))]
use tokio::io::AsyncWrite;

pub(crate) const ROOT_HELP_TEMPLATE: &str = "\
{before-help}{about-with-newline}\
Usage:\n  {usage}\n\n\
{all-args}{after-help}";

pub(crate) const COMMAND_GROUP_HELP_TEMPLATE: &str = "\
{before-help}{about-with-newline}\
Usage:\n  {usage}\n\n\
{all-args}{after-help}";

pub(crate) const COMMAND_HELP_TEMPLATE: &str = "\
{before-help}{about-with-newline}\
Usage:\n  {usage}\n\n\
{all-args}{subcommands}{after-help}";

pub(crate) const ROOT_HELP_EXAMPLES: &str = "\
Examples:
  neovex serve
  neovex machine start
  neovex service up";

pub(crate) const SERVE_HELP_EXAMPLES: &str = "\
Examples:
  neovex serve
  neovex serve --compose-file ./compose.yaml
  neovex serve --tenant-provider postgres --postgres-url postgres://localhost/neovex";

pub(crate) const MACHINE_HELP_EXAMPLES: &str = "\
Examples:
  neovex machine init --now
  neovex machine status -f json
  neovex machine ssh";

pub(crate) const MACHINE_OS_HELP_EXAMPLES: &str = "\
Examples:
  neovex machine os upgrade --dry-run
  neovex machine os apply docker://quay.io/podman/machine-os@sha256:<digest>";

pub(crate) const MACHINE_INIT_HELP_EXAMPLES: &str = "\
Examples:
  neovex machine init --now
  neovex machine init --cpus 4 --memory 4096 team-a";

pub(crate) const MACHINE_START_HELP_EXAMPLES: &str = "\
Examples:
  neovex machine start
  neovex machine start --cpus 4 --memory 4096 team-a
  neovex machine start --quiet team-a";

pub(crate) const MACHINE_STOP_HELP_EXAMPLES: &str = "\
Examples:
  neovex machine stop
  neovex machine stop team-a";

pub(crate) const MACHINE_STATUS_HELP_EXAMPLES: &str = "\
Examples:
  neovex machine status
  neovex machine status -f json
  neovex machine status --noheading
  neovex machine status --quiet";

pub(crate) const MACHINE_LIST_HELP_EXAMPLES: &str = "\
Examples:
  neovex machine list
  neovex machine list --noheading
  neovex machine ls --quiet
  neovex machine list -f json";

pub(crate) const MACHINE_INFO_HELP_EXAMPLES: &str = "\
Examples:
  neovex machine info
  neovex machine info -f json
  neovex machine info -f yaml";

pub(crate) const MACHINE_INSPECT_HELP_EXAMPLES: &str = "\
Examples:
  neovex machine inspect
  neovex machine inspect -f yaml team-a";

pub(crate) const MACHINE_SET_HELP_EXAMPLES: &str = "\
Examples:
  neovex machine set --cpus 4 --memory 4096
  neovex machine set --disk-size 40 team-a";

pub(crate) const MACHINE_CP_HELP_EXAMPLES: &str = "\
Examples:
  neovex machine cp ./local.txt default:/tmp/remote.txt
  neovex machine cp team-a:/var/log/cloud-init-output.log ./cloud-init.log";

pub(crate) const MACHINE_SSH_HELP_EXAMPLES: &str = "\
Examples:
  neovex machine ssh
  neovex machine ssh team-a uname -a";

pub(crate) const MACHINE_RM_HELP_EXAMPLES: &str = "\
Examples:
  neovex machine rm
  neovex machine rm team-a";

pub(crate) const MACHINE_OS_APPLY_HELP_EXAMPLES: &str = "\
Examples:
  neovex machine os apply docker://quay.io/podman/machine-os@sha256:<digest>
  neovex machine os apply docker://ghcr.io/agentstation/neovex-machine-os:v0.1.19 --restart";

pub(crate) const MACHINE_OS_UPGRADE_HELP_EXAMPLES: &str = "\
Examples:
  neovex machine os upgrade --dry-run
  neovex machine os upgrade --restart";

pub(crate) const SERVICE_HELP_EXAMPLES: &str = "\
Examples:
  neovex service config
  neovex service up
  neovex service logs api --follow";

pub(crate) const SERVICE_CONFIG_HELP_EXAMPLES: &str = "\
Examples:
  neovex service config
  neovex service config --file ./compose.dev.yaml
  neovex service config --services";

pub(crate) const SERVICE_UP_HELP_EXAMPLES: &str = "\
Examples:
  neovex service up
  neovex service up api
  neovex service up --tenant demo";

pub(crate) const SERVICE_DOWN_HELP_EXAMPLES: &str = "\
Examples:
  neovex service down
  neovex service down api
  neovex service down --tenant demo";

pub(crate) const SERVICE_LIST_HELP_EXAMPLES: &str = "\
Examples:
  neovex service list
  neovex service list --all-tenants
  neovex service list --noheading
  neovex service list -f json";

pub(crate) const SERVICE_INSPECT_HELP_EXAMPLES: &str = "\
Examples:
  neovex service inspect api
  neovex service inspect api --tenant demo
  neovex service inspect api -f yaml";

pub(crate) const SERVICE_LOGS_HELP_EXAMPLES: &str = "\
Examples:
  neovex service logs api
  neovex service logs api --follow";

pub(crate) const SERVICE_PS_HELP_EXAMPLES: &str = "\
Examples:
  neovex service ps api
  neovex service ps api --noheading
  neovex service ps api --tenant demo
  neovex service ps api -f json";

const SUPPRESS_PHASE_OUTPUT: usize = 1 << 0;
const SUPPRESS_INFO_OUTPUT: usize = 1 << 1;
const SUPPRESS_PROGRESS_OUTPUT: usize = 1 << 2;

static OUTPUT_MODE_FLAGS: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct OutputMode {
    pub(crate) suppress_phase: bool,
    pub(crate) suppress_info: bool,
    pub(crate) suppress_progress: bool,
}

impl OutputMode {
    fn flags(self) -> usize {
        let mut flags = 0;
        if self.suppress_phase {
            flags |= SUPPRESS_PHASE_OUTPUT;
        }
        if self.suppress_info {
            flags |= SUPPRESS_INFO_OUTPUT;
        }
        if self.suppress_progress {
            flags |= SUPPRESS_PROGRESS_OUTPUT;
        }
        flags
    }
}

pub(crate) struct OutputModeGuard {
    previous_flags: usize,
    _lock: MutexGuard<'static, ()>,
}

impl Drop for OutputModeGuard {
    fn drop(&mut self) {
        OUTPUT_MODE_FLAGS.store(self.previous_flags, Ordering::SeqCst);
    }
}

pub(crate) fn push_output_mode(mode: OutputMode) -> OutputModeGuard {
    let lock = output_mode_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let previous_flags = OUTPUT_MODE_FLAGS.swap(mode.flags(), Ordering::SeqCst);
    OutputModeGuard {
        previous_flags,
        _lock: lock,
    }
}

#[cfg(any(unix, test))]
pub(crate) fn stderr_is_tty() -> bool {
    io::stderr().is_terminal()
}

#[cfg(any(unix, test))]
pub(crate) fn info_output_enabled() -> bool {
    output_mode_flags() & SUPPRESS_INFO_OUTPUT == 0
}

pub(crate) fn write_stdout(rendered: &str) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    stdout.write_all(rendered.as_bytes())?;
    stdout.flush()
}

pub(crate) fn write_stdout_line(line: &str) -> io::Result<()> {
    write_stdout(&format!("{line}\n"))
}

pub(crate) fn write_stderr_line(line: &str) -> io::Result<()> {
    let mut stderr = io::stderr().lock();
    stderr.write_all(line.as_bytes())?;
    stderr.write_all(b"\n")?;
    stderr.flush()
}

pub(crate) fn write_stderr_prefixed_line(prefix: &str, message: &str) -> io::Result<()> {
    write_stderr_line(&format!("{prefix} {message}"))
}

#[cfg(any(unix, test))]
pub(crate) fn emit_phase(message: &str) -> io::Result<()> {
    if stderr_is_tty() && phase_output_enabled() {
        write_stderr_prefixed_line("==>", message)
    } else {
        Ok(())
    }
}

pub(crate) fn format_action_summary(summary: &str) -> String {
    format!("{summary}\n")
}

pub(crate) fn format_action_block(summary: &str, detail_lines: &[String]) -> String {
    let mut rendered = format_action_summary(summary);
    for line in detail_lines {
        rendered.push_str(line);
        rendered.push('\n');
    }
    rendered
}

pub(crate) fn format_hint(message: &str) -> String {
    format!("Hint: {message}")
}

#[cfg(any(unix, test))]
pub(crate) struct ByteProgress {
    message: String,
    bar: Option<ProgressBar>,
    finished: bool,
}

#[cfg(any(unix, test))]
impl ByteProgress {
    pub(crate) fn new(message: impl Into<String>, total_bytes: Option<u64>) -> io::Result<Self> {
        let message = message.into();
        if !stderr_is_tty() || !progress_output_enabled() {
            return Ok(Self {
                message,
                bar: None,
                finished: false,
            });
        }

        let bar = match total_bytes.filter(|bytes| *bytes > 0) {
            Some(total_bytes) => {
                let bar =
                    ProgressBar::with_draw_target(Some(total_bytes), ProgressDrawTarget::stderr());
                bar.set_style(progress_bar_style());
                bar.set_message(message.clone());
                Some(bar)
            }
            None => {
                emit_phase(&message)?;
                None
            }
        };

        Ok(Self {
            message,
            bar,
            finished: false,
        })
    }

    pub(crate) fn wrap_read<R>(&self, reader: R) -> ProgressRead<R> {
        ProgressRead {
            inner: reader,
            bar: self.bar.clone(),
        }
    }

    pub(crate) fn wrap_async_write<W>(&self, writer: W) -> ProgressAsyncWrite<W> {
        ProgressAsyncWrite {
            inner: writer,
            bar: self.bar.clone(),
        }
    }

    pub(crate) fn finish(&mut self) {
        if let Some(bar) = &self.bar {
            bar.finish_with_message(format!("{}: done", self.message));
        }
        self.finished = true;
    }
}

#[cfg(any(unix, test))]
impl Drop for ByteProgress {
    fn drop(&mut self) {
        if !self.finished {
            if let Some(bar) = &self.bar {
                bar.abandon();
            }
        }
    }
}

#[cfg(any(unix, test))]
fn progress_bar_style() -> ProgressStyle {
    ProgressStyle::with_template(
        "{msg:30} [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} {bytes_per_sec} eta {eta}",
    )
    .expect("progress bar template should be valid")
    .progress_chars("=> ")
}

#[cfg(any(unix, test))]
pub(crate) struct ProgressRead<R> {
    inner: R,
    bar: Option<ProgressBar>,
}

#[cfg(any(unix, test))]
impl<R: Read> Read for ProgressRead<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let read = self.inner.read(buf)?;
        if read > 0 {
            if let Some(bar) = &self.bar {
                bar.inc(read as u64);
            }
        }
        Ok(read)
    }
}

#[cfg(any(unix, test))]
pub(crate) struct ProgressAsyncWrite<W> {
    inner: W,
    bar: Option<ProgressBar>,
}

#[cfg(any(unix, test))]
impl<W: AsyncWrite + Unpin> AsyncWrite for ProgressAsyncWrite<W> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match Pin::new(&mut self.inner).poll_write(cx, buf) {
            Poll::Ready(Ok(written)) => {
                if written > 0 {
                    if let Some(bar) = &self.bar {
                        bar.inc(written as u64);
                    }
                }
                Poll::Ready(Ok(written))
            }
            other => other,
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TableAlignment {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TableColumn<'a> {
    pub(crate) header: &'a str,
    pub(crate) alignment: TableAlignment,
    pub(crate) min_width: usize,
}

impl<'a> TableColumn<'a> {
    pub(crate) const fn left(header: &'a str, min_width: usize) -> Self {
        Self {
            header,
            alignment: TableAlignment::Left,
            min_width,
        }
    }

    pub(crate) const fn right(header: &'a str, min_width: usize) -> Self {
        Self {
            header,
            alignment: TableAlignment::Right,
            min_width,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct TableRenderOptions {
    pub(crate) omit_header: bool,
}

pub(crate) fn render_table_with_options(
    columns: &[TableColumn<'_>],
    rows: &[Vec<String>],
    options: TableRenderOptions,
) -> String {
    if columns.is_empty() {
        return String::new();
    }
    if options.omit_header && rows.is_empty() {
        return String::new();
    }

    let mut table = Table::new();
    table
        .load_preset(NOTHING)
        .set_content_arrangement(ContentArrangement::Disabled)
        .force_no_tty();
    if !options.omit_header {
        table.set_header(
            columns
                .iter()
                .map(|column| {
                    Cell::new(column.header).set_alignment(cell_alignment(column.alignment))
                })
                .collect::<Vec<_>>(),
        );
    }
    for row in rows {
        debug_assert_eq!(
            row.len(),
            columns.len(),
            "table rows should match the number of columns"
        );
        table.add_row(
            row.iter()
                .enumerate()
                .map(|(index, value)| {
                    Cell::new(value.as_str())
                        .set_alignment(cell_alignment(columns[index].alignment))
                })
                .collect::<Vec<_>>(),
        );
    }

    for (index, column) in columns.iter().enumerate() {
        let table_column = table
            .column_mut(index)
            .expect("table column should exist for every declared column");
        table_column.set_padding((0, 1));
        table_column.set_constraint(ColumnConstraint::LowerBoundary(Width::Fixed(
            column.min_width as u16,
        )));
        table_column.set_cell_alignment(cell_alignment(column.alignment));
    }

    let mut rendered = table.trim_fmt();
    rendered.push('\n');
    rendered
}

fn cell_alignment(alignment: TableAlignment) -> CellAlignment {
    match alignment {
        TableAlignment::Left => CellAlignment::Left,
        TableAlignment::Right => CellAlignment::Right,
    }
}

fn output_mode_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[cfg(any(unix, test))]
fn output_mode_flags() -> usize {
    OUTPUT_MODE_FLAGS.load(Ordering::SeqCst)
}

#[cfg(any(unix, test))]
fn phase_output_enabled() -> bool {
    output_mode_flags() & SUPPRESS_PHASE_OUTPUT == 0
}

#[cfg(any(unix, test))]
fn progress_output_enabled() -> bool {
    output_mode_flags() & SUPPRESS_PROGRESS_OUTPUT == 0
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Read};

    use tokio::io::{AsyncWriteExt, sink};

    use super::{
        ByteProgress, OutputMode, TableColumn, TableRenderOptions, format_action_block,
        format_action_summary, format_hint, info_output_enabled, phase_output_enabled,
        progress_output_enabled, push_output_mode, render_table_with_options,
    };

    #[test]
    fn format_action_summary_appends_newline() {
        assert_eq!(
            format_action_summary("Machine \"default\" started successfully"),
            "Machine \"default\" started successfully\n"
        );
    }

    #[test]
    fn format_action_block_appends_detail_lines() {
        assert_eq!(
            format_action_block(
                "Machine \"default\" machine OS upgraded successfully",
                &[
                    "Image: docker://example.com/image@sha256:abc123".to_owned(),
                    format_hint("run `neovex machine start` to boot the updated image"),
                ]
            ),
            "Machine \"default\" machine OS upgraded successfully\n\
Image: docker://example.com/image@sha256:abc123\n\
Hint: run `neovex machine start` to boot the updated image\n"
        );
    }

    #[test]
    fn output_mode_guard_sets_and_restores_flags() {
        assert!(phase_output_enabled());
        assert!(info_output_enabled());
        assert!(progress_output_enabled());

        {
            let _guard = push_output_mode(OutputMode {
                suppress_phase: true,
                suppress_info: true,
                suppress_progress: true,
            });
            assert!(!phase_output_enabled());
            assert!(!info_output_enabled());
            assert!(!progress_output_enabled());
        }

        assert!(phase_output_enabled());
        assert!(info_output_enabled());
        assert!(progress_output_enabled());
    }

    #[test]
    fn render_table_honors_alignment_and_minimum_widths() {
        let columns = [TableColumn::left("NAME", 8), TableColumn::right("CPUS", 4)];
        let rows = vec![
            vec!["default".to_owned(), "2".to_owned()],
            vec!["team-a".to_owned(), "16".to_owned()],
        ];

        let rendered = render_table_with_options(&columns, &rows, TableRenderOptions::default());
        let mut lines = rendered.lines();

        assert_eq!(lines.next(), Some("NAME    CPUS"));
        assert_eq!(lines.next(), Some("default    2"));
        assert_eq!(lines.next(), Some("team-a    16"));
        assert_eq!(lines.next(), None);
    }

    #[test]
    fn render_table_preserves_header_for_empty_rows() {
        let columns = [TableColumn::left("NAME", 8), TableColumn::right("CPUS", 4)];
        let rendered = render_table_with_options(&columns, &[], TableRenderOptions::default());
        let mut lines = rendered.lines();

        assert_eq!(lines.next(), Some("NAME    CPUS"));
        assert_eq!(lines.next(), None);
    }

    #[test]
    fn render_table_can_omit_header() {
        let columns = [TableColumn::left("NAME", 8), TableColumn::right("CPUS", 4)];
        let rows = vec![
            vec!["default".to_owned(), "2".to_owned()],
            vec!["team-a".to_owned(), "16".to_owned()],
        ];

        let rendered =
            render_table_with_options(&columns, &rows, TableRenderOptions { omit_header: true });
        let mut lines = rendered.lines();

        assert_eq!(
            lines
                .next()
                .map(str::split_whitespace)
                .map(Iterator::collect::<Vec<_>>),
            Some(vec!["default", "2"])
        );
        assert_eq!(
            lines
                .next()
                .map(str::split_whitespace)
                .map(Iterator::collect::<Vec<_>>),
            Some(vec!["team-a", "16"])
        );
        assert_eq!(lines.next(), None);
    }

    #[test]
    fn render_table_omitting_header_with_no_rows_returns_empty_output() {
        let columns = [TableColumn::left("NAME", 8), TableColumn::right("CPUS", 4)];
        let rendered =
            render_table_with_options(&columns, &[], TableRenderOptions { omit_header: true });

        assert!(rendered.is_empty());
    }

    #[test]
    fn byte_progress_wrap_read_preserves_contents() {
        let progress = ByteProgress::new("Downloading test artifact", Some(4))
            .expect("progress helper should construct");
        let mut reader = progress.wrap_read(Cursor::new(vec![1_u8, 2, 3, 4]));
        let mut bytes = Vec::new();
        reader
            .read_to_end(&mut bytes)
            .expect("reader should remain usable through the wrapper");

        assert_eq!(bytes, vec![1_u8, 2, 3, 4]);
    }

    #[tokio::test]
    async fn byte_progress_wrap_async_write_preserves_contents() {
        let progress = ByteProgress::new("Pulling test artifact", Some(4))
            .expect("progress helper should construct");
        let mut writer = progress.wrap_async_write(sink());
        writer
            .write_all(&[1_u8, 2, 3, 4])
            .await
            .expect("async writer should remain usable through the wrapper");
        writer
            .shutdown()
            .await
            .expect("wrapped async writer should shut down cleanly");
    }
}
