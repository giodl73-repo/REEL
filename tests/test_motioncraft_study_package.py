"""Transport failure checks: bad inputs must fail before hydration publication."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("study_package", Path(__file__).resolve().parents[1] / "tools/motioncraft_study_package.py")
package = importlib.util.module_from_spec(spec)
spec.loader.exec_module(package)


class HydrationFailures(unittest.TestCase):
    def fixture(self, root):
        source = root / "studies/landscape/scene.json"
        source.parent.mkdir(parents=True)
        source.write_text("{}", encoding="utf-8")
        package.write(root / "hydration.json", {
            "schema": "reel.motioncraft-study-hydration.v1", "synthetic_only": True,
            "profiles": {"landscape": {"assets": [], "authoring_files": ["studies/landscape/scene.json"]}}})
        components = [{"path": str(p.relative_to(root)).replace("\\", "/"), "sha256": package.sha(p)}
            for p in (source, root / "hydration.json")]
        package.write(root / "package.json", {"schema": "reel.production-package.v0.1", "components": components})
        return source

    def test_tampered_inventory_fails_before_creating_destination(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = self.fixture(root)
            source.write_text("tampered", encoding="utf-8")
            output = root / "clean"
            with self.assertRaisesRegex(ValueError, "hash mismatch"):
                package.hydrate(root, "landscape", output)
            self.assertFalse(output.exists())

    def test_parent_traversal_fails_before_creating_destination(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root)
            manifest = package.read(root / "package.json")
            manifest["components"][0]["path"] = "../scene.json"
            (root / "package.json").write_text(json.dumps(manifest), encoding="utf-8")
            output = root / "clean"
            with self.assertRaisesRegex(ValueError, "invalid portable path"):
                package.hydrate(root, "landscape", output)
            self.assertFalse(output.exists())

    def test_clean_hydration_preserves_inputs_and_refuses_overwrite(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = self.fixture(root)
            output = root / "clean"
            package.hydrate(root, "landscape", output)
            self.assertEqual(source.read_bytes(), (output / "scene.json").read_bytes())
            self.assertFalse((output / "compiled-revised").exists())
            with self.assertRaises(FileExistsError):
                package.hydrate(root, "landscape", output)


class PackageCadenceFailures(unittest.TestCase):
    def fixture(self, root):
        package.write(root / "study.json", {"synthetic_only": True})
        (root / "review-comparison-v2").mkdir()
        package.write(root / "review-comparison-v2/comparison.json", {"native_clock_verified": True})
        (root / "cadence-r1").mkdir()
        (root / "compiled-baseline").mkdir()
        (root / "render-baseline").mkdir()
        package.write(root / "compiled-baseline/job.json", {})
        (root / "render-baseline/picture.mkv").write_bytes(b"test-only bytes")
        report = {"schema": "reel.motioncraft-cadence.v1", "passed": True,
            "shots": [{"passed": True}],
            "job_sha256": package.sha(root / "compiled-baseline/job.json"),
            "picture_sha256": package.sha(root / "render-baseline/picture.mkv")}
        package.write(root / "cadence-r1/baseline.json", report)
        return report

    def test_failed_cadence_prevents_package_publication(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            report = self.fixture(root)
            report["passed"] = False
            (root / "cadence-r1/baseline.json").write_text(json.dumps(report))
            output = root / "package"
            with self.assertRaisesRegex(ValueError, "cadence report failed"):
                package.pack({"landscape": root, "portrait": root}, output)
            self.assertFalse(output.exists())

    def test_stale_cadence_job_binding_prevents_package_publication(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root)
            (root / "compiled-baseline/job.json").write_text('{"changed":true}')
            output = root / "package"
            with self.assertRaisesRegex(ValueError, "inventoried render/job"):
                package.pack({"landscape": root, "portrait": root}, output)
            self.assertFalse(output.exists())


if __name__ == "__main__":
    unittest.main()
