# Third-party notices

Tessera Launcher ships with the open-source software below. Thank you to everyone who made it.
The full list, with every license text, is also at <https://tesseralauncher.com/licenses>.

Tessera is not an official Minecraft product. Not approved by or associated with Mojang or Microsoft.

## Fonts (bundled in the app)

| Font | Copyright | License |
|---|---|---|
| [Inter](https://github.com/rsms/inter) | Copyright 2016 The Inter Project Authors | SIL Open Font License 1.1 |
| [Montserrat](https://github.com/JulietaUla/Montserrat) | Copyright 2011 The Montserrat Project Authors | SIL Open Font License 1.1 |
| [Geist Mono](https://github.com/vercel/geist-font) | Copyright 2024 The Geist Project Authors | SIL Open Font License 1.1 |

Packaged by [Fontsource](https://fontsource.org). The OFL text is at the end of this file.

## JavaScript

| Package | License |
|---|---|
| [Lucide](https://lucide.dev) (`lucide-vue-next`), icons | ISC; portions from Feather under MIT |
| [skinview3d](https://github.com/bs-community/skinview3d), the 3D skin viewer | MIT |
| [three.js](https://threejs.org), used by skinview3d | MIT, Copyright © 2010–2024 three.js authors |
| [Vue](https://vuejs.org) and [Vue Router](https://router.vuejs.org) | MIT, Copyright (c) 2018–present Evan You and contributors |
| [Tauri](https://tauri.app) JavaScript APIs and plugins (`@tauri-apps/*`) | MIT OR Apache-2.0 |

## Rust crates

The app is built on [Tauri](https://tauri.app) (MIT OR Apache-2.0) and about 530 other crates. Licenses
of the crates in `launcher/src-tauri/Cargo.lock` (from `cargo metadata`, every platform; 2026-10-31):

| Crates | License |
|---:|---|
| 316 | MIT OR Apache-2.0 (any spelling) |
| 121 | MIT |
| 29 | MIT OR Apache-2.0, with further choices (Zlib, BSD-3-Clause, 0BSD, CC0-1.0, BSL-1.0) |
| 19 | Unicode-3.0 (one with MIT OR Apache-2.0 as well) |
| 11 | Unlicense OR MIT |
| 6 | Apache-2.0 WITH LLVM-exception, alone or as one choice |
| 6 | ISC, or Apache-2.0 OR ISC OR MIT |
| 5 | BSD-3-Clause, or BSD-3-Clause with MIT |
| 5 | MPL-2.0 (cssparser, cssparser-macros, dtoa-short, option-ext, selectors; unmodified, source at crates.io) |
| 3 | Apache-2.0 (sync_wrapper, tao, zopfli) |
| 2 | Zlib (foldhash, zlib-rs) |
| 2 | CDLA-Permissive-2.0 (webpki-roots, webpki-root-certs: Mozilla's root certificates) |
| 2 | Apache-2.0 AND ISC (ring), Apache-2.0 AND MIT (dpi) |
| 2 | MIT OR Apache-2.0 OR LGPL-2.1-or-later (r-efi; used under MIT or Apache-2.0) |

To list every crate with its license text, run in `launcher/src-tauri`:

```sh
cargo install cargo-about && cargo about generate about.hbs > licenses.html
# or, only names and licenses:
cargo install cargo-license && cargo license
```

## Lucide: ISC License

```text
ISC License

Copyright (c) for portions of Lucide are held by Cole Bemis 2013-2026 as part of Feather (MIT). All other copyright (c) for Lucide are held by Lucide Contributors 2026.

Permission to use, copy, modify, and/or distribute this software for any
purpose with or without fee is hereby granted, provided that the above
copyright notice and this permission notice appear in all copies.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.

---

The MIT License (MIT) (for portions derived from Feather)

Copyright (c) 2013-2026 Cole Bemis

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## skinview3d: MIT License

```text
MIT License

Copyright (c) 2014-2018 Kent Rasmussen
Copyright (c) 2017-2022 Haowei Wen, Sean Boult and contributors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## SIL Open Font License 1.1

Applies to Inter, Montserrat and Geist Mono (copyright lines above).

```text
-----------------------------------------------------------
SIL OPEN FONT LICENSE Version 1.1 - 26 February 2007
-----------------------------------------------------------

PREAMBLE
The goals of the Open Font License (OFL) are to stimulate worldwide
development of collaborative font projects, to support the font creation
efforts of academic and linguistic communities, and to provide a free and
open framework in which fonts may be shared and improved in partnership
with others.

The OFL allows the licensed fonts to be used, studied, modified and
redistributed freely as long as they are not sold by themselves. The
fonts, including any derivative works, can be bundled, embedded,
redistributed and/or sold with any software provided that any reserved
names are not used by derivative works. The fonts and derivatives,
however, cannot be released under any other type of license. The
requirement for fonts to remain under this license does not apply
to any document created using the fonts or their derivatives.

DEFINITIONS
"Font Software" refers to the set of files released by the Copyright
Holder(s) under this license and clearly marked as such. This may
include source files, build scripts and documentation.

"Reserved Font Name" refers to any names specified as such after the
copyright statement(s).

"Original Version" refers to the collection of Font Software components as
distributed by the Copyright Holder(s).

"Modified Version" refers to any derivative made by adding to, deleting,
or substituting -- in part or in whole -- any of the components of the
Original Version, by changing formats or by porting the Font Software to a
new environment.

"Author" refers to any designer, engineer, programmer, technical
writer or other person who contributed to the Font Software.

PERMISSION & CONDITIONS
Permission is hereby granted, free of charge, to any person obtaining
a copy of the Font Software, to use, study, copy, merge, embed, modify,
redistribute, and sell modified and unmodified copies of the Font
Software, subject to the following conditions:

1) Neither the Font Software nor any of its individual components,
in Original or Modified Versions, may be sold by itself.

2) Original or Modified Versions of the Font Software may be bundled,
redistributed and/or sold with any software, provided that each copy
contains the above copyright notice and this license. These can be
included either as stand-alone text files, human-readable headers or
in the appropriate machine-readable metadata fields within text or
binary files as long as those fields can be easily viewed by the user.

3) No Modified Version of the Font Software may use the Reserved Font
Name(s) unless explicit written permission is granted by the corresponding
Copyright Holder. This restriction only applies to the primary font name as
presented to the users.

4) The name(s) of the Copyright Holder(s) or the Author(s) of the Font
Software shall not be used to promote, endorse or advertise any
Modified Version, except to acknowledge the contribution(s) of the
Copyright Holder(s) and the Author(s) or with their explicit written
permission.

5) The Font Software, modified or unmodified, in part or in whole,
must be distributed entirely under this license, and must not be
distributed under any other license. The requirement for fonts to
remain under this license does not apply to any document created
using the Font Software.

TERMINATION
This license becomes null and void if any of the above conditions are
not met.

DISCLAIMER
THE FONT SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO ANY WARRANTIES OF
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT
OF COPYRIGHT, PATENT, TRADEMARK, OR OTHER RIGHT. IN NO EVENT SHALL THE
COPYRIGHT HOLDER BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY,
INCLUDING ANY GENERAL, SPECIAL, INDIRECT, INCIDENTAL, OR CONSEQUENTIAL
DAMAGES, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
FROM, OUT OF THE USE OR INABILITY TO USE THE FONT SOFTWARE OR FROM
OTHER DEALINGS IN THE FONT SOFTWARE.
```
