# IVS Input

- Shortcut: Ctrl+Shift+Henkan (VK_CONVERT). Ctrl+Henkan remains dictionary/AI switching.
- In conversion, focus a one-ideograph segment, optionally with kana okurigana.
  Live-conversion drafts are also eligible. The shortcut cycles its registered
  sequences and opens the existing candidate list; Space, arrows, numbers and
  Enter use the normal candidate controls.
- After committing, select one ideograph (including any existing IVS) in a TSF
  text editor. The shortcut opens the same candidate list in a composition over
  that selection. Escape/Backspace restores the original selection's text;
  Enter commits the chosen form. This does not invent a phonetic reading or
  teach a new word. No clipboard access or synthesized keystrokes are used.
- Ambiguous selections with multiple ideographs, empty selections, malformed UTF-16 and non-IVD
  characters are not consumed. An editor refusing synchronous TSF edits is not
  modified; its own shortcut handling remains available.
- The standard form without a selector is included, so cycling can remove IVS.
  Explicitly encoded variants (currently U+9AD8/U+9AD9 and U+514E/U+5154) are
  also offered; these are not mislabeled as registered IVS sequences. Their
  Unicode Unihan 17.0.0 kSemanticVariant references are recorded in the embedded
  `ideograph_variants.txt`. The complete semantic-variant relation is not used
  as glyph equivalence.
  Registered selectors use a fixed numeric order, not an estimated frequency.
- Candidates containing IVS use the existing right-side note column. Fonts and
  applications determine whether distinct glyphs can actually be displayed.
  A registered selector, especially the first one, need not look different from
  the unselected standard glyph. Candidate-list visibility is therefore important.
- There is no extra IVS word history or selection-count logging. Normal IME word
  learning is unchanged, including words committed with IVS.

## Data and License

The embedded index is derived from Unicode IVD 2026-08-03:
https://www.unicode.org/ivd/data/2026-08-03/IVD_Sequences.txt

Source SHA256: `2b466659c2bfde1c52e60bd9fc883ab14dfa5c583df024b80352420265d2cd87`.
Identical sequences registered in multiple collections are deduplicated. Only
officially registered base/selector pairs are offered; arbitrary selectors are
never synthesized. The index covers 15,304 base ideographs.

Copyright 2006-2026 Unicode, Inc. Data is used under Unicode License v3, reproduced
in `UNICODE_LICENSE.txt`. No fonts or glyph-chart images are redistributed.

Regeneration (using a separately downloaded, pinned source file and the generator
script `generate_ivs_data.py`, which is kept outside this repository):

```sh
python3 generate_ivs_data.py --source IVD_Sequences.txt \
  --out src/crates/client/data/ivs_sequences.txt
```

Use `--check` to verify deterministic regeneration without writing the index.
