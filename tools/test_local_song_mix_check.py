import unittest
import numpy as np
from local_song_mix_check import compare_samples


class MixCheckTests(unittest.TestCase):
    def test_quantized_delivery_passes_but_wrong_gain_or_shift_fails(self):
        expected=np.array([[0,0],[.25,-.5],[.1,.2]],dtype=np.float32)
        self.assertTrue(compare_samples(expected,expected+2**-24)['stem_sum_matches'])
        for wrong in [expected*.9,np.roll(expected,1,axis=0),expected[:-1]]:
            with self.assertRaises(ValueError):compare_samples(expected,wrong)


if __name__=='__main__':unittest.main()
