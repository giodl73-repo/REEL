import tempfile
import unittest
from pathlib import Path
from local_song_campaign import Campaign, qualifies, stanza_roles, reference_lyric_focus, take_dependency_contract
from local_song_backend import validate_request, descriptor


class AcceptanceTests(unittest.TestCase):
    def test_prior_compiled_analysis_remains_provenance(self):
        source=dict(logical_id='source/poem',version_id='v1',manifest_sha256='a'*64)
        compiled=dict(logical_id='take/previous',version_id='v1',manifest_sha256='b'*64)
        report=dict(sha256='c'*64,bytes=100)
        package=dict(assets={'source':dict(sha256='d'*64)},dependencies=[source])
        reference_lyric_focus(package,dict(analysis=report,version_lock=compiled))
        contract=take_dependency_contract(package,[])
        self.assertEqual(package['dependencies'],[source])
        self.assertEqual(package['lyric_focus_reference'],compiled)
        self.assertIn(report['sha256'],contract['references'])
        self.assertNotIn(report['sha256'],contract['primary_inputs'])

    def test_complete_coverage_still_required(self):
        self.assertFalse(qualifies(dict(passed=False,one_word_gap_lines=0),3))
        self.assertTrue(qualifies(dict(passed=True,one_word_gap_lines=3),3))
        self.assertFalse(qualifies(dict(passed=True,one_word_gap_lines=4),3))
        self.assertTrue(qualifies(dict(passed=True,one_word_gap_lines=4)))

    def test_invalid_caps_fail(self):
        for cap in [-1,True,3.5,'3']:
            with self.subTest(cap=cap),self.assertRaises(ValueError):
                qualifies(dict(passed=True,one_word_gap_lines=0),cap)

    def test_roles_leave_lyric_contract_valid(self):
        with tempfile.TemporaryDirectory() as t:
            root=Path(t);source=root/'source.txt'
            source.write_text('La luz vuelve.\n\nCanta el agua.\n\nVuelve la luz.\n',encoding='utf-8')
            asset=descriptor(source);lock=dict(logical_id='poem/example',version_id='v1',manifest_sha256='a'*64)
            slot=dict(id='example-a',title='Example',source=asset,source_lock=lock,caption='Cuban song',duration_seconds=120,max_duration_seconds=120,seed_base=20)
            c=Campaign(root,root/'unused.exe',root/'work');c.locate=lambda a:source
            c.briefs={'example-a':dict(source=asset,source_lock=lock,caption=slot['caption'],stanza_roles=['Verse','Verse','Chorus'])}
            request=c.request(slot,1)
            self.assertIn('[Chorus]\nVuelve la luz.',request['render_lyrics'])
            self.assertEqual(request['canonical_lyrics'],source.read_bytes().decode('utf-8'))
            validate_request(request,source)
            c.briefs['example-a']['stanza_roles']=['Verse']
            with self.assertRaises(ValueError):c.request(slot,1)

    def test_invalid_or_mismatched_roles_fail(self):
        for roles in [[],['Chorus','Verse'],['Instrumental'],['Verse]\nnew words']]:
            with self.subTest(roles=roles),self.assertRaises(ValueError):
                stanza_roles('[Verse]\nOriginal words\n[Outro - instrumental]',roles)


if __name__=='__main__':unittest.main()
