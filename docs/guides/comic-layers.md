# Comic issues, pages and editable layers

[All illustrated guides](../README.md#illustrated-guides) · [Development status](../development-status.md)

The optional comic publication CLI composes private review candidates from selected issue sources. It preserves the exact selected inputs and the editable layers.

An issue orders pages. Each page declares panel geometry, reading order, art
layers and lettering. Artwork and text remain independent inputs to the reading
edition, allowing clean art and translated reading editions to coexist.

```mermaid
flowchart TD
    accTitle: Editable comic layers
    accDescr: An issue orders pages with panel geometry. Background, character, prop and effect layers compose clean art. Independent lettering combines with that art into a reading page or issue PDF.
    issue["Issue: ordered pages"] --> page["Page: panels + reading order"]
    page --> background["Background layer"]
    page --> actors["Character and prop layers"]
    page --> effects["Effect layers"]
    page --> text["Lettering: balloons + text + fonts"]
    background --> clean["Clean artwork"]
    actors --> clean
    effects --> clean
    clean --> reading["Reading page / issue PDF"]
    text --> reading
```

## A lettering correction

In an illustrative page, a balloon overlaps a character's hand. Correct the
balloon's position as a new lettering version, retain the exact art sources,
then compose and review a new reading page. The clean-art output can stay the
same. The surrounding page/spread and issue reading order still need review.

| Source | Owns | Example change |
| --- | --- | --- |
| Page/layout | Panel rectangles and ordered panel/utterance IDs | Change panel reading order |
| Art layers | Separate background, character, prop and effect PNG inputs | Replace a character pose |
| Lettering | Balloon/text SVG, exact words, font and placement | Move a balloon or supply a translation |
| Edition | Ordered selected page versions and output requirements | Select the corrected reading page |

A translation may need different balloon geometry while sharing clean art.
Recovering a flattened page is insufficient evidence of editability: source
recovery should also support a fresh lettering edit and recomposition.

## Where the work lives

Use the [comic publication CLI](../../crates/reel-publication/README.md) to prepare a selected BBJ book and assemble its private HTML reader. The same CLI checks portable issue plans, assembles reading editions from parts, and restores pinned editable sources. Keep approval, native custody and private delivery in their existing project workflow.

Continue with [assets and revisions](assets-and-revisions.md) for retaining
source relationships, or [editions and delivery](editions-and-delivery.md) for
the distinction between reading output, selection and actual publication.
