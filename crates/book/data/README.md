# Opening name data

`a.tsv` … `e.tsv` are taken verbatim from
[lichess-org/chess-openings](https://github.com/lichess-org/chess-openings)
(ECO volumes A–E, 3,810 named lines).

The upstream project releases them under the CC0 Public Domain Dedication:

> As a collection of facts, this data set is in the public domain. Considerable
> effort was spent curating and cleaning the data. Insofar as that qualifies for
> copyright, the work is released under the CC0 Public Domain Dedication.

No attribution is required; it is given anyway.

Columns are `eco`, `name`, `pgn`. There is no position column — `crates/book/src/eco.rs`
replays each `pgn` with shakmaty to derive the EPD it indexes by. (Upstream generates
files with `uci` and `epd` columns into `dist/`, but that directory is a build artifact
and is not committed, so it cannot be vendored.)

To refresh:

```sh
cd crates/book/data
for f in a b c d e; do
  curl -sfL -o $f.tsv "https://raw.githubusercontent.com/lichess-org/chess-openings/master/$f.tsv"
done
```
