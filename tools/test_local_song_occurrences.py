import unittest
from local_song_occurrences import occurrences


class OccurrenceTests(unittest.TestCase):
    def test_first_qualifying_and_first_exact_are_distinct(self):
        tokens=[dict(word=w,span=[i,i+.5]) for i,w in enumerate(['amber','lamp','noise','noise','noise','amber','lamp','glows'])]
        row=occurrences('Amber lamp glows',[dict(name='independent',tokens=tokens)])[0]
        self.assertEqual(row['first']['start'],0)
        self.assertEqual(row['first_exact']['start'],5)
        self.assertEqual(occurrences('Amber lamp glows',[dict(name='independent',tokens=tokens)],5)[0]['first']['start'],5)

    def test_near_identical_lines_need_their_distinguishing_word(self):
        tokens=[dict(word=w,span=[i,i+.5]) for i,w in enumerate(['amber','lamp','glows','noise','noise','noise','amber','bell','glows'])]
        rows=occurrences('Amber lamp glows\nAmber bell glows',[dict(name='independent',tokens=tokens)])
        self.assertEqual(rows[0]['first']['start'],0)
        self.assertEqual(rows[1]['first']['start'],6)

    def test_never_combines_words_from_independent_passes(self):
        passes=[dict(name=str(i),tokens=[dict(word=w,span=[0,1])]) for i,w in enumerate(['amber','lamp','glows'])]
        self.assertIsNone(occurrences('Amber lamp glows',passes)[0]['first'])


if __name__=='__main__':unittest.main()
