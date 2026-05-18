use std::path::{Path, PathBuf};

use super::{AdapterDescriptor, AdapterId, AdapterStatus, RenderOperationKind};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlenderPlan {
    pub manifest: PathBuf,
    pub output_dir: PathBuf,
    pub platform: String,
    pub command_shape: Vec<String>,
}

pub fn descriptor() -> AdapterDescriptor {
    AdapterDescriptor {
        id: AdapterId::Blender,
        status: AdapterStatus::Planned,
        boundary: "Blender CLI or Python file boundary; planned only, no binary is required yet.",
        dependency_policy: "No Blender binary, Python script, or add-on dependency is required until implementation is explicitly selected.",
        operations: vec![
            RenderOperationKind::ShotCards,
            RenderOperationKind::ReviewPack,
        ],
    }
}

pub fn plan(
    manifest: impl AsRef<Path>,
    output_dir: impl AsRef<Path>,
    platform: &str,
) -> BlenderPlan {
    let manifest = manifest.as_ref().to_path_buf();
    let output_dir = output_dir.as_ref().to_path_buf();
    BlenderPlan {
        command_shape: vec![
            "blender".to_string(),
            "--background".to_string(),
            "--python".to_string(),
            "reel_blender_adapter.py".to_string(),
            "--".to_string(),
            "--manifest".to_string(),
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
    fn blender_descriptor_is_planned_file_boundary() {
        let descriptor = descriptor();

        assert_eq!(descriptor.id, AdapterId::Blender);
        assert_eq!(descriptor.status, AdapterStatus::Planned);
        assert!(descriptor.boundary.contains("no binary is required yet"));
    }

    #[test]
    fn blender_plan_records_cli_shape_without_executing() {
        let plan = plan("work/manifest.yaml", "renders/blender", "youtube-demo");

        assert_eq!(plan.platform, "youtube-demo");
        assert_eq!(plan.command_shape[0], "blender");
        assert!(plan.command_shape.contains(&"--manifest".to_string()));
    }
}
