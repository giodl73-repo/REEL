# Explicit editor host review

Producer-applied Editor and Rights/Provenance checklists; human visual/media
approval and independent review remain separate.

| Finding | Implementation and evidence |
| --- | --- |
| Static exports need a real browser origin without reviving a Python server or daemon | One explicit foreground Rust command serves cel/note/underlay exports, optionally opens the default browser, and expires on idle/lifetime limits. Export and resume paths never start it. |
| A private-folder server must not become a general filesystem or job API | Only known bundled frontends and their declared resources are allowed. Loopback binding, OS-random capability paths, strict Host/Origin checks, GET/HEAD-only behavior, bounded parsing, CSP and source fingerprints constrain access. There are no mutating network operations. |
| Media review must avoid another copy and preserve source evidence | Controls freeze in memory; declared media use bounded read-only handles, initial fingerprints, metadata-change checks and byte-range streaming. Reports and URLs stay private. Media quality and original delivery authority are not inferred. |
| Expiry must work in the real executable | Synthetic process tests check read-only HTTP behavior, source preservation, report refusal, idle/lifetime expiry and listener closure. Socket tests cover HEAD/ranges and frozen controls; parser tests include hostile paths, duplicate headers, foreign origins and unsupported methods. |
| Browser connections can arrive idle or split across packets | Accepted sockets explicitly use blocking reads within bounded concurrent readers. Split-header and idle-connection regressions exercise the actual failure paths. Frozen controls stay shared/read-only; only an individual media handle's seek position serializes. |

Four retained private editor sessions were opened in isolated Edge through the
actual Rust listener: both cel languages, notes and underlay. Native drafts
download with original source locks, HEAD/ranges work, foreign-origin and POST
requests refuse, desktop/mobile document bounds pass and source files stay
unchanged. All four processes expire normally. No page errors or unexpected
page requests were observed. Detailed reports remain private project assets.

The optional OS helper is tested as a command/argument construction without a
GUI side effect; the browser fixture navigates the real listener URL directly.
Human default-browser interaction, media playback and rendered review remain
explicit acceptance work rather than being claimed by these transport tests.
