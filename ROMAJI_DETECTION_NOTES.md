# Rare Combined Kana And English Boundaries

The 2026-10-12 policy treats uncommon single-sequence small-kana spellings as
weak evidence for Japanese during automatic language detection. It is a user-
requested conservative prior, not a measured population frequency. The underlying
romaji conversion table remains available when no stronger English interpretation
exists. Explicit x/l small-kana commands retain their existing boundary protection.

Examples of the uncommon combinations supported by the current converter:

| Combined Input | Kana | Explicit Alternatives |
| --- | --- | --- |
| dwo | どぉ | doxo / dolo |
| dwa / dwi / dwe | どぁ / どぃ / どぇ | doxa / doxi / doxe, or l equivalents |
| two | とぉ | toxo / tolo |
| twa / twi / twe | とぁ / とぃ / とぇ | toxa / toxi / toxe, or l equivalents |
| swa / swi / swu / swe / swo | すぁ / すぃ / すぅ / すぇ / すぉ | su + xa/xi/xu/xe/xo, or l equivalents |
| tha / the / tho | てゃ / てぇ / てょ | te + xya/xe/xyo, or l equivalents |
| dha / dhe / dho | でゃ / でぇ / でょ | de + xya/xe/xyo, or l equivalents |
| jyi / tyi / cyi / dyi | じぃ / ちぃ / ちぃ / ぢぃ | ji/ti/di + xi/li |
| nyi / hyi / byi / pyi / myi / ryi | にぃ / ひぃ / びぃ / ぴぃ / みぃ / りぃ | ni/hi/bi/pi/mi/ri + xi/li |
| hwa/hwi/hwe/hwo, fwa/fwi/fwu/fwe/fwo | ふ + small vowel | fu + x/l small vowel |

Common sha/cha/fa-family input remains in the normal syllable group. The
preexisting thi/dhi/thu/dhu/twu/dwu and other rare-syllable group also receives
the higher ambiguity penalty. Rare kana transitions cost 4, ordinary kana 0,
and an ordinary known English word 1 in the segmentation heuristic. These are
relative costs, not probabilities; existing priority words have a separate cost.

The `pwdwo` failure additionally exposed a missing lexeme: before the correction,
`pwd` was not in the shipped English detector, and the conversion table could
consume its final `d` together with the particle's `wo`. The public command
supplement now includes pwd, gdb, gcc, awk, sed, grep, chmod, chown, mkdir, rmdir,
ssh and npm. This does not depend on the author's learned vocabulary. Regressions
cover command + wo/de/ni, Japanese prefixes, and ordinary Japanese homophones.

References checked on 2026-10-12:

- Mozc's own input table confirms dwo -> どぉ:
  https://github.com/google/mozc/blob/master/src/data/preedit/hiragana-romanji.tsv
- GNU's pwd documentation identifies the command:
  https://www.gnu.org/s/coreutils/manual/html_node/pwd-invocation.html

No reliable population statistics for use of dwo versus doxo/dolo were found.
The developer PC's keystroke history is not used as population-frequency evidence.
