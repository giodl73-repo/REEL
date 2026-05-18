use super::{AdapterDescriptor, AdapterId, AdapterStatus, RenderOperationKind};

pub fn descriptor() -> AdapterDescriptor {
    AdapterDescriptor {
        id: AdapterId::Ffmpeg,
        status: AdapterStatus::ImplementedBaseline,
        boundary: "Rust-owned subprocess orchestration around external FFmpeg/ffprobe.",
        operations: vec![
            RenderOperationKind::Smoke,
            RenderOperationKind::ShotCards,
            RenderOperationKind::ContactSheet,
            RenderOperationKind::ReviewPack,
        ],
    }
}
