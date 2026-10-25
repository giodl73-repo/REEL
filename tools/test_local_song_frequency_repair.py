import unittest
import numpy as np
from scipy import signal
from local_song_frequency_repair import peaking_section,repair


class FrequencyRepairTests(unittest.TestCase):
    def test_requested_cut_accounts_for_two_filter_passes(self):
        section=peaking_section(48000,1800,10,-8)
        _,h=signal.sosfreqz(section.reshape(1,6),worN=[1800],fs=48000)
        self.assertAlmostEqual(float(40*np.log10(abs(h[0]))),-8,places=6)

    def test_protected_samples_and_other_tone_are_preserved(self):
        rate=48000;t=np.arange(rate*4)/rate
        source=np.stack([.2*np.sin(2*np.pi*1800*t)+.15*np.sin(2*np.pi*300*t)]*2,axis=1).astype(np.float32)
        result=repair(source,rate,1,3,[dict(hz=1800,q=10,gain_db=-8)])
        np.testing.assert_array_equal(source[:rate],result[:rate]);np.testing.assert_array_equal(source[3*rate:],result[3*rate:])
        span=slice(rate*3//2,rate*5//2)
        for hz,expected in [(1800,10**(-8/20)),(300,1)]:
            carrier=np.exp(-2j*np.pi*hz*t[span]);before=abs(np.dot(source[span,0],carrier));after=abs(np.dot(result[span,0],carrier))
            self.assertAlmostEqual(after/before,expected,delta=.002)

    def test_out_of_bounds_and_boost_rejected(self):
        with self.assertRaises(ValueError):peaking_section(48000,1800,10,8)
        with self.assertRaises(ValueError):repair(np.zeros((48000,2),dtype=np.float32),48000,.8,1.2,[dict(hz=1800,q=10,gain_db=-8)])


if __name__=='__main__':unittest.main()
