# Fonts in the compact window

The compact Vigil window (`ui/compact.slint`) is drawn by the program
itself, so the fonts it uses are built into the program rather than loaded
by a browser. They are the dashboard's own:

| Files | Font | Used for |
| --- | --- | --- |
| `Montserrat-500.ttf`, `-600`, `-700` | [Montserrat](https://github.com/JulietaUla/Montserrat) 9.000 | Names and headings |
| `IBMPlexMono-400.ttf`, `-600`, `-700` | [IBM Plex Mono](https://github.com/IBM/plex) 2.3 | Details and table figures |
| `Lato-300.ttf` | [Lato](https://www.latofonts.com) 1.104 | The large readings |

All three are under the SIL Open Font License 1.1, in `OFL.txt` beside
this file with each font's copyright line. The files are as published on
Google Fonts, unchanged apart from their names: one weight each, Latin
only.

`build.rs` hands them to Slint, which embeds them (`import` lines at the
top of `ui/compact.slint`).
