# Current source-sensitive key contract

Frozen handler1cad4ce is authoritative; contexts and modifier priority matter. This is documentation, not native-layout verification.

| Context | Oracle key | Native spelling/additive alias | Contract |
|---|---|---|---|
| Normal file list | Right | right | enter directory/open selected file through default app |
| Normal file list | Enter | enter | start rename; never open by default |
| Normal file list | plain letters including l | same logical letter | common-folder/bookmark lookup after explicit actions; l is not an open alias |
| Search | Right / Enter | right / enter | enter/open / confirm filter respectively |
| Editor | Ctrl+A / Ctrl+C / plain s | ctrl-a / ctrl-c / s | line start / quit / insert s; Cmd+A/C clipboard/select aliases do not override oracle |
| Native text input | clipboard/undo additive | Cmd+C/X/V/Z, Ctrl+Shift+C/X/V | native clipboard aliases preserve Ctrl+C quit |
| Normal list j/k | j / k | same letter | common-folder/bookmark lookup, not movement |
| Fuzzy search | Alt-Up / Alt-Down | alt-up / alt-down | top / bottom exclusively, no additional step |
| Normal list bookmarks | n | n | explicit create action shadows allocated bookmark n, preserve G0015 |
| Bookmark navigation | assigned shortcut | same shortcut | retain existing pane filter, preserve G0017 |

Actual OS modifier/layout/IME and menu/window aliases remain UNVERIFIED; see T016/T026 for narrow headless proof, source-facts.md for contradictions and frozen anchors.
