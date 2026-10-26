"""Checked score-application exports from retained measured MusicXML.

This bridges an existing score, never transcribes a recording or replaces the
native REEL model exporter. Application MIDI is a score-relative performance
derivative: playback gates and clock differences are reported explicitly.
"""
from collections import Counter, defaultdict
from fractions import Fraction
from pathlib import Path
import os
import subprocess
from local_song_backend import checked, descriptor, offline, read, write
from local_instrumental_engrave import intervals, main as engrave


def midi_notes(path):
    import mido
    midi = mido.MidiFile(path)
    if midi.type not in (0, 1) or midi.ticks_per_beat <= 0:
        raise ValueError('Unsupported editable MIDI clock')
    result = []
    for track in midi.tracks:
        clock = 0
        pending = defaultdict(list)
        for event in track:
            clock += event.time
            if event.type == 'note_on' and event.velocity:
                pending[(event.channel, event.note)].append(clock)
            elif event.type == 'note_off' or event.type == 'note_on' and not event.velocity:
                key = (event.channel, event.note)
                if not pending[key]:
                    raise ValueError('MIDI contains an unmatched note end')
                start = pending[key].pop(0)
                if clock <= start:
                    raise ValueError('MIDI contains a nonpositive note duration')
                result.append((event.note, Fraction(start, midi.ticks_per_beat),
                               Fraction(clock, midi.ticks_per_beat)))
        if any(pending.values()):
            raise ValueError('MIDI contains an unfinished note')
    if not result:
        raise ValueError('MIDI has no pitched events')
    return result, midi.ticks_per_beat


def compare(xml_intervals, notes, ppq):
    expected = []
    for (_, pitch, start, end), count in xml_intervals.items():
        expected.extend([(pitch, start, end)] * count)
    if Counter(x[0] for x in expected) != Counter(x[0] for x in notes):
        raise ValueError('Score application MIDI changed pitches or note counts')
    errors = []
    for before, after in zip(sorted(expected), sorted(notes)):
        errors.append(dict(pitch=before[0],
                           onset_error_quarters=float(after[1]-before[1]),
                           duration_error_quarters=float((after[2]-after[1])-(before[2]-before[1]))))
    return dict(source_notes=len(expected), midi_notes=len(notes), ppq=ppq,
                pitch_counts_equal=True,
                nominal_onsets_within_two_ticks=all(abs(x['onset_error_quarters']) <= 2/ppq for x in errors),
                maximum_onset_error_quarters=max(abs(x['onset_error_quarters']) for x in errors),
                maximum_duration_error_quarters=max(abs(x['duration_error_quarters']) for x in errors),
                differences=errors,
                clock_scope='Score-application performance MIDI; not original-recording-clock evidence',
                articulation_review_required=True, human_musical_approval=False)


def main():
    offline()
    q = read('engraving-request.json')
    checked('score.musicxml', q['musicxml'])
    # The existing adapter owns duration spelling, engraving and the exact
    # MusicXML interval round trip, including retained rendered-page QA.
    engrave()
    executable = checked(q['musescore']['path'], q['musescore'])
    env = os.environ.copy()
    env['QT_QPA_PLATFORM'] = 'windows:fontengine=freetype'
    env['QT_LOGGING_RULES'] = '*.debug=false'
    result = subprocess.run([str(executable), '-f', '-o', 'editable.mid',
                             'engraving-input.musicxml'], env=env,
                            capture_output=True, text=True, encoding='utf-8',
                            errors='replace', timeout=600)
    Path('editable.mid.log').write_text(result.stdout+'\n'+result.stderr, encoding='utf-8')
    if result.returncode or not Path('editable.mid').is_file():
        raise ValueError('Score application did not export editable MIDI')
    notes, ppq = midi_notes('editable.mid')
    report = compare(intervals('score.musicxml'), notes, ppq)
    report.update(source=q['musicxml'], midi=descriptor('editable.mid'),
                  source_score_unchanged=True,
                  exporter='Explicitly hash-pinned installed score application')
    write('midi-check.json', report)
    print('LEGACY SCORE EXPORT VERIFIED', report['source_notes'], 'notes; score-relative MIDI', flush=True)


if __name__ == '__main__':
    main()
