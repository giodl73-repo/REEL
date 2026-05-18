use std::fmt;

use serde::Serialize;

pub mod ai_video;
pub mod blender;
pub mod ffmpeg;
pub mod remotion;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum AdapterId {
    #[serde(rename = "ffmpeg")]
    Ffmpeg,
    #[serde(rename = "remotion")]
    Remotion,
    #[serde(rename = "blender")]
    Blender,
    #[serde(rename = "ai-video")]
    AiVideo,
}

impl AdapterId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ffmpeg => "ffmpeg",
            Self::Remotion => "remotion",
            Self::Blender => "blender",
            Self::AiVideo => "ai-video",
        }
    }
}

impl fmt::Display for AdapterId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum AdapterStatus {
    #[serde(rename = "implemented-baseline")]
    ImplementedBaseline,
    #[serde(rename = "planned")]
    Planned,
}

impl AdapterStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ImplementedBaseline => "implemented-baseline",
            Self::Planned => "planned",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum RenderOperationKind {
    #[serde(rename = "smoke")]
    Smoke,
    #[serde(rename = "shot-cards")]
    ShotCards,
    #[serde(rename = "contact-sheet")]
    ContactSheet,
    #[serde(rename = "scene-preview")]
    ScenePreview,
    #[serde(rename = "review-pack")]
    ReviewPack,
}

impl RenderOperationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Smoke => "smoke",
            Self::ShotCards => "shot-cards",
            Self::ContactSheet => "contact-sheet",
            Self::ScenePreview => "scene-preview",
            Self::ReviewPack => "review-pack",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AdapterDescriptor {
    pub id: AdapterId,
    pub status: AdapterStatus,
    pub boundary: &'static str,
    pub dependency_policy: &'static str,
    pub operations: Vec<RenderOperationKind>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RenderOperation {
    pub adapter: AdapterId,
    pub kind: RenderOperationKind,
    pub platform: Option<String>,
}

impl RenderOperation {
    pub fn smoke(adapter: AdapterId) -> Self {
        Self {
            adapter,
            kind: RenderOperationKind::Smoke,
            platform: None,
        }
    }

    pub fn shot_cards(adapter: AdapterId, platform: impl Into<String>) -> Self {
        Self {
            adapter,
            kind: RenderOperationKind::ShotCards,
            platform: Some(platform.into()),
        }
    }

    pub fn contact_sheet(adapter: AdapterId, platform: impl Into<String>) -> Self {
        Self {
            adapter,
            kind: RenderOperationKind::ContactSheet,
            platform: Some(platform.into()),
        }
    }

    pub fn review_pack(adapter: AdapterId) -> Self {
        Self {
            adapter,
            kind: RenderOperationKind::ReviewPack,
            platform: None,
        }
    }

    pub fn scene_preview(adapter: AdapterId, platform: impl Into<String>) -> Self {
        Self {
            adapter,
            kind: RenderOperationKind::ScenePreview,
            platform: Some(platform.into()),
        }
    }
}

pub fn adapter_catalog() -> Vec<AdapterDescriptor> {
    vec![
        ffmpeg::descriptor(),
        remotion::descriptor(),
        blender::descriptor(),
        ai_video::descriptor(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_keeps_ffmpeg_as_only_implemented_adapter() {
        let catalog = adapter_catalog();

        assert_eq!(catalog.len(), 4);
        assert_eq!(catalog[0].id, AdapterId::Ffmpeg);
        assert_eq!(catalog[0].status, AdapterStatus::ImplementedBaseline);
        assert!(
            catalog[0]
                .operations
                .contains(&RenderOperationKind::ShotCards)
        );
        assert!(
            catalog
                .iter()
                .find(|adapter| adapter.id == AdapterId::Remotion)
                .expect("remotion adapter exists")
                .operations
                .contains(&RenderOperationKind::ShotCards)
        );
        assert!(
            catalog
                .iter()
                .find(|adapter| adapter.id == AdapterId::Blender)
                .expect("blender adapter exists")
                .operations
                .contains(&RenderOperationKind::ReviewPack)
        );
        assert!(
            catalog
                .iter()
                .find(|adapter| adapter.id == AdapterId::AiVideo)
                .expect("ai-video adapter exists")
                .operations
                .contains(&RenderOperationKind::ReviewPack)
        );
        assert!(
            catalog
                .iter()
                .skip(1)
                .all(|adapter| adapter.status == AdapterStatus::Planned)
        );
    }

    #[test]
    fn render_operations_are_adapter_neutral() {
        let operation = RenderOperation::shot_cards(AdapterId::Ffmpeg, "youtube-demo");

        assert_eq!(operation.adapter, AdapterId::Ffmpeg);
        assert_eq!(operation.kind, RenderOperationKind::ShotCards);
        assert_eq!(operation.platform.as_deref(), Some("youtube-demo"));
    }
}
