"""Synthetic control checks; no model import, generation, setup or private data."""
import copy
import unittest
import json
import os
import sys
import tempfile
import types
from pathlib import Path
from unittest import mock
import local_song_repaint as provider
from local_song_repaint import validate_original_repair
from local_song_backend import validate_request


def fixture():
    scope = dict(private_only=True, external_upload=False, identifiable_imitation=False, public_release=False)
    context = dict(schema='reel.song-repair-source-context.v1',private_scope=scope,
        canonical_lyrics='Álpha beta\nDelta echo',recording=dict(sha256='a'*64,bytes=100),
        sample_rate=8000,source_frames=8000,channels=2,original_request_rewritten=False,
        provider_executed=False,human_approval=False,shareable=False,mapping_authority_verified=False)
    request = dict(schema='reel.original-song-repair-provider-request.v1',permissions=copy.deepcopy(scope),
        source=context['recording'],sample_rate=8000,source_frames=8000,channels=2,
        attempts_per_patch=1,semantic_planner=False,caption='Synthetic private fixture',seed=7,
        repaint_mode='conservative',repaint_strength=.5,patches=[dict(start_frame=2000,end_frame=5000,
        fade_frames=2,context_start_frame=0,context_end_frame=8000,source_lines=[1,2],supplied_lyrics='Álpha beta\nDelta echo')])
    return request, context


