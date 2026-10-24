import unittest
import numpy as np
from local_instrumental_form import tonal_candidates,section_ranges


class FormTests(unittest.TestCase):
    def test_tonal_ranking_is_transposition_equivariant(self):
        vector=np.asarray([3,0,1,0,2,1,0,2.5,0,1,0,1.]);a=tonal_candidates(vector)[0];b=tonal_candidates(np.roll(vector,5))[0];self.assertEqual(a['quality'],b['quality']);self.assertEqual((a['root']+5)%12,b['root'])
    def test_empty_tonal_evidence_is_rejected(self):
        with self.assertRaises(ValueError):tonal_candidates(np.zeros(12))
    def test_quiet_source_keeps_one_full_section(self):
        result=section_ranges(np.zeros(100),np.linspace(0,30,100),30);self.assertEqual([(x['start'],x['end']) for x in result],[(0.,30.)])
    def test_section_proposals_preserve_extent_and_spacing(self):
        times=np.arange(0,60,.1);values=np.zeros(len(times));values[100]=1;values[120]=.8;values[300]=1;values[500]=1;r=section_ranges(values,times,60);bounds=[r[0]['start']]+[x['end'] for x in r];self.assertEqual(bounds[0],0);self.assertEqual(bounds[-1],60);self.assertTrue(all(b>a for a,b in zip(bounds[:-1],bounds[1:])));self.assertTrue(all(b-a>=8 for a,b in zip(bounds[1:-2],bounds[2:-1])))


if __name__=='__main__':unittest.main()
