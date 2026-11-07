//! Shared bounded request loading/preparation for single and multi-asset services.
use super::*;
use crate::source::native::{AssetSnapshot, updates};
use tig_assets::edits::Prepared;

pub(super) struct Loaded {
    sequence: bool,
    requests: Vec<Request>,
    raw: Vec<Vec<u8>>,
}
impl Loaded {
    pub(super) fn read(request: Request, request_path: &Path) -> Result<Self, String> {
        let (sequence, requests) = match request {
            Request::Sequence { changes } => (true, changes),
            single => (false, vec![single]),
        };
        if requests.is_empty()
            || requests.len() > tig_assets::edits::sequence::MAX_CHANGES
            || requests
                .iter()
                .any(|r| matches!(r, Request::Sequence { .. }))
        {
            return Err("invalid bounded history sequence".into());
        }
        let mut raw = Vec::new();
        for request in &requests {
            let document = match request {
                Request::ConfigureDelivery { document }
                | Request::RegisterRevision { document, .. }
                | Request::RecordDelivery { document, .. } => Some(document),
                _ => None,
            };
            raw.push(if let Some(document) = document {
                let path = if document.path.is_absolute() {
                    document.path.clone()
                } else {
                    request_path
                        .parent()
                        .unwrap_or(Path::new("."))
                        .join(&document.path)
                };
                let raw = bytes(&path)?;
                if Object::from_bytes(&raw) != document.expected {
                    return Err("change document pin differs".into());
                }
                raw
            } else {
                Vec::new()
            });
        }
        Ok(Self {
            sequence,
            requests,
            raw,
        })
    }
    pub(super) fn prepare(
        &self,
        snapshot: &AssetSnapshot,
        asset: &str,
        expected: &Object,
        dialect: Dialect,
        reader: &mut CacheMetadataReader,
    ) -> Result<Prepared, updates::Error> {
        let names = self
            .requests
            .iter()
            .map(|request| match request {
                Request::SelectRevision { streams, .. } => {
                    streams.iter().map(String::as_str).collect::<Vec<_>>()
                }
                _ => vec![],
            })
            .collect::<Vec<_>>();
        let mut changes = Vec::new();
        for ((request, raw), names) in self.requests.iter().zip(&self.raw).zip(&names) {
            changes.push(match request {
                Request::Sequence { .. } => {
                    return Err(updates::Error::Update(tig_assets::edits::Error::Budget));
                }
                Request::ConfigureDelivery { .. } => Change::ConfigureDelivery { policy: raw },
                Request::RegisterRevision { coverage, .. } => Change::RegisterRevision {
                    manifest: raw,
                    coverage: *coverage,
                },
                Request::RecordDelivery { coverage, .. } => Change::RecordDelivery {
                    event: raw,
                    coverage: *coverage,
                },
                Request::SelectRevision {
                    revision,
                    representation,
                    subject,
                    status,
                    ..
                } => Change::SelectRevision {
                    revision,
                    representation,
                    subject,
                    status,
                    streams: names,
                },
            });
        }
        if self.sequence {
            snapshot.prepare_history_sequence(
                asset,
                expected,
                dialect,
                Limits::default(),
                changes,
                reader,
            )
        } else {
            let change = changes
                .pop()
                .ok_or(updates::Error::Update(tig_assets::edits::Error::Budget))?;
            snapshot.prepare_history(asset, expected, dialect, Limits::default(), change, reader)
        }
    }
}