class OriginalProviderControls(unittest.TestCase):
    def test_explicit_float16_uses_native_cuda_selection_and_asserts_actual_dtype(self):
        fp32, fp16 = object(), object()
        torch = types.ModuleType('torch')
        torch.float32, torch.float16 = fp32, fp16
        torch.set_num_threads = mock.Mock()
        original_probe = mock.Mock(return_value=True)
        torch.cuda = types.SimpleNamespace(is_bf16_supported=original_probe)
        gpu = types.ModuleType('acestep.gpu_config')
        original_platform = lambda: False
        gpu.is_rocm_available = original_platform
        package = types.ModuleType('acestep')
        package.__path__ = []
        package.gpu_config = gpu
        handler = types.ModuleType('acestep.handler')
        def initialize(**kwargs):
            self.assertFalse(torch.cuda.is_bf16_supported())
            return 'synthetic initialized', True
        initialized = mock.Mock(side_effect=initialize)
        handler.AceStepHandler = lambda: types.SimpleNamespace(dtype=fp16,initialize_service=initialized)
        inference = types.ModuleType('acestep.inference')
        inference.GenerationConfig, inference.GenerationParams = object(), object()
        inference.generate_music = mock.Mock(side_effect=AssertionError('initialization must not generate'))
        old_path = list(sys.path)
        try:
            with mock.patch.dict(sys.modules,{'torch':torch,'acestep':package,'acestep.gpu_config':gpu,'acestep.handler':handler,'acestep.inference':inference}), mock.patch.dict(os.environ,{}):
                provider.initialize_ace({'ace_root':'synthetic'},model_precision='float16')
                self.assertIs(gpu.is_rocm_available,original_platform)
                self.assertEqual(os.environ['ACESTEP_ROCM_DTYPE'],'float16')
                self.assertIs(torch.cuda.is_bf16_supported,original_probe)
                inference.generate_music.assert_not_called()
                self.assertEqual(initialized.call_args.kwargs['quantization'],'w8a8_dynamic')
                handler.AceStepHandler = lambda: types.SimpleNamespace(dtype=fp32,initialize_service=initialized)
                with self.assertRaisesRegex(RuntimeError,'dtype differs'):
                    provider.initialize_ace({'ace_root':'synthetic'},model_precision='float16')
                self.assertIs(torch.cuda.is_bf16_supported,original_probe)
        finally:
            sys.path[:] = old_path

    def test_progress_is_bounded_synced_and_never_overwrites_or_logs_source(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder)/'phases.jsonl'
            with provider.OriginalRepairProgress(path) as progress:
                with self.assertRaises(ValueError):
                    progress.emit('private source words')
                for _ in range(32):
                    progress.emit('entered')
                with self.assertRaises(ValueError):
                    progress.emit('entered')
                self.assertEqual(len(path.read_text().splitlines()),32)
            self.assertLess(path.stat().st_size,16384)
            with self.assertRaises(FileExistsError):
                provider.OriginalRepairProgress(path)
            self.assertNotIn('private source words',path.read_text())

    def test_progress_keeps_last_phase_and_propagates_python_failure(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder)/'phases.jsonl'
            with self.assertRaisesRegex(RuntimeError,'private error'):
                with provider.OriginalRepairProgress(path) as progress:
                    progress.emit('model-init-start')
                    raise RuntimeError('private error')
            rows = [json.loads(line) for line in path.read_text().splitlines()]
            self.assertEqual([r['stage'] for r in rows],['model-init-start','python-exception'])
            self.assertNotIn('private error',path.read_text())
            failure = json.loads((Path(folder)/'provider-outcome.json').read_text())
            self.assertTrue(failure['failed'])
            self.assertEqual(failure['exception_type'],'RuntimeError')
            self.assertEqual(failure['message'],'private error')
            self.assertFalse(failure['shareable'])

    def test_failure_evidence_is_bounded_and_never_overwrites_or_masks_exception(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            message = '\u00e9'*10000
            with self.assertRaisesRegex(RuntimeError,'\u00e9'):
                with provider.OriginalRepairProgress(root/'first.jsonl'):
                    raise RuntimeError(message)
            failure_path = root/'provider-outcome.json'
            raw = failure_path.read_bytes()
            failure = json.loads(raw)
            self.assertLessEqual(len(failure['message'].encode('utf-8')),4096)
            self.assertTrue(failure['message_truncated'])
            self.assertLessEqual(len(raw),32768)
            with self.assertRaisesRegex(ValueError,'second failure'):
                with provider.OriginalRepairProgress(root/'second.jsonl'):
                    raise ValueError('second failure')
            self.assertEqual(failure_path.read_bytes(),raw)

    def test_success_emits_declared_private_outcome_without_failure_claim(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            with provider.OriginalRepairProgress(root/'phases.jsonl'):
                pass
            outcome = json.loads((root/'provider-outcome.json').read_text())
            self.assertFalse(outcome['failed'])
            self.assertIsNone(outcome['exception_type'])
            self.assertIsNone(outcome['message'])
            self.assertFalse(outcome['human_approval'])

    def test_runtime_only_mode_returns_before_any_generation_constructor_or_call(self):
        request, context = fixture()
        for field in ['runtime','source_context','backend','backend_helper']:
            request[field] = dict(sha256='b'*64,bytes=10)
        request['patches'][0]['context_audio'] = dict(sha256='c'*64,bytes=100)
        runtime = dict(python=dict(path='synthetic'),ace_root='synthetic',ace_commit='a'*40)
        inputs = {'provider-request.json':request,'source-context.json':context,'runtime.json':runtime}
        info = types.SimpleNamespace(subtype='FLOAT',samplerate=8000,frames=8000,channels=2)
        forbidden = mock.Mock(side_effect=AssertionError('runtime-only must not generate'))
        def initialized(_, progress, model_precision):
            self.assertEqual(model_precision,'float32')
            for stage in ['torch-import-start','torch-import-complete','ace-import-start','ace-import-complete','model-init-start','model-init-complete']:
                progress.emit(stage)
            return object(), forbidden, forbidden, forbidden
        original = Path.cwd()
        with tempfile.TemporaryDirectory() as folder:
            try:
                os.chdir(folder)
                with mock.patch.object(provider,'read',side_effect=lambda name:inputs[name]), \
                     mock.patch.object(provider,'checked'), mock.patch.object(provider,'runtime_files'), \
                     mock.patch.object(provider,'descriptor',return_value=dict(sha256='b'*64,bytes=10)), \
                     mock.patch.object(provider.subprocess,'check_output',return_value='a'*40), \
                     mock.patch.object(provider,'initialize_ace',side_effect=initialized), \
                     mock.patch.dict(sys.modules,{'soundfile':types.SimpleNamespace(info=lambda _:info)}):
                    provider.original_repair_main(runtime_check_only=True)
                forbidden.assert_not_called()
                receipt = json.loads(Path('provider-runtime-check.json').read_text())
                self.assertFalse(receipt['provider_generation_called'])
                self.assertFalse(receipt['human_approval'])
                rows = [json.loads(line) for line in Path('provider-progress.jsonl').read_text().splitlines()]
                self.assertEqual(rows[-1]['stage'],'runtime-check-complete')
                self.assertFalse(any(Path('.').glob('raw-patch-*')))
                self.assertFalse(Path('provider-receipt.json').exists())
            finally:
                os.chdir(original)

    def test_exact_original_context_and_canonical_words(self):
        request, context = fixture()
        validate_original_repair(request,context,8000,8000,2)
        self.assertEqual(context['canonical_lyrics'],'Álpha beta\nDelta echo')

    def test_private_flags_remain_boolean_and_restrictive(self):
        for field,value in [('private_only',False),('external_upload',True),('identifiable_imitation',True),('public_release',True),('private_only',1)]:
            request, context = fixture()
            request['permissions'][field] = value
            with self.assertRaises(ValueError):
                validate_original_repair(request,context,8000,8000,2)
        for field in ['original_request_rewritten','provider_executed','human_approval','shareable','mapping_authority_verified']:
            request, context = fixture()
            context[field] = True
            with self.assertRaises(ValueError):
                validate_original_repair(request,context,8000,8000,2)

    def test_clock_masks_context_and_literal_word_changes_refuse(self):
        for field,value in [('start_frame',True),('end_frame',8001),('fade_frames',1500),('context_start_frame',1),('source_lines',[0,1]),('supplied_lyrics','Alpha beta\nDelta echo')]:
            request, context = fixture()
            request['patches'][0][field] = value
            with self.assertRaises(ValueError):
                validate_original_repair(request,context,8000,8000,2)
        request, context = fixture()
        with self.assertRaises(ValueError):
            validate_original_repair(request,context,16000,8000,2)

    def test_one_explicit_attempt_and_finite_conditioning_controls(self):
        for field,value in [('attempts_per_patch',2),('attempts_per_patch',True),('semantic_planner',True),('seed',True),('repaint_strength',float('nan')),('repaint_mode','unknown'),('model_precision','bfloat16'),('model_precision',True)]:
            request, context = fixture()
            request[field] = value
            with self.assertRaises(ValueError):
                validate_original_repair(request,context,8000,8000,2)

    def test_separate_context_route_does_not_loosen_modern_guide_validation(self):
        with self.assertRaises(ValueError):
            validate_request(dict(schema='example.older-request.v1'),'Álpha beta')
        request, context = fixture()
        context['schema'] = 'reel.local-song-request.v1'
        with self.assertRaises(ValueError):
            validate_original_repair(request,context,8000,8000,2)


if __name__ == '__main__':
    unittest.main()
