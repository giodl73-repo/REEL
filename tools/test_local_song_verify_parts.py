import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from local_song_campaign import Campaign


class VerificationPartsTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.addCleanup(self.temp.cleanup)
        self.root=Path(self.temp.name);self.c=Campaign(self.root,self.root/'unused.exe',self.root/'work');self.c.max_verification_bytes=5000
        (self.root/'storage.json').write_text(json.dumps({'capacity':{'max_object_bytes':1000}}))
        (self.root/'asset-project.json').write_text(json.dumps({'storage_config':'storage.json'}))
        self.config=(self.root/'storage.json').read_bytes()
        self.dep=dict(logical_id='input/model',version_id='v1',manifest_sha256='a'*64)
        self.assets={role:dict(sha256=h*64,bytes=n,cache_uri='cache://sha256/'+h*64) for role,h,n in [('input','b',50),('output','c',100),('cold-reference','d',9000)]}
        self.manifest=dict(sha256='e'*64,bytes=100,cache_uri='cache://sha256/'+'e'*64)
        self.package=self.c.save('package.json',dict(kind='song',assets=self.assets,dependencies=[self.dep]))
        self.child=self.c.save('child.json',dict(kind='tool-runtime'))
        self.record=self.c.save('take/artifact.json',dict(schema='caminos.artifact-record.v1',artifact_id='take/a',kind='song',current_version='v1',versions=[dict(version_id='v1',manifest=self.manifest,version_lock=dict(logical_id='take/a',version_id='v1',manifest_sha256='e'*64))]))
        self.definition=self.c.save('take/version.json',dict(logical_id='take/a',version_id='v1',manifest_sha256='e'*64,dependencies=[self.dep],dependency_contract=dict(primary_inputs=['b'*64],outputs=['c'*64],references=['d'*64])))
        self.events=[]

    def native(self,command,*args):
        self.events.append((command,*args))
        if command=='artifact-verify':
            if Path(args[0])==self.record:raise RuntimeError('Artifact verification exceeds read budget')
            return dict(fresh_hash_verified=True,verified_bytes=300)
        if command=='artifact-check':return dict(fresh_hash_verified=False,verification_scope='active-primary-input-and-output-closure')
        if command=='register-version':return dict(existing=True)
        if command=='verify-active':
            if args[0]=='a'*64:return dict(sha256='a'*64,bytes=self.child.stat().st_size,path=str(self.child))
            matches=[a for a in self.assets.values() if a['sha256']==args[0]]
            self.assertTrue(matches);return matches[0]
        self.fail(command)

    def verify(self):
        with patch.object(self.c,'call',side_effect=self.native),patch.object(self.c,'locate',return_value=self.package),patch.object(self.c,'asset_json',return_value={'sha256':'receipt'}):
            return self.c.verify_take(self.record,'v1')

    def test_native_parts_cover_active_inputs_outputs_and_keep_limits(self):
        proof=self.verify()
        self.assertTrue(proof['fresh_hash_verified']);self.assertEqual(proof['verified_bytes'],450)
        verified={e[1] for e in self.events if e[0]=='verify-active'}
        self.assertEqual(verified,{'a'*64,'b'*64,'c'*64})
        self.assertIn(('register-version',self.definition),self.events)
        self.assertEqual((self.root/'storage.json').read_bytes(),self.config)

    def test_no_implicit_opt_in(self):
        self.c.max_verification_bytes=None
        with self.assertRaisesRegex(RuntimeError,'read budget'):self.verify()
        self.assertEqual(len(self.events),1)

    def test_corruption_is_never_a_fallback(self):
        with patch.object(self.c,'call',side_effect=RuntimeError('Corrupt active asset')) as native:
            with self.assertRaisesRegex(RuntimeError,'Corrupt'):self.c.verify_take(self.record,'v1')
        self.assertEqual(native.call_count,1)

    def test_budget_checked_before_dependency_data_reads(self):
        self.c.max_verification_bytes=1000
        with self.assertRaisesRegex(ValueError,'Insufficient explicit budget'):self.verify()
        self.assertFalse(any(e[0]=='verify-active' for e in self.events))

    def test_smaller_than_native_budget_rejected_before_any_reads(self):
        self.c.max_verification_bytes=999
        with self.assertRaisesRegex(ValueError,'unchanged native per-call limit'):self.verify()
        self.assertEqual(self.events,[])

    def test_modified_registration_identity_rejected(self):
        data=json.loads(self.definition.read_text());data['manifest_sha256']='f'*64;self.definition.write_text(json.dumps(data))
        with self.assertRaisesRegex(ValueError,'differs'):self.verify()
        self.assertFalse(any(e[0]=='register-version' for e in self.events))

    def test_native_child_failure_remains_fatal(self):
        original=self.native
        def native(command,*args):
            if command=='artifact-verify' and Path(args[0])!=self.record:raise RuntimeError('Child model corrupt')
            return original(command,*args)
        with patch.object(self.c,'call',side_effect=native),patch.object(self.c,'locate',return_value=self.package):
            with self.assertRaisesRegex(RuntimeError,'corrupt'):self.c.verify_take(self.record,'v1')


if __name__=='__main__':unittest.main()
