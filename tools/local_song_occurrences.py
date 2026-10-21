"""Per-line source-clock occurrences; these are diagnostics, not a new gate."""
import math
from local_song_backend import phrase_candidates, words


def occurrences(lyrics, passes, not_before=0):
    source=[x for x in lyrics.splitlines() if x.strip()]
    expected_lines=[words(text) for text in source]
    rows=[]
    for number,text in enumerate(source,1):
        expected=expected_lines[number-1];found=[];anchors=set()
        # Near-identical source lines need their distinguishing word for a
        # timing claim. This does not alter the independent coverage gate.
        for other in expected_lines:
            if len(other)==len(expected):
                differences=[i for i,(a,b) in enumerate(zip(expected,other)) if a!=b]
                if len(differences)==1:anchors.update(differences)
        for recognition in passes:
            for mapping in phrase_candidates(expected,recognition['tokens']):
                if not anchors.issubset(mapping):continue
                spans=[recognition['tokens'][i]['span'] for i in mapping.values()]
                if not spans or any(type(t) not in (int,float) or not math.isfinite(t) for span in spans for t in span):continue
                start=min(span[0] for span in spans);end=max(span[1] for span in spans)
                if start<not_before:continue
                found.append(dict(start=start,end=end,pass_name=recognition['name'],matched_words=len(mapping),
                    missing_words=len(expected)-len(mapping)))
        found.sort(key=lambda x:(x['start'],-x['matched_words'],x['end'],x['pass_name']))
        exact=[x for x in found if not x['missing_words']]
        rows.append(dict(line=number,canonical=text,first=found[0] if found else None,
            first_exact=exact[0] if exact else None,candidates=found,distinguishing_word_indices=sorted(anchors)))
    return rows
