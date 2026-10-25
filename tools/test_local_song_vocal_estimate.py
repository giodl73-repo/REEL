import unittest

from local_song_vocal_estimate import normalize, restore, validate


class VocalEstimateTests(unittest.TestCase):
    def test_normalization_and_dc_restoration(self):
        import torch
        mix = torch.tensor([[.1, .3, -.2, .5], [.2, .4, -.1, .6]])
        normalized, mean, std = normalize(mix)
        estimates = torch.stack([normalized*.2, normalized*.8])
        restored = restore(estimates, mean, std, 2)
        self.assertTrue(torch.allclose(restored.sum(0), mix, atol=1e-7))
        with self.assertRaises(ValueError):
            restore(estimates, mean, std, 3)

    def test_silent_and_nonfinite_sources_fail(self):
        import torch
        for samples in [torch.zeros((2, 12)), torch.full((2, 12), float('nan'))]:
            with self.assertRaises(ValueError):
                normalize(samples)

    def test_private_scope_and_explicit_source(self):
        q = dict(schema='reel.local-song-vocal-estimate.v1', duration_seconds=120,
            device='cpu', golden_lock=dict(logical_id='song/fixture', manifest_sha256='a'*64, version_id='full-v1'),
            permissions=dict(private_only=True, external_upload=False, public_release=False))
        validate(q)
        q['permissions']['external_upload'] = True
        with self.assertRaises(ValueError):
            validate(q)
        q['permissions']['external_upload'] = False
        q['golden_lock']['manifest_sha256'] = 'unknown'
        with self.assertRaises(ValueError):
            validate(q)


if __name__ == '__main__':
    unittest.main()
