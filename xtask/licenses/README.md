# Standard licence texts

`cargo xtask notices` uses these texts for crates that declare a licence in their
`Cargo.toml` but publish no licence file. The texts are the standard ones, without a
copyright line:

| File | Source |
| --- | --- |
| `Apache-2.0.txt` | the terms of the Apache License 2.0, as published in `serde` 1.0.229 (`LICENSE-APACHE`) |
| `BSL-1.0.txt` | Boost Software License 1.0, as published in `error-code` 3.4.0 (`LICENSE`) |
| `CC0-1.0.txt` | Creative Commons CC0 1.0 Universal, as published in `imgref` 1.12.3 (`LICENSE-CC0`) |
| `IJG.txt` | the terms of the Independent JPEG Group, as published in `jpeg-encoder` 0.7.1 (`LICENSE-IJG`, from "In plain English" on) |
| `BSD-3-Clause.txt`, `MIT.txt`, `Zlib.txt` | the SPDX templates of these licences |

When a new dependency needs a licence that is not here, the command stops and names it:
add its standard text (from <https://spdx.org/licenses/>) and a line to this table.
