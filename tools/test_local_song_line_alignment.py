import unittest
from local_song_line_alignment import align


class AlignmentTests(unittest.TestCase):
    def fixture(self):
        analysis={'passes':[{'name':'test','tokens':[{'word':w,'span':[i,i+.4]} for i,w in enumerate(['bright','river','flows'])]}]}
        model={'tempo_map':[{'tick':0,'microseconds_per_quarter':500000}],
               'musical_timebase':{'pulses_per_quarter':100},'duration_ticks':1000,
               'parts':[{'notes':[{'id':'n1','start_tick':0,'duration_ticks':600}]}]}
        return analysis,model

    def test_owner_omission_overrides_machine_match(self):
        a,m=self.fixture();r=align('bright river flows','bright river flows',a,m,[1])
        self.assertEqual(r['lines'][0]['candidates'],[])
        self.assertEqual(r['owner_accepted_omissions'],[1])
        self.assertFalse(r['underlay_assigned'])

    def test_source_clock_maps_existing_notes_without_mutation(self):
        a,m=self.fixture();r=align('bright river flows','bright river flows',a,m)
        self.assertEqual(r['lines'][0]['candidates'][0]['note_run_ids'],['n1'])
        self.assertFalse(r['pitches_modified'])

    def test_different_line_counts_require_explicit_map(self):
        a,m=self.fixture()
        with self.assertRaises(ValueError):align('one line','two\nlines',a,m)


if __name__=='__main__':unittest.main()
