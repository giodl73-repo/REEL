"""Editable word-to-note proposals from independent recognition clocks.

This intermediate ledger is deliberately not a MusicModel lyric layer: the
native lead-sheet contract requires every melody note mapped exactly once.
Unknown words, shared pitch runs and alternative phrase occurrences stay open.
"""
import math
import re
import html
from pathlib import Path

from local_song_backend import checked, offline, phrase_candidates, read, words, write


def propose(performed, alignment, analysis, model, listening_windows=()):
    tempo = model['tempo_map']
    if len(tempo) != 1 or tempo[0]['tick'] != 0:
        raise ValueError('Variable tempo requires an explicit source-clock map')
    scale = tempo[0]['microseconds_per_quarter'] / 1e6 / model['musical_timebase']['pulses_per_quarter']
    notes = [dict(id=n['id'], start=n['start_tick']*scale,
                  end=(n['start_tick']+n['duration_ticks'])*scale)
             for p in model['parts'] for n in p['notes']]
    if len({n['id'] for n in notes}) != len(notes):
        raise ValueError('Duplicate note IDs')
    duration = model['duration_ticks']*scale
    passes = {p['name']: p['tokens'] for p in analysis['passes']}
    if len(passes) != len(analysis['passes']):
        raise ValueError('Recognition pass names must be unique')
    source_lines = [m for m in re.finditer(r'[^\r\n]+', performed) if m.group().strip()]
    if len(source_lines) != len(alignment['lines']):
        raise ValueError('Source/alignment line cardinality differs')
    phrases = []
    for line, match in zip(alignment['lines'], source_lines):
        number = line['line']
        if number in alignment['owner_accepted_omissions']:
            continue
        text = match.group()
        if text.strip() != line['performed_source_line'].strip():
            raise ValueError('Alignment wording differs from bound source')
        spans = list(re.finditer(r'[^\W\d_]+', text, re.UNICODE))
        expected = words(text)
        if [words(s.group()) for s in spans] != [[w] for w in expected]:
            raise ValueError('Explicit word correspondence required')
        for occurrence, candidate in enumerate(line['candidates'], 1):
            tokens = passes[candidate['pass_name']]
            mappings = []
            for mapping in phrase_candidates(expected, tokens):
                clocks = [tokens[i]['span'] for i in mapping.values()]
                if (len(expected)-len(mapping) == candidate['missing_words']
                    and abs(min(s[0] for s in clocks)-candidate['start']) < 1e-6
                    and abs(max(s[1] for s in clocks)-candidate['end']) < 1e-6):
                    mappings.append(mapping)
            if not mappings:
                raise ValueError('Candidate cannot be reproduced in its independent pass')
            # Different internal token mappings with identical endpoints remain
            # ambiguous; never choose one silently.
            mapping = mappings[0] if len(mappings) == 1 else {}
            items = []
            for i, span in enumerate(spans):
                start = len(performed[:match.start()+span.start()].encode('utf-8'))
                end = len(performed[:match.start()+span.end()].encode('utf-8'))
                item = dict(word=span.group(), text_start_byte=start, text_end_byte=end,
                            token_index=mapping.get(i), note_ids=[], status='word-clock-unresolved')
                if i in mapping:
                    token = tokens[mapping[i]]
                    t0, t1 = token['span']
                    item['recognized_word'] = token['word']
                    item['source_span_seconds'] = [t0, t1]
                    if all(type(t) in (int, float) and math.isfinite(t) for t in [t0, t1]) and 0 <= t0 < t1 <= duration+.02 and t1-t0 <= 3:
                        item['note_ids'] = [n['id'] for n in notes if min(t1, n['end'])-max(t0, n['start']) >= .04]
                        item['status'] = 'word-to-note-proposal' if item['note_ids'] else 'no-recovered-note'
                    else:
                        item['status'] = 'coarse-or-invalid-word-clock'
                items.append(item)
            shared = {n for item in items for n in item['note_ids']
                      if sum(n in other['note_ids'] for other in items) > 1}
            for item in items:
                if shared.intersection(item['note_ids']):
                    item['status'] = 'shared-pitch-run; articulation-review-required'
            phrases.append(dict(id=f'line-{number}-occurrence-{occurrence}', line=number,
                                canonical=line['canonical'], performed_source_line=text,
                                pass_name=candidate['pass_name'],
                                span_seconds=[candidate['start'], candidate['end']],
                                alternative_mapping_count=len(mappings), words=items,
                                selected=False, syllables_assigned=False))
    # Token reuse across different source lines is an alternative, not two
    # independently established sung phrases. Repeats at different times remain.
    for phrase in phrases:
        for item in phrase['words']:
            if item['token_index'] is not None and any(
                other['line'] != phrase['line'] and other['pass_name'] == phrase['pass_name']
                and any(w['token_index'] == item['token_index'] for w in other['words'])
                for other in phrases):
                item['status'] = 'token-reused-by-other-line; phrase-review-required'
    windows = []
    by_line = {line['line']: line for line in alignment['lines']}
    for window in listening_windows:
        line = by_line[window['line']]
        if line['line'] in alignment['owner_accepted_omissions']:
            raise ValueError('An accepted omission cannot receive a listening-window binding')
        start, end = window['span_seconds']
        if not 0 <= start < end <= duration or end-start > 15:
            raise ValueError('Listening window outside bounded recording clock')
        windows.append(dict(window, canonical=line['canonical'], underlay_assigned=False,
                            status='producer-proposed listening window; not lexical confirmation'))
    return dict(schema='reel.local-song-word-underlay-proposal.v1', phrases=phrases,
                listening_windows=windows, complete_canonical_lines=[r['canonical'] for r in alignment['lines']],
                owner_accepted_omissions=alignment['owner_accepted_omissions'],
                unresolved_line_clocks=alignment['unresolved_lines'],
                total_model_notes=len(notes),
                unique_notes_with_word_proposals=len({n for p in phrases for w in p['words'] for n in w['note_ids']}),
                word_proposals=sum(w['status']=='word-to-note-proposal' for p in phrases for w in p['words']),
                source_audio_modified=False, pitches_modified=False, native_model_modified=False,
                native_underlay_export_ready=False, human_review='pending',
                limitations=['Word clocks are independent recognition hypotheses, not measured syllable boundaries.',
                             'Phrase alternatives and repeated performances require listening selection.',
                             'Shared pitch runs need articulation review; uncovered notes stay unmapped.',
                             'Do not fill the native every-note underlay contract with invented syllables.'])


