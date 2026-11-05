"""Synthetic control checks; no model import, generation, setup or private data."""
import copy
import unittest
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
        for field,value in [('attempts_per_patch',2),('attempts_per_patch',True),('semantic_planner',True),('seed',True),('repaint_strength',float('nan')),('repaint_mode','unknown')]:
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
