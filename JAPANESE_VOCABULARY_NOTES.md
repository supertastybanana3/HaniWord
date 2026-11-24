# Japanese Vocabulary Supplement

This is an original, manually curated collection of spellings and readings, not
an imported dictionary or model-training dataset. The compilation is dedicated
to the public domain under CC0-1.0. It contains spellings, readings, categories
and fixed insertion bands only: no source posts, definitions, author names,
account identifiers, search URLs or usage counts.

General ranking policy: the published order uses fixed curated bands and a
stable order, not any individual's typing history. Normal private IME word
learning remains separate from the published vocabulary.

The words were chosen by hand in October 2026 after checking that each spelling
is in recent public use. This is not a popularity ranking. The rare spelling
`確横` was investigated but not added.

Admission abbreviations are also observed spellings, not all combinations of
university names and faculties. 理一/理二/理三 and 文一/文二/文三 are the University
of Tokyo's entrance course divisions (科類), not faculty names.

The supplement is available independently of biology and Latin-language settings.
Exact-reading alternatives preserve unrelated candidates' relative order.
Their insertion slots are fixed curated bands, stored as `slot` in the JSON:
after 5, 10 or 18 existing candidates. Ambiguous readings have a minimum slot of
16. These are insertion slots, not changes to zenz scores or an assertion of
corpus-wide frequency.
Shorter lists clamp the slot before the final kana fallback candidates.
Ordinary standalone abbreviations keep their selected
output. Only narrow admission continuations (志望, に行きたい) promote them.
死亡率, 脂肪を減らす and 死亡した are not admission continuations.

Roman-number spellings have equal priority with the 漢数字 spellings:
理Ⅰ/理Ⅱ/理Ⅲ and 文Ⅰ/文Ⅱ/文Ⅲ. These are explicit variants of the corresponding
漢数字 entry. They share its insertion slot and collision guard, and sit next to
the first spelling already ranked by the dictionary/model/learning. If neither
is present, the stable order is 漢数字 followed by Roman numeral. This preference
does not assert equal usage, retrain zenz, or globally replace numbers in other
words.

Only 運ガキ, ギュ鳴らし and ギュられる prefer their curated spelling in standalone
input. A small set of nominal continuations also preserves 運ガキ and ギュ鳴らし.
`運が気になる`, `運が切れた`, `コミュの話`, `仰げば尊し` and technical vocabulary
must not be forcibly respelled. Known compounds share one candidate scope while
following particles stay separate, without changing ordinary selected text.

The original 64 spellings (69 reading entries), plus six explicit Roman-number
variants, cover 37 slang terms, 6 admission
abbreviations and 21 subject abbreviations. Subject abbreviations were checked
in Japanese school contexts (for example, not the Chinese uses of 地基). 化基 is
weaker than 英コミュ or 論表 and cannot displace 書き/柿. 共テ and 政経 are already
present and are tested without adding duplicate entries. Spellings and known
reading variants are curated separately.

Adjacent parts of speech were reviewed separately with SudachiPy 0.6.11 and
SudachiDict-small 20260723 (short-unit/A mode), in an isolated research environment.
The tokenizer's guessed POS for a new term is not accepted as its true POS.
No source text is included. This analyzer and dictionary are not included in the
runtime or used to retrain zenz.

These reviews inform manual curation and regression cases, not automatic
transition scores or evidence that a word was newly coined. Curated verb,
i-adjective and interjection labels prevent noun-continuation rules from applying
to ギュられる/ギュる/メロがる, メロい and エッホエッホ. Full inflection handling and
empirical preceding/following-POS reranking are not yet implemented. Existing
dictionary grammar is still used for other segmentation and connections.
