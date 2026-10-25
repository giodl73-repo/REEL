import unittest
from local_song_asr_windows import validate_windows


class WindowBoundsTests(unittest.TestCase):
    def test_valid_independent_conditions(self):
        validate_windows([[20,50],[70,100]],['mix','vocals'],195)

    def test_rejects_unbounded_overlapping_and_nonfinite_windows(self):
        for ranges in [[],[[0,61]],[[20,50],[49,60]],[[-1,10]],[[190,200]],[[0,float('nan')]],[[True,2]],[[0,60],[60,120]]]:
            with self.assertRaises(ValueError):validate_windows(ranges,['mix','vocals'],195)

    def test_rejects_implicit_or_unknown_conditions(self):
        for conditions in [[],['expected-lyrics'],['mix','mix']]:
            with self.assertRaises(ValueError):validate_windows([[0,20]],conditions,195)


if __name__=='__main__':unittest.main()
