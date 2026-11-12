"""Synthetic fixtures; no model, voice material, network, or ML install needed."""
import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import socket
import tempfile
import unittest
from unittest.mock import patch
import wave

spec = importlib.util.spec_from_file_location("worker", Path(__file__).with_name("local_tts_worker.py"))
worker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(worker)


class LocalTTS(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        def asset(name):
            p = self.root / name
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_bytes(b"synthetic " + name.encode())
            return {"path": name, "sha256": worker.digest(p), "bytes": p.stat().st_size}
        self.request = {"schema": worker.SCHEMA, "text": "A synthetic test.",
                        "text_sha256": hashlib.sha256(b"A synthetic test.").hexdigest(),
                        "language": "en", "seed": 3, "device": "cpu", "sample_rate": 24000,
                        "max_seconds": 1,
                        "settings": {"exaggeration": .4, "cfg_weight": .4, "temperature": .6,
                                     "repetition_penalty": 2, "min_p": .05, "top_p": .96},
                        "authorization": {"local_synthesis": True, "remote_transfer": False,
                                          "public_release": False, "decision": asset("decision.txt")},
                        "reference": asset("reference.wav"),
                        "model_files": {n: asset("model/" + n) for n in worker.REQUIRED_MODEL},
                        "output": "out/audio.wav", "receipt": "out/receipt.json"}

    def test_synthetic_waveform_receipt_and_existing_output_refusal(self):
        # Keep network monkeypatches scoped to the synthetic worker invocation.
        with patch.object(worker, "offline"):
            result = worker.render(self.root, self.request, lambda *args: [.2, -.1] * 240)
        output = self.root / self.request["output"]
        with wave.open(str(output), "rb") as wav:
            self.assertEqual((wav.getnchannels(), wav.getsampwidth(), wav.getnframes()), (1, 2, 480))
        self.assertEqual(result["output_sha256"], worker.digest(output))
        self.assertFalse(result["human_listening_approval"])
        self.assertEqual(json.loads((self.root / self.request["receipt"]).read_text())["frames"], 480)
        with self.assertRaisesRegex(ValueError, "already exists"):
            worker.validate(self.root, self.request)

    def test_rejects_input_corruption_before_backend(self):
        (self.root / "reference.wav").write_bytes(b"tampered")
        with self.assertRaisesRegex(ValueError, "mismatch"):
            worker.render(self.root, self.request, lambda *args: self.fail("backend ran"))

    def test_rejects_missing_or_expanded_authorization(self):
        for key, value in [("local_synthesis", False), ("remote_transfer", True), ("public_release", True)]:
            with self.subTest(key=key):
                r = copy.deepcopy(self.request)
                r["authorization"][key] = value
                with self.assertRaises(ValueError):
                    worker.validate(self.root, r)

    def test_rejects_escape_alias_extra_model_and_nonfinite_settings(self):
        bad = [("output", "../escape.wav"), ("output", "reference.wav"),
               ("text_sha256", "0" * 64), ("language", "unsupported")]
        for key, value in bad:
            with self.subTest(key=key, value=value):
                r = copy.deepcopy(self.request)
                r[key] = value
                with self.assertRaises(ValueError):
                    worker.validate(self.root, r)
        (self.root / "model/extra.bin").write_bytes(b"not pinned")
        with self.assertRaisesRegex(ValueError, "Unpinned"):
            worker.validate(self.root, self.request)

    def test_invalid_and_oversized_audio_is_not_published(self):
        for samples in [[], [0.0], [float("nan")], [.1] * 24001]:
            with self.subTest(samples=len(samples)), patch.object(worker, "offline"):
                with self.assertRaises(ValueError):
                    worker.render(self.root, self.request, lambda *args: samples)
                self.assertFalse((self.root / self.request["output"]).exists())

    def test_nonfinite_settings_and_clipping_attenuation(self):
        invalid = copy.deepcopy(self.request)
        invalid["settings"]["temperature"] = float("nan")
        with self.assertRaisesRegex(ValueError, "finite"):
            worker.validate(self.root, invalid)
        with patch.object(worker, "offline"):
            result = worker.render(self.root, self.request, lambda *args: [2.0, -2.0])
        self.assertAlmostEqual(result["gain"], .49)
        self.assertEqual(result["source_peak"], 2)

    def test_network_attempts_are_blocked(self):
        with patch.object(socket.socket, "connect"), patch.object(socket.socket, "connect_ex"), patch.object(socket, "create_connection"):
            worker.offline()
            with self.assertRaisesRegex(RuntimeError, "disabled"):
                socket.create_connection(("127.0.0.1", 1))


if __name__ == "__main__":
    unittest.main()
