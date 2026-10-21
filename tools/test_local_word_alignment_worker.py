"""Synthetic custody and semantic-clock checks; no ML/runtime/network needed."""
import copy
import hashlib
import json
from pathlib import Path
import socket
import struct
import tempfile
import unittest
from unittest.mock import patch
import wave

import local_word_alignment_worker as worker


class LocalWords(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        model = self.root / "models" / worker.MODELS["es"][1]
        model.parent.mkdir(); model.write_bytes(b"synthetic model")
        audio = self.root / "input.wav"
        with wave.open(str(audio), "wb") as wav:
            wav.setparams((1, 2, 44100, 44100, "NONE", "not compressed"))
            wav.writeframes(b"\0\0" * 44100)
        def asset(p):return {"path":p.relative_to(self.root).as_posix(),"sha256":worker.digest(p),"bytes":p.stat().st_size}
        self.item = {"id":"one","cue_id":"cue-one","language":"es","text":"¡Está aquí!",
                     "text_sha256":hashlib.sha256("¡Está aquí!".encode()).hexdigest(),
                     "audio":asset(audio),"sample_rate":44100,"cue_end_sample":44100}
        self.request = {"schema":worker.SCHEMA,"device":"cpu","models":{"es":asset(model)},
                        "items":[self.item],"output":"result.json","runtime":{"python_sha256":"0"*64,
                        "module_versions":{},"alignment_module_sha256":"0"*64,"ffmpeg_sha256":"0"*64}}
        self.words = [{"word":"¡Está","start":0.1,"end":0.4},{"word":"aquí!","start":0.5,"end":0.9}]

    def test_exact_native_sample_clock_and_no_selection(self):
        with patch.object(worker,"offline"):
            result=worker.align(self.root,self.request,lambda *a:lambda *b:self.words)
        self.assertEqual(result["candidate_count"],1)
        evidence=result["results"][0]["evidence"]
        self.assertEqual(evidence["words"][0]["start_sample"],4410)
        self.assertEqual(evidence["selected_take_sha256"],self.item["audio"]["sha256"])
        self.assertFalse(result["human_listening_approval"])
        self.assertFalse(result["performance_selected"])

    def test_lexical_mismatch_and_missing_word_are_held(self):
        for words in [self.words[:1],[dict(self.words[0],word="Esta"),self.words[1]]]:
            with self.subTest(words=words):
                with self.assertRaisesRegex(ValueError,"Complete word"):
                    worker.evidence(self.item,words)

    def test_invalid_clock_is_never_published_as_evidence(self):
        for change in [{"end":2},{"start":float("nan")},{"start":.4,"end":.4},{"start":-.1}]:
            with self.subTest(change=change):
                with self.assertRaises(ValueError):worker.evidence(self.item,[dict(self.words[0],**change),self.words[1]])
        with self.assertRaises(ValueError):worker.evidence(self.item,[self.words[1],self.words[0]])

    def test_real_scalar_clocks_and_boolean_refusal(self):
        class Scalar(float):pass
        words=[dict(w,start=Scalar(w['start']),end=Scalar(w['end'])) for w in self.words]
        self.assertEqual(len(worker.evidence(self.item,words)['words']),2)
        with self.assertRaises(ValueError):worker.evidence(self.item,[dict(words[0],start=False),words[1]])

    def test_source_hash_and_wave_clock_fail_before_backend(self):
        for key,value in [("text_sha256","0"*64),("cue_end_sample",4),("sample_rate",16000)]:
            request=copy.deepcopy(self.request);request["items"][0][key]=value
            with self.subTest(key=key),self.assertRaises(ValueError):
                worker.align(self.root,request,lambda *a:self.fail("backend ran"))

    def test_extensible_pcm_clock_and_rejected_float_subtype(self):
        path=self.root/"input.wav"
        pcm=b"\0\0"*44100
        fmt=struct.pack("<HHIIHHHHI",65534,1,44100,88200,2,16,22,16,4)
        guid=bytes.fromhex("0100000000001000800000aa00389b71")
        body=b"WAVEfmt "+struct.pack("<I",40)+fmt+guid+b"data"+struct.pack("<I",len(pcm))+pcm
        path.write_bytes(b"RIFF"+struct.pack("<I",len(body))+body)
        self.assertEqual(worker.wave_clock(path),(44100,44100))
        self.item["audio"].update(sha256=worker.digest(path),bytes=path.stat().st_size)
        worker.validate(self.root,self.request)
        raw=path.read_bytes().replace(guid,b"\x03"+guid[1:],1);path.write_bytes(raw)
        with self.assertRaisesRegex(ValueError,"subtype"):worker.wave_clock(path)
        path.write_bytes(raw[:-1])
        with self.assertRaisesRegex(ValueError,"length"):worker.wave_clock(path)

    def test_corruption_duplicate_output_and_path_alias_are_refused(self):
        (self.root/"input.wav").write_bytes(b"corrupt")
        with self.assertRaisesRegex(ValueError,"mismatch"):worker.validate(self.root,self.request)
        request=copy.deepcopy(self.request);request["items"].append(request["items"][0])
        with self.assertRaises(ValueError):worker.validate(self.root,request)
        request=copy.deepcopy(self.request);request["output"]="../out.json"
        with self.assertRaises(ValueError):worker.validate(self.root,request)
        (self.root/"result.json").write_text("old")
        with self.assertRaisesRegex(ValueError,"already exists"):worker.validate(self.root,self.request)

    def test_existing_unpinned_model_cannot_trigger_backend_download(self):
        (self.root/"models"/"other.pt").write_bytes(b"unknown")
        with self.assertRaisesRegex(ValueError,"Unpinned"):worker.validate(self.root,self.request)

    def test_offline_socket_refusal(self):
        with patch.object(socket.socket,"connect"),patch.object(socket.socket,"connect_ex"),patch.object(socket,"create_connection"):
            worker.offline()
            with self.assertRaises(RuntimeError):socket.create_connection(("example.invalid",443))

    def test_partial_measurement_retains_diagnostic_without_claiming_pass(self):
        with patch.object(worker,"offline"):
            result=worker.align(self.root,self.request,lambda *a:lambda *b:self.words[:1])
        self.assertEqual(result["held_count"],1)
        self.assertNotIn("evidence",result["results"][0])
        self.assertEqual(json.loads((self.root/"result.json").read_bytes())["candidate_count"],0)


if __name__=="__main__":unittest.main()
