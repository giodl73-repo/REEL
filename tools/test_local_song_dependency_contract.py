import unittest
from local_song_campaign import take_dependency_contract

class ContractTests(unittest.TestCase):
    def test_direct_take_keeps_sources_active_and_history_separate(self):
        assets={role:dict(sha256=str(i)*64) for i,role in enumerate(['source','request','runtime','backend','recording','listening','job','job_result','timing-reference-request'])}
        result=take_dependency_contract(dict(assets=assets),['recording','listening'])
        self.assertEqual(set(result['primary_inputs']),{assets[k]['sha256'] for k in ['source','request','runtime','backend']})
        self.assertEqual(set(result['outputs']),{assets[k]['sha256'] for k in ['recording','listening']})
        self.assertEqual(set(result['references']),{assets[k]['sha256'] for k in ['job','job_result','timing-reference-request']})
        self.assertEqual(sum(map(len,result.values())),len(assets))

    def test_input_output_alias_fails(self):
        with self.assertRaisesRegex(ValueError,'overlap'):
            take_dependency_contract(dict(assets={'source':dict(sha256='a'*64),'recording':dict(sha256='a'*64)}),['recording'])

if __name__=='__main__':unittest.main()
