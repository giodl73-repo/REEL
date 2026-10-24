import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import local_song_arrangement_render as renderer


class BatchTests(unittest.TestCase):
    def request(self,directories):
        permissions=dict(private_only=True,external_upload=False,public_release=False)
        return dict(schema='reel.recording-derived-arrangement-render.v1',stage='batch',permissions=permissions,items=[dict(directory=d,request=dict(schema='reel.recording-derived-arrangement-render.v1',stage='layer',permissions=permissions)) for d in directories])

    def run_batch(self,request,callback):
        before=Path.cwd()
        with tempfile.TemporaryDirectory() as d:
            try:
                os.chdir(d);Path('render-request.json').write_text(json.dumps(request))
                with patch.object(renderer,'layer',side_effect=callback):renderer.main()
                return json.loads(Path('batch-report.json').read_text())
            finally:os.chdir(before)

    def test_independent_case_directories_and_complete_receipt(self):
        visited=[];report=self.run_batch(self.request(['first','second']),lambda q:visited.append(Path.cwd().name));self.assertEqual(visited,['first','second']);self.assertEqual(report['requested'],2);self.assertEqual(len(report['completed']),2);self.assertFalse(report['musical_approval'])

    def test_directory_escape_is_rejected(self):
        with self.assertRaisesRegex(ValueError,'Invalid scoped'):self.run_batch(self.request(['../escape']),lambda q:None)

    def test_failed_case_propagates_and_restores_working_directory(self):
        before=Path.cwd()
        with self.assertRaisesRegex(RuntimeError,'fixture failure'):self.run_batch(self.request(['first']),lambda q:(_ for _ in ()).throw(RuntimeError('fixture failure')))
        self.assertEqual(Path.cwd(),before)

    def test_more_than_fourteen_cases_is_rejected(self):
        with self.assertRaises(ValueError):self.run_batch(self.request([str(i) for i in range(15)]),lambda q:None)

    def test_child_cannot_expand_permissions(self):
        request=self.request(['first']);request['items'][0]['request']['permissions']=dict(private_only=True,external_upload=True,public_release=False)
        with self.assertRaises(ValueError):self.run_batch(request,lambda q:None)


class PresetKeyTests(unittest.TestCase):
    def test_octave_extension_preserves_target_frequency_without_padding(self):
        import numpy as np
        rate=44100;t=np.arange(rate)/rate;sample=np.column_stack([np.sin(2*np.pi*440*t)]*2).astype(np.float32);parent=sample.copy();output,row=renderer.same_preset_sample({'p69':sample},81,.3,rate,12);spectrum=np.abs(np.fft.rfft(output[:,0]));frequency=np.argmax(spectrum)*rate/len(output);self.assertAlmostEqual(frequency,880,delta=2);self.assertEqual(len(output),rate//2);self.assertTrue(np.array_equal(parent,sample));self.assertEqual(row['source_pitch'],69)

    def test_extension_rejects_missing_duration_and_out_of_range_sources(self):
        import numpy as np
        sample=np.ones((100,2),dtype=np.float32)
        with self.assertRaises(ValueError):renderer.same_preset_sample({'p69':sample},81,1,44100,12)
        with self.assertRaises(ValueError):renderer.same_preset_sample({'p69':sample},95,.001,44100,12)


class PlayerMidiTests(unittest.TestCase):
    def test_overlapping_unisons_keep_distinct_voice_routes(self):
        import mido
        with tempfile.TemporaryDirectory() as directory:
            source=Path(directory)/'source.mid';target=Path(directory)/'player.mid';midi=mido.MidiFile();track=mido.MidiTrack();midi.tracks.append(track);track.append(mido.MetaMessage('track_name',name='REEL_PART:["guitar","Guitar"]'));track.extend([mido.Message('note_on',channel=0,note=60,velocity=90,time=0),mido.Message('note_on',channel=1,note=60,velocity=80,time=50),mido.Message('note_off',channel=0,note=60,time=50),mido.Message('note_off',channel=1,note=60,time=50)]);midi.save(source);renderer.assigned_midi(source,[dict(id='guitar',program=24,drums=False)],target);before=renderer.native_parts(source)['guitar'];after=renderer.native_parts(target)['guitar'];self.assertEqual(sorted(n[:4] for n in before),sorted(n[:4] for n in after));self.assertEqual(len(after),2)


if __name__=='__main__':unittest.main()
