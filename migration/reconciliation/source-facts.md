# T030 source reconciliation

Oracle remains tui-oracle-baseline ->1cad4ce43cc72d52d4cc4eef920e0da22cb69568. All facts below are from that source, not from new GPUI behavior.

## F083 open vs rename

src/handler.rs:342 dispatches KeyCode::Right to app.enter_folder(); :346 dispatches normal Enter to app.start_rename(). src/app.rs:2206 enter_folder enters directories or opens a non-directory via system default app. Search Enter confirms its filter (:188), search Right enters (:191); dialog/editor contexts keep their own Enter behavior. The old INV-PE-010-derived enter:open-default and l:open-default-or-directory surfaces were documentation errors: plain letters dispatch common-folder/bookmark shortcuts (:332), so l is not a dedicated open action. Removed only these false semantic surfaces from canonical/per-slice lists and ownership; normal Enter/Right and rename surfaces remain. Historical doc text remains recoverable at d3e46e5; no runtime key change. The same frozen handler has no dedicated j/k movement; generic letters perform common/bookmark lookup. Fuzzy search Alt-Up/Alt-Down select top/bottom using an exclusive else, so they do not additionally step prev/next. Historical inventory claims to the contrary are superseded by these frozen-source facts, not silently treated as source truth.

## F152 drive refresh

src/app.rs:863 start_drive_poller starts one worker, list_drives() runs off render, polls every two seconds, retains cache on errors, increments generation only for changed drives. :959 refresh_drives suppresses equal seen generations and consumes latest cache. This previously omitted observable capability is F152 with five precise surfaces; it is not inferred as complete from broad F014/F140. Native runtime window-generation guards are additive UX, covered only by named headless tests; physical hotplug/platform errors still UNVERIFIED.

## Bookmark quirks retained

src/services/bookmarks.rs:7 KEYBOARD_ORDER includes n; RESERVED_KEYS:16 omits n. src/handler.rs normal create-n case precedes generic bookmark dispatch, so allocator can assign an unreachable bookmark n (G0015). src/app.rs:2192 set_folder_from_bookmark clears search_query but does not clear pane.filter_query/filter_indices, unlike enter_folder's explicit clear (G0017). These are oracle inconsistencies, not permission to normalize desktop behavior. Exhaustion text still says a-p while allocation uses keyboard order; preserve exact message.

## Safety provenance

Current approved-root-changes.json pins only exact transfer source/test blobs from32768c0/7f3456b/4290fde. No editor-root allowance exists yet. T028/new transfer fixes need reviewed exact commit/blob/hash plus regression links and explicit fixture update. Structural allowance is not safety approval: G0039/G0046 findings remain on this reviewed older snapshot. The oracle is never retagged to absorb fixes.