def main():
    offline()
    request = read('underlay.json')
    if request['schema'] != 'reel.local-song-word-underlay-request.v1':
        raise ValueError('Unsupported request')
    for filename, key in [('performed.txt', 'performed'), ('analysis.json', 'analysis'),
                          ('model.json', 'model'), ('line-alignment.json', 'line_alignment')]:
        checked(filename, request[key])
    alignment, analysis, model = read('line-alignment.json'), read('analysis.json'), read('model.json')
    original = alignment['request']
    for key in ['performed', 'analysis', 'model', 'recording']:
        if request[key] != original[key]:
            raise ValueError('Underlay input differs from frozen alignment source')
    if analysis['recording']['sha256'] != request['recording']['sha256']:
        raise ValueError('Recognition belongs to another recording')
    report = propose(Path('performed.txt').read_text(encoding='utf-8'), alignment, analysis, model,
                     request.get('listening_windows', []))
    report['request'] = request
    write('word-underlay.json', report)
    print('WORD UNDERLAY PROPOSALS', report['word_proposals'], 'unselected; no source/model changes', flush=True)


def render_review(entries):
    """Local, server-free review: seek buttons always target original full audio."""
    esc = html.escape
    sections = []
    def button(audio_id, start, end, label):
        return (f'<button data-audio="{audio_id}" data-start="{max(0,start-1):.4f}" '
                f'data-end="{end+1:.4f}">{esc(label)}</button>')
    for index, entry in enumerate(entries):
        report = entry['report']
        audio_id = f'audio-{index}'
        rows = []
        for window in report['listening_windows']:
            start, end = window['span_seconds']
            rows.append(f'<p class="hold">Line {window["line"]}: {esc(window["canonical"])} '
                        + button(audio_id, start, end, f'Listen {start:.2f}–{end:.2f}s')
                        + f'<br>{esc(window.get("rationale", ""))} — listening window only; word underlay unresolved.</p>')
        for line, text in enumerate(report['complete_canonical_lines'], 1):
            if line in report['owner_accepted_omissions']:
                rows.append(f'<p class="hold">{line}. {esc(text)} — accepted performance omission; retained for a future rendition.</p>')
                continue
            phrases = [p for p in report['phrases'] if p['line'] == line]
            details = []
            for phrase in phrases:
                start, end = phrase['span_seconds']
                table = ''.join('<tr><td>'+esc(w['word'])+'</td><td>'
                                +esc(str(w.get('source_span_seconds', 'unresolved')))+'</td><td>'
                                +esc(', '.join(w['note_ids']) or 'unmapped')+'</td><td>'
                                +esc(w['status'])+'</td></tr>' for w in phrase['words'])
                details.append('<details><summary>'+button(audio_id, start, end, f'{start:.2f}–{end:.2f}s')
                               +f' {esc(phrase["pass_name"])} · unselected candidate</summary>'
                               +'<table><tr><th>Performed-source word</th><th>Seconds</th><th>Existing notes</th><th>Review state</th></tr>'
                               +table+'</table></details>')
            rows.append(f'<div class="line"><b>{line}. {esc(text)}</b>'
                        +(''.join(details) if details else '<p>Word clock unresolved.</p>')+'</div>')
        sections.append(f'<section><h2>{esc(entry["title"])}</h2><audio id="{audio_id}" controls preload="metadata" src="{esc(entry["audio_uri"],quote=True)}"></audio>'
                        +f'<p>{report["word_proposals"]} unambiguous word-to-note proposals within individual phrase candidates. '
                        +f'{report["unique_notes_with_word_proposals"]}/{report["total_model_notes"]} recovered note runs overlap word clocks across alternatives; this is not melody completeness.</p>'
                        +'<p>Canonical wording is shown in bold. Candidate tables preserve performed-source spelling separately. '
                        +'Every occurrence remains unselected; repeats and alternative recognition passes need listening review.</p>'
                        +f'<p><a href="{esc(entry["report_uri"],quote=True)}">Editable proposal JSON</a></p>'
                        +''.join(rows)+'</section>')
    return '''<!doctype html><html lang="en"><meta charset="utf-8"><title>Full Golden lyric-underlay draft</title>
<style>body{font:17px system-ui;max-width:1100px;margin:32px auto;padding:0 20px;background:#faf8f2;color:#222}section{margin:35px 0;padding:20px;background:white;border:1px solid #ddd}audio{width:100%}.line{margin:16px 0}details{margin:5px 0}button{padding:6px 12px;cursor:pointer}table{border-collapse:collapse;font-size:14px;width:100%}td,th{text-align:left;padding:7px;border:1px solid #ddd}.hold{padding:12px;background:#fff0c7}</style>
<h1>Full Golden lyric-underlay draft</h1><p>Producer-applied proposals · musical review pending. Original full recordings and accepted melody drafts are unchanged. These are word-clock proposals, not finished syllable engraving. Playback buttons include one second of context.</p>
''' + ''.join(sections) + '''<script>
document.querySelectorAll('button[data-audio]').forEach(b=>b.addEventListener('click',()=>{
document.querySelectorAll('audio').forEach(a=>a.pause());
const a=document.getElementById(b.dataset.audio);a.currentTime=Number(b.dataset.start);a.dataset.stop=b.dataset.end;
a.play().catch(e=>alert('Playback could not start: '+e.message));}));
document.querySelectorAll('audio').forEach(a=>a.addEventListener('timeupdate',()=>{if(a.dataset.stop&&a.currentTime>=Number(a.dataset.stop)){a.pause();delete a.dataset.stop;}}));
</script></html>'''


if __name__ == '__main__':
    main()
