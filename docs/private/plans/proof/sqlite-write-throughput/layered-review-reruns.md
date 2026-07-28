# Rejected Post-Review Layered Reruns

Date: 2026-07-27

Purpose: bind the layered measurement to the review-pass benchmark binary
after adding exact tabulated Student-t critical values, runtime identity, and
post-timer fixture assertions. A later review corrected the displayed
transaction/s estimator, so this binary is retained as superseded diagnostic
evidence rather than described as final.

Superseded review-pass binary SHA-256:
`2cdd3e5da0cbbd20be497dcc47ad78274182358ea4c0540a5585243abdd955c1`.

Verdict: **none of these reports is an accepted baseline**. Each whole run
was rejected because at least one lane exceeded the plan's 10% CV limit.
No lane or round was copied into the planning reference. A concurrent Rust
build in another checkout was observed during the first attempts, followed
by sustained unrelated host CPU and disk activity.

| Complete report | Report SHA-256 | Rounds × repetitions | Failing lane CVs |
| --- | --- | ---: | --- |
| `layered-review-attempt-1.md` | `e35b627b79b29866128aa3a81c27080114eedd78ce1d2dd8d6500195696b3779` | 12 × 60 | lower bound 16.3%; production storage 14.3% |
| `layered-review-attempt-2.md` | `00a729806f24587daf51a2c4482508575f1d4a967261374d0509ef9e6c69bc96` | 12 × 60 | resident current 10.1% |
| `layered-review-attempt-3.md` | `cc45655045646376b8c33b37c1bd6f3ac82e4149b727f6d3a50b67ce101b430e` | 12 × 60 | guarded 17.0%; lower bound 15.2% |
| `layered-review-attempt-4-120-repetitions.md` | `450735d736a2fe83db11d53d5d06a86210641cfed72261d4b3b1b27e14001715` | 12 × 120 diagnostic | raw 14.8%; lower bound 21.9% |
| `layered-review-attempt-5.md` | `a0953924d098066ad2410e04075d206c4047fa915fbd867d2f42155298625328` | 12 × 60 | guarded 29.5%; production storage 10.6% |

The 120-repetition experiment changed only the within-sample averaging and
was diagnostic; it does not replace the fixed 60-repetition protocol.

## Final hardened binary

After the review-pass attempts, the harness gained one rate estimator,
deterministic fixture identity, exact durable/live-state audits, fieldwise
maximum I/O retention, and exact catalog cardinality. Its binary SHA-256 is
`1ac46fb1dbf2d2d2b56eeedfe65770d5766be3dadc40f7bfd075efe406a2aa39`.
The complete `layered-final-binary-rejected.md` report has SHA-256
`77cb7fec6178c5d579462c3d9dcf4ee654188f2b5a62e5b7d89f199f866ea559`.
That whole 12 × 60 run is rejected because production storage measured 10.3%
CV; all other lanes were at or below 9.7%.

SWT0 owns the next quiet-host rerun and may supersede the planning reference
only with a complete report whose binary and report hashes are both recorded.
