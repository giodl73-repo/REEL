use std::path::{Path, PathBuf};

use super::{AdapterDescriptor, AdapterId, AdapterStatus, RenderOperationKind};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AiVideoPackagePlan {
    pub manifest: PathBuf,
    pub package_dir: PathBuf,
    pub platform: String,
    pub package_files: Vec<String>,
}

pub fn descriptor() -> AdapterDescriptor {
    AdapterDescriptor {
        id: AdapterId::AiVideo,
        status: AdapterStatus::Planned,
        boundary: "Provider-neutral package boundary; planned only, no SDK or credentials are required yet.",
        operations: vec![
            RenderOperationKind::ShotCards,
            RenderOperationKind::ReviewPack,
        ],
    }
}

pub fn plan_package(
    manifest: impl AsRef<Path>,
    package_dir: impl AsRef<Path>,
    platform: &str,
) -> AiVideoPackagePlan {
    AiVideoPackagePlan {
        manifest: manifest.as_ref().to_path_buf(),
        package_dir: package_dir.as_ref().to_path_buf(),
        platform: platform.to_string(),
        package_files: vec![
            "manifest.yaml".to_string(),
            "shots.json".to_string(),
            "continuity.md".to_string(),
            "review-notes.md".to_string(),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ai_video_descriptor_is_provider_neutral() {
        let descriptor = descriptor();

        assert_eq!(descriptor.id, AdapterId::AiVideo);
        assert_eq!(descriptor.status, AdapterStatus::Planned);
        assert!(descriptor.boundary.contains("no SDK or credentials"));
    }

    #[test]
    fn ai_video_plan_records_package_shape_without_provider_lock_in() {
        let plan = plan_package("work/manifest.yaml", "renders/ai-video", "iphone-social");

        assert_eq!(plan.platform, "iphone-social");
        assert!(plan.package_files.contains(&"manifest.yaml".to_string()));
        assert!(
            !plan
                .package_files
                .iter()
                .any(|file| file.contains("provider"))
        );
    }
}
