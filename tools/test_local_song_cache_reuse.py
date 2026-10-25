import tempfile
import unittest
from pathlib import Path
from unittest.mock import Mock
from local_song_campaign import Campaign


class CacheReuseTests(unittest.TestCase):
    def fixture(self, folder):
        path=Path(folder)/'source.txt';path.write_bytes(b'immutable')
        campaign=Campaign(folder,Path(folder)/'unused.exe',Path(folder)/'work')
        campaign.ingest=Mock(return_value={'admitted':'new data'})
        return campaign,path

    def test_reuse_requires_fresh_active_verification_and_preserves_registration(self):
        with tempfile.TemporaryDirectory() as folder:
            campaign,path=self.fixture(folder);campaign.locate=Mock(return_value=path)
            asset=campaign.reuse_or_ingest(path,'new context')
            campaign.locate.assert_called_once_with(asset)
            campaign.ingest.assert_not_called()
            self.assertEqual(asset['bytes'],9)
            self.assertEqual(asset['cache_uri'],'cache://sha256/'+asset['sha256'])

    def test_unknown_bytes_still_require_native_admission(self):
        with tempfile.TemporaryDirectory() as folder:
            campaign,path=self.fixture(folder);campaign.locate=Mock(side_effect=RuntimeError('Unregistered hash'))
            self.assertEqual(campaign.reuse_or_ingest(path,'new context'),{'admitted':'new data'})
            campaign.ingest.assert_called_once_with(path,'new context')

    def test_corruption_never_falls_back_to_ingest(self):
        with tempfile.TemporaryDirectory() as folder:
            campaign,path=self.fixture(folder);campaign.locate=Mock(side_effect=RuntimeError('Active object hash mismatch'))
            with self.assertRaises(RuntimeError):campaign.reuse_or_ingest(path,'new context')
            campaign.ingest.assert_not_called()


if __name__=='__main__':unittest.main()
