use std::path::{Path, PathBuf};

use super::{AdapterDescriptor, AdapterId, AdapterStatus, RenderOperationKind};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemotionPlan {
    pub manifest: PathBuf,
    pub output_dir: PathBuf,
    pub platform: String,
    pub command_shape: Vec<String>,
}

pub fn descriptor() -> AdapterDescriptor {
    AdapterDescriptor {
        id: AdapterId::Remotion,
        status: AdapterStatus::Planned,
        boundary: "Node/Remotion project boundary; planned only, no dependency is required yet.",
        dependency_policy: "No Node, Remotion package, browser runtime, or npm install is required until implementation is explicitly selected.",
        operations: vec![
            RenderOperationKind::ShotCards,
            RenderOperationKind::ContactSheet,
            RenderOperationKind::ReviewPack,
        ],
    }
}

pub fn plan(
    manifest: impl AsRef<Path>,
    output_dir: impl AsRef<Path>,
    platform: &str,
) -> RemotionPlan {
    let manifest = manifest.as_ref().to_path_buf();
    let output_dir = output_dir.as_ref().to_path_buf();
    RemotionPlan {
        command_shape: vec![
            "npx".to_string(),
            "remotion".to_string(),
            "render".to_string(),
            "--props".to_string(),
            manifest.display().to_string(),
            "--out-dir".to_string(),
            output_dir.display().to_string(),
            "--platform".to_string(),
            platform.to_string(),
        ],
        manifest,
        output_dir,
        platform: platform.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remotion_descriptor_is_planned_without_dependency_lock_in() {
        let descriptor = descriptor();

        assert_eq!(descriptor.id, AdapterId::Remotion);
        assert_eq!(descriptor.status, AdapterStatus::Planned);
        assert!(
            descriptor
                .boundary
                .contains("no dependency is required yet")
        );
        assert!(
            descriptor
                .operations
                .contains(&RenderOperationKind::ShotCards)
        );
    }

    #[test]
    fn remotion_plan_records_file_boundary_without_executing() {
        let plan = plan("work/manifest.yaml", "renders/remotion", "youtube-demo");

        assert_eq!(plan.platform, "youtube-demo");
        assert_eq!(plan.manifest, PathBuf::from("work/manifest.yaml"));
        assert_eq!(plan.output_dir, PathBuf::from("renders/remotion"));
        assert_eq!(plan.command_shape[0], "npx");
    }
}
