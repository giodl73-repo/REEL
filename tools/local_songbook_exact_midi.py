"""Exact score-relative editable MIDI from retained measured MusicXML intervals."""
from collections import Counter
from fractions import Fraction
from math import lcm
from pathlib import Path
import xml.etree.ElementTree as ET
from local_song_backend import checked, offline, read, write, descriptor
from local_instrumental_engrave import intervals
from local_songbook_legacy_exports import midi_notes, compare

def export(source,target):
    import mido
    measured=intervals(source);ppq=1
    for (_,pitch,start,end) in measured:
        ppq=lcm(ppq,start.denominator,end.denominator)
    if ppq>32767:raise ValueError('Exact score clock exceeds MIDI PPQ representation')
    ppq=lcm(ppq,480) if lcm(ppq,480)<=32767 else ppq
    midi=mido.MidiFile(type=1,ticks_per_beat=ppq)
    for part in sorted({x[0] for x in measured}):
        track=mido.MidiTrack();midi.tracks.append(track);events=[]
        for (pi,pitch,start,end),count in measured.items():
            if pi!=part:continue
            for _ in range(count):events.extend([(int(start*ppq),1,pitch),(int(end*ppq),0,pitch)])
        previous=0
        for tick,on,pitch in sorted(events):
            track.append(mido.Message('note_on' if on else 'note_off',channel=0,note=pitch,velocity=80 if on else 0,time=tick-previous));previous=tick
    midi.save(target);notes,actual_ppq=midi_notes(target);report=compare(measured,notes,actual_ppq)
    if not report['nominal_onsets_within_two_ticks'] or report['maximum_duration_error_quarters']!=0:raise ValueError('Exact MIDI clock differs')
    report.update(clock_scope='Exact nominal score quarters; MIDI default tempo 120 BPM is provisional and is not the recording clock',source=descriptor(source),midi=descriptor(target),source_score_modified=False)
    return report

def main():
    offline();q=read('midi-request.json');checked('score.musicxml',q['musicxml']);write('midi-check.json',export('score.musicxml','editable.mid'))
if __name__=='__main__':main()
